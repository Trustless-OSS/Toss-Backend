# Webhook queue operations

The GitHub webhook pipeline (`src/infra/queue.rs`) is a Redis-backed
at-least-once queue with crash-safe delivery. This document covers the key
layout, retry/retention policy, crash-recovery behavior, and how to inspect
and replay dead-lettered jobs.

## Why this exists

The previous implementation used `LPUSH`/`RPOP`: a job was removed from
Redis the instant a worker picked it up, before the handler ran. Killing the
process between pop and completion lost the event permanently. The dedup key
was also written at enqueue time, so a GitHub redelivery of a lost event was
silently swallowed. Webhook handlers create bounties, reserve escrow
balance, push on-chain milestones, and release payouts, so losing an event
means losing money movement with no trace.

## State machine

```text
ready --(claim)--> in-flight (leased) --(ack)-->      completed
                        |
                        +--(fail, attempts left)--> delayed --(promote)--> ready
                        |
                        +--(fail, attempts exhausted)--> dead-letter
```

A **claim** atomically pops a job id from `ready` and records a lease
expiry in the `in-flight` sorted set; the job's envelope stays in the `jobs`
hash until it is acknowledged. A worker that crashes mid-handler leaves the
job sitting in `in-flight` with an expired lease -- it is never lost, just
picked up later by the recovery sweep.

## Redis key layout

All keys are namespaced under `queue:webhooks`:

| Key | Type | Purpose |
| --- | --- | --- |
| `queue:webhooks:ready` | list | job ids ready to be claimed |
| `queue:webhooks:jobs` | hash | `job_id -> envelope JSON` (full job data) |
| `queue:webhooks:inflight` | zset | `job_id -> lease expiry (ms)` |
| `queue:webhooks:delayed` | zset | `job_id -> next attempt time (ms)` |
| `queue:webhooks:dead-letter` | list | job ids that exhausted their retry budget |
| `webhook:delivery:<github-delivery-id>` | string | dedup marker: `{state, job_id}` |

The dedup marker's `state` is one of `queued`, `processing`, `completed`, or
`dead_lettered`. Only `completed` causes a GitHub redelivery to be rejected
outright; `queued`/`processing` markers are cross-checked against the `jobs`
hash so a redelivery of a job Redis actually lost is re-enqueued instead of
dropped.

## Job envelope

Each job in the `jobs` hash is a versioned JSON envelope
(`WebhookJobEnvelope`) containing: `version`, `job_id`, `delivery_id`,
`event`, `action`, `payload`, `attempts`, `received_at`,
`first_attempt_at`, `next_attempt_at`, `lease_expires_at`, `last_error`,
`correlation_id`, and `replay_source` (set on operator replay).

## Retry policy

Configured via environment variables (see `.env.example`):

- `WEBHOOK_MAX_ATTEMPTS` (default `6`): attempts, including the first,
  before a job is dead-lettered.
- `WEBHOOK_LEASE_SECONDS` (default `30`): how long a worker may hold a
  claimed job before it's eligible for crash recovery.
- `WEBHOOK_RETRY_BASE_MS` / `WEBHOOK_RETRY_MAX_MS` (default `1000` /
  `300000`): bounded exponential backoff (`base * 2^attempt`, capped),
  with full jitter -- the actual delay is a random value between `0` and
  the capped delay, so a burst of failures doesn't retry in lockstep.
- `WEBHOOK_COMPLETED_DEDUP_TTL_SECONDS` (default `604800`, 7 days): how
  long a completed delivery's dedup marker survives, i.e. the window in
  which a GitHub redelivery is recognized as a true duplicate.

Retries never block other webhook work: a failed job is moved to the
`delayed` sorted set and the worker loop immediately goes back to claiming
ready jobs. A background sweeper (every 5 seconds) promotes due delayed
jobs back onto `ready` and recovers expired leases.

## Crash / restart recovery

Every 5 seconds, a sweeper scans `queue:webhooks:inflight` for leases whose
expiry has passed. For each one it atomically takes ownership (`ZREM`,
which only one process can win), then routes it through the same
retry-or-dead-letter logic as a normal handler failure, tagged with
`"lease expired: worker crashed or restarted"`. A job can only be recovered
once, even if two sweeper ticks race.

## Duplicate-delivery safety

Two independent layers protect money-moving handlers:

1. **Redis dedup marker** (fast path, described above).
2. **Postgres `webhook_deliveries` table** (`src/modules/github/idempotency.rs`):
   claiming a delivery id is a single `INSERT ... ON CONFLICT DO NOTHING`.
   A delivery already marked `completed` is skipped before the handler
   runs at all; a delivery stuck in `processing` (previous attempt
   crashed) is retried. This is a backstop that still works even if the
   Redis dedup key is lost.

If the idempotency ledger itself is unavailable (database hiccup), the
guard fails open and logs a warning rather than dropping the webhook --
Redis-level dedup still applies.

## Operator API

All endpoints below require the `x-queue-admin-token` header to match
`QUEUE_ADMIN_TOKEN`. If that env var is unset, the endpoints return `403`.

- `GET /api/queue/stats` -- waiting/active/delayed/failed counts (no auth
  required; safe for dashboards).
- `GET /api/queue/webhooks/dead-letter` -- list dead-lettered jobs with
  their full envelope and failure history.
- `POST /api/queue/webhooks/dead-letter/{job_id}/replay` -- replay one job.
  Body: `{"replayed_by": "operator-name"}`. Resets the attempt budget and
  tags the envelope's `replay_source` for the audit trail.
- `POST /api/queue/webhooks/dead-letter` -- batch replay. Body:
  `{"replayed_by": "...", "event": "issues", "action": "labeled", "limit": 50}`
  (`event`/`action` optional filters).

Replay never bypasses handler idempotency: a replayed job still goes
through the same delivery-id claim in `webhook_deliveries` as any other
attempt.

## Graceful shutdown

On `SIGTERM`/`Ctrl-C`, the process stops claiming new webhook jobs
immediately (`QueueInfra::begin_shutdown`), lets the HTTP layer finish
in-flight requests, then waits up to 30 seconds for any job a worker is
currently processing to finish (`QueueInfra::wait_for_idle`) before
exiting. A job still running past that window is left leased; it will be
picked up by the lease-recovery sweep after restart rather than lost.

## Out of scope

This queue is specific to the webhook pipeline. The lower-stakes
`escrow-balance-sync` scheduler job still uses a simple `LPUSH`/`RPOP`
list, since it re-derives its state from the database on every run and
losing a tick just means waiting for the next one.
