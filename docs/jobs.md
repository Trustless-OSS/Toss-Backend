# Background jobs

All background work runs on [BullMQ for Rust](https://docs.bullmq.io/rust/introduction)
(`bullmq-official`), against the same Redis as the cache (`REDIS_URL`). The
custom `queue:webhooks` / `queue:sync` Redis lists and their sequential poller
are gone; there is one job system, not two.

The goal this serves: **a bounty advances by itself.** Maintainers and
contributors do normal GitHub and wallet actions — label, assign, connect a
wallet, merge — and nothing else. No maintainer needs to press a retry button
because a wallet arrived late or Trustless Work timed out.

---

## Hub, producers, workers

The code is split three ways, and the split is enforced: `bullmq` is imported
only by `src/infra/queue.rs` and `src/infra/jobs/`.

| Role | Where | Responsibility |
| --- | --- | --- |
| **Hub** | [`src/infra/queue.rs`](../src/infra/queue.rs) | Creates the queues, workers and scheduler. Exposes `QueueInfra` on `AppState`. |
| **Producers** | GitHub handlers, contributor routes, bounty service | *Only* add jobs, via `state.queue.*`. They never start workers and never move funds. |
| **Workers** | [`src/infra/jobs/`](../src/infra/jobs/) | Run jobs. The only place background work executes. |
| **Rules** | [`src/modules/bounty/automation.rs`](../src/modules/bounty/automation.rs) | The issue state machine. Queue-free — workers call into it. |
| **Money API** | [`src/modules/escrow/`](../src/modules/escrow/) | "Do this action." No queue awareness. Called by workers *after* rules pass. |

BullMQ is the runner, not the rules.

---

## Queues and jobs

| Queue | Job name | Concurrency | What it does |
| --- | --- | --- | --- |
| `toss-webhooks` | `github-webhook` | `BULLMQ_CONCURRENCY` (default 4) | Processes one signed GitHub delivery |
| `toss-bounty` | `advance-issue` | 1 | Evaluates the rules, queues the next step, or parks itself |
| `toss-bounty` | `push-milestone` | 1 | Re-checks, then pushes the milestone on-chain |
| `toss-bounty` | `release-payout` | 1 | Re-checks, then releases the bounty |
| `toss-sync` | `escrow-balance-sync` | 1 | Reconciles every deployed escrow's balance |

The bounty queue runs at concurrency **1** on purpose: it is the queue that moves
money, and serialising it keeps the on-chain call sequence predictable.

Queue keys are namespaced by `BULLMQ_PREFIX` (default `bull`, the BullMQ
default), so any BullMQ dashboard can read them.

---

## Job ids and deduplication

Every job carries a stable id:

- `github-webhook:<X-GitHub-Delivery>`
- `advance-issue:<issue-uuid>`, `push-milestone:<issue-uuid>`, `release-payout:<issue-uuid>`

Because the id is per issue, **a burst of events in one second cannot enqueue two
payouts** — BullMQ ignores an `add` for an id that already exists.

Enqueueing is *ensure* semantics, not a blind `add`:

| Existing job state | What happens |
| --- | --- |
| none | Add the job |
| `completed` / `failed` | Remove it, then add — a permanently failed job must never wedge an issue shut |
| `delayed` | **Promote** it, so a real event runs the parked re-check now instead of waiting out its delay |
| `waiting` / `prioritized` | Nothing; a pass is already coming |
| `active` | Set a dirty flag; the running worker re-evaluates before finishing |

That last row is what stops an event landing mid-job from being lost. The
`advance-issue` worker clears the flag *before* reading state and re-checks it
after acting, so anything that arrives during the pass earns another one (up to
3 passes per job).

Completed webhook jobs are kept for 24 hours, so a GitHub redelivery of the same
delivery id is recognised as a duplicate rather than reprocessed. A delivery that
*failed* is the exception: redelivering it from GitHub is an explicit "try this
again", so the failed job is dropped and the delivery is accepted.

Completed bounty jobs are removed immediately so the per-issue id frees up for
the next event — which means `escrow-operations.completed` in the stats endpoint
reads 0 by design.

---

## Retry policy

| Queue | Attempts | Backoff |
| --- | --- | --- |
| `toss-webhooks` | 5 | exponential, 2s base |
| `toss-bounty` | 5 | exponential, 5s base |
| `toss-sync` | 3 | exponential, 5s base |

Errors are classified in [`src/infra/jobs/mod.rs`](../src/infra/jobs/mod.rs):

- **Retryable** (`ProcessingError`) — Trustless Work timeouts and 5xx, GitHub
  rate limits and 5xx, database blips. BullMQ retries these with backoff, on its
  own.
- **Permanent** (`Unrecoverable`) — malformed webhook payloads, missing records,
  rejected requests. These fail once and land in `failed` with their reason
  intact, rather than burning five attempts on a state that cannot change.

Failed jobs are kept for 7 days: `failed` *is* the dead-letter queue.

BullMQ also recovers stalled jobs, so a worker killed mid-job does not strand it.

---

## The issue state machine

`advance-issue` evaluates; `push-milestone` and `release-payout` act. All three
re-read Postgres, GitHub and the escrow contract before doing anything, so a job
that has been queued for minutes — or retrying with backoff — never acts on
stale state.

```
labeled + assigned          → wait for wallet
wallet connected            → push milestone on-chain
PR merged / issue closed
  + milestone on-chain
  + receiver matches wallet
  + not already released    → release payout
```

Decisions (`Decision` in `automation.rs`):

| Decision | Meaning | Next |
| --- | --- | --- |
| `WaitForWallet` | No payout address yet | Park; delayed re-check, and any real event promotes it |
| `PushMilestone` | Wallet known, milestone not on-chain | Queue `push-milestone` |
| `ReleasePayout` | Every live rule passed | Queue `release-payout` |
| `RepairDatabase` | Chain says released, Postgres says pending | **Update Postgres only. Never pay again.** |
| `Settled` | Nothing left to do | Stop |
| `Waiting` | Issue still open, nobody assigned yet | Park; delayed re-check |
| `Blocked` | Amount mismatch, no escrow deployed, DB/chain disagreement | Stop and log — deliberately not retried |

### Money safety

- The **chain is read first**. If the milestone is already released, the only
  permitted action is repairing Postgres.
- If the on-chain **receiver** does not match the contributor's current payout
  address, the milestone is stale — the contributor changed wallets since it was
  pushed. The payout does **not** proceed; the milestone is re-pushed to the
  correct receiver first. That moves no funds, and it is what keeps a wallet
  change from wedging a bounty shut.
- If the on-chain **amount** does not match the issue reward, the payout is
  `Blocked`. Rewards only change while an issue is pending, so a mismatch here
  means something moved that should not have — neither figure gets paid.
- If Postgres says "released" but the chain does not, that is `Blocked` too —
  paying again to "fix" the books is exactly the failure we refuse to have.
- A blind scheduled pass never auto-releases, refunds or disputes. Money moves
  only when the live rules pass.

### Loop safety

Every queue-to-queue handoff carries a `hops` counter (`advance-issue` →
`push-milestone` → `advance-issue` → `release-payout`). If a condition the chain
never satisfies would otherwise ping-pong jobs forever, the budget runs out and
the chain stops with a log line instead.

### Parked flows

`WaitForWallet` and `Waiting` park the issue with exponential spacing (60s,
doubling to a 30-minute cap, 12 re-checks).

The job parks *itself* — it calls `move_to_delayed` rather than enqueueing a
second job, because a running job already owns `advance-issue:<issue-id>` and
re-adding that id would be swallowed as a duplicate. BullMQ treats this as
control flow, not a failure: no retry attempt is consumed, and the job keeps its
id.

Keeping the id is the point. The timer is only the backstop — a wallet connect or
a merged PR **promotes** the parked job so it resumes immediately.

---

## Producers

| File | Enqueues |
| --- | --- |
| `modules/github/routes.rs` | `github-webhook` (job id = delivery id) |
| `modules/github/handlers/issue_assigned.rs` | `advance-issue` |
| `modules/github/handlers/pull_request.rs` | `advance-issue`, after recording the merge |
| `modules/github/handlers/issue_closed.rs` | `advance-issue` |
| `modules/github/handlers/issue_comment.rs` | `advance-issue` (the `/retry` command) |
| `modules/contributor/routes.rs` | `advance-issue` for every parked bounty, after wallet connect |
| `modules/bounty/service.rs` | `advance-issue` from `/api/milestones/push` and `/api/issues/{id}/retry` |
| `infra/jobs/advance.rs` | `push-milestone`, `release-payout`, and a follow-up `advance-issue` after a push |
| `infra/queue.rs` scheduler | `escrow-balance-sync`, via a BullMQ `JobScheduler` |

---

## Scheduled work

`escrow-balance-sync` is registered with a BullMQ job scheduler, which holds
**exactly one pending job at a time**. The old code pushed a new `queue:sync`
list entry every 60 seconds — entries nothing ever consumed, so they accumulated
forever. Re-registering on every boot is idempotent.

---

## Retry is not part of the happy path

`POST /api/issues/{issueId}/retry` and `@Trustless-OSS /retry` still exist as
maintainer escape hatches, but nothing depends on them. Both now do only one
thing: ask the state machine to run *now*, against the same live rules. They
cannot force a payout the automation would refuse.

The cases that used to require a manual retry are handled automatically:

| Situation | Before | Now |
| --- | --- | --- |
| Wallet connected after the PR merged | Comment told the maintainer to run `/retry` | Wallet connect promotes the parked re-check; payout continues |
| Trustless Work timeout or 5xx | Payout marked `failed`, manual retry | BullMQ retries with backoff until it succeeds |
| Issue closed before the milestone was pushed | Stalled | `advance-issue` pushes, then re-evaluates and releases |
| Duplicate webhook deliveries | Custom SETEX dedup key | Delivery id is the job id |

---

## Observability

`GET /api/queue/stats` returns live BullMQ counts:

```json
{
  "webhooks":          { "waiting": 0, "active": 1, "completed": 42, "failed": 0, "delayed": 0 },
  "escrow-operations": { "waiting": 0, "active": 0, "completed": 0,  "failed": 1, "delayed": 2 },
  "sync":              { "waiting": 0, "active": 0, "completed": 17, "failed": 0, "delayed": 1 }
}
```

`webhooks` is `toss-webhooks`, `escrow-operations` is `toss-bounty`, `sync` is
`toss-sync`. As noted above, `escrow-operations.completed` stays at 0 because
completed bounty jobs are removed to free their per-issue id.

---

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `REDIS_URL` | — | Shared by the cache and every queue |
| `BULLMQ_PREFIX` | `bull` | Redis key prefix for all queues |
| `BULLMQ_CONCURRENCY` | `4` | Webhook worker concurrency (bounty is always 1) |
| `ESCROW_SYNC_INTERVAL_SECS` | `60` | Interval of the repeating sync job |

If Redis is unreachable at boot the API still starts: the hub is disabled,
workers do not start, webhooks are processed inline and `/api/queue/stats`
reports zeros — the same degraded behaviour as before.

---

## Lifecycle

Workers start **after** database migrations, so a job never runs against a stale
schema. On `SIGTERM`/`SIGINT` the HTTP server drains first, then workers are
closed with a 10-second grace period for in-flight jobs. Anything still queued is
picked up on the next boot.

---

## Testing

`tests/queue_bullmq.rs` covers the behaviour above against a real Redis:

- a redelivered `X-GitHub-Delivery` is recognised as a duplicate;
- a delivery that failed permanently can still be revived by a redelivery, and an
  unrecoverable error reaches `failed` without burning its retry budget;
- a burst of five events for one issue collapses to a single job;
- separate issues do not share a job;
- a real event promotes a parked re-check instead of waiting out its delay;
- a job that parks *itself* via `move_to_delayed` keeps its id, consumes no
  retry attempt, is not counted as failed, and resumes when promoted;
- the escrow-sync scheduler holds exactly one pending job across repeated
  registrations;
- a transient failure retries with backoff and then succeeds, with no operator
  involvement.

The tests skip themselves when no Redis is reachable.

```bash
docker compose up -d redis
cargo test --test queue_bullmq
```
