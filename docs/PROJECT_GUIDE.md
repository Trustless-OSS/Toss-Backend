# Trustless OSS (TOSS) Backend — Complete Project Guide

> Code-derived documentation for this repository. Last reviewed: 2 November 2026.
>
> This guide explains what the project does, why it exists, how requests and events move through it, how money is protected, how to run it, and where each concern lives in the code. For exact request/response examples, also see [APIs.md](APIs.md). For queue internals, see [jobs.md](jobs.md).

## Contents

1. [Executive summary](#1-executive-summary)
2. [The problem TOSS solves](#2-the-problem-toss-solves)
3. [Actors and terminology](#3-actors-and-terminology)
4. [System architecture](#4-system-architecture)
5. [Technology stack](#5-technology-stack)
6. [Application startup and shutdown](#6-application-startup-and-shutdown)
7. [Authentication and authorization](#7-authentication-and-authorization)
8. [Repository onboarding](#8-repository-onboarding)
9. [Escrow lifecycle](#9-escrow-lifecycle)
10. [Bounty lifecycle from start to finish](#10-bounty-lifecycle-from-start-to-finish)
11. [GitHub webhook and comment behavior](#11-github-webhook-and-comment-behavior)
12. [Background jobs and reliability](#12-background-jobs-and-reliability)
13. [Notifications and email](#13-notifications-and-email)
14. [Dashboard and leaderboard](#14-dashboard-and-leaderboard)
15. [Uploads and support tickets](#15-uploads-and-support-tickets)
16. [Database model](#16-database-model)
17. [Redis usage](#17-redis-usage)
18. [Complete HTTP endpoint map](#18-complete-http-endpoint-map)
19. [Configuration reference](#19-configuration-reference)
20. [Local development](#20-local-development)
21. [Testing and verification](#21-testing-and-verification)
22. [Observability and operations](#22-observability-and-operations)
23. [Security model](#23-security-model)
24. [Source tree guide](#24-source-tree-guide)
25. [Failure modes and troubleshooting](#25-failure-modes-and-troubleshooting)
26. [Current implementation notes](#26-current-implementation-notes)

## 1. Executive summary

TOSS is the backend orchestration service for **Trustless OSS**, a platform that attaches USDC bounties to GitHub issues and releases the funds through a Stellar escrow after the work is verified.

It coordinates six systems:

- GitHub App installations, repositories, issues, labels, assignments, comments, and pull requests.
- Supabase Auth for browser/user bearer-token verification.
- PostgreSQL for durable application records.
- Redis for caching, distributed rate limits, job queues, deduplication, and event coalescing.
- Trustless Work and Stellar for escrow deployment, funding, milestones, disputes, refunds, and payouts.
- Resend and S3-compatible storage for email notifications and direct file uploads.

The central design rule is that **an HTTP request or GitHub webhook does not blindly move money**. GitHub events enqueue work. A background state machine reloads current database, GitHub, wallet, and on-chain state before it decides whether to create a milestone, wait, repair local state, block, or release a payout.

## 2. The problem TOSS solves

A normal GitHub issue tracker knows who opened, assigned, closed, or merged work, but it does not reserve funds or enforce payout rules. A blockchain escrow can hold and transfer funds, but it does not understand repository permissions, labels, assignees, or pull requests.

TOSS joins these worlds:

1. A maintainer connects a public, non-fork repository.
2. The maintainer deploys and funds a repository-specific escrow.
3. Reward labels map GitHub labels to USDC amounts.
4. Applying the required bounty labels reserves that amount from the repository's cached available pool.
5. Assigning a contributor creates or updates the contributor profile.
6. A payout wallet allows TOSS to push an escrow milestone.
7. A merged linked pull request and a completed/closed issue trigger re-evaluation.
8. The worker verifies the live conditions again and releases the milestone only when they are safe.

This provides automation without treating an old webhook, stale database row, or repeated request as sufficient authority to pay.

## 3. Actors and terminology

| Term | Meaning in this project |
| --- | --- |
| Maintainer | A profile connected to a repository through `repo_maintainers`; allowed to configure rewards and operate its escrow. |
| Contributor | A GitHub user assigned to a bounty. The same profile may also be a maintainer. |
| Repository | A connected GitHub repository plus its installation ID, escrow contract, cached balance, rewards, and maintainers. |
| Reward level | A repository-local GitHub label to decimal USDC amount mapping. |
| Bounty | The durable record corresponding to a GitHub issue and its payout lifecycle. Some older function names call it an “issue.” |
| Escrow | A Trustless Work contract on Stellar associated with one repository. |
| Milestone | The on-chain payout instruction for one bounty, including amount and receiver. |
| Assignment | In the current model, assignment fields are stored on the bounty record itself. Some code retains assignment-oriented naming. |
| Internal UUID | TOSS-generated identifier used in most API paths. It is not a GitHub issue number or GitHub numeric ID. |

## 4. System architecture

```mermaid
flowchart LR
    Browser[Frontend / API client] -->|Supabase bearer token| Axum[Axum HTTP API]
    GitHub[GitHub App] -->|HMAC-signed webhook| Axum
    Axum <--> Postgres[(PostgreSQL)]
    Axum <--> Redis[(Redis)]
    Axum --> S3[S3-compatible storage]
    Axum --> TW[Trustless Work API]
    Axum --> Stellar[Stellar network]
    Redis --> Workers[BullMQ workers]
    Workers <--> Postgres
    Workers <--> GitHub
    Workers <--> TW
    Workers --> Resend[Resend email]
```

### Main runtime layers

| Layer | Responsibility | Important files |
| --- | --- | --- |
| Entry point | Load configuration, build state, start workers/server, graceful shutdown | `src/main.rs`, `src/lifecycle.rs` |
| HTTP assembly | Root route, feature routers, CORS, tracing, IP limiter | `src/app.rs`, `src/routes/mod.rs` |
| Middleware | Supabase token verification and rate limiting | `src/middleware/` |
| Feature modules | GitHub, repository, bounty, escrow, contributor, notification, dashboard, tickets, upload | `src/modules/` |
| Infrastructure | Toasty/PostgreSQL, Redis, BullMQ, S3, Stellar, Resend, migrations | `src/infra/` |
| Data model | API/domain projections and Toasty schema models | `src/shared/models/` |
| API description | Utoipa OpenAPI registration and Swagger UI | `src/docs/openapi.rs` |

### Request flow

1. Axum receives the request.
2. Global middleware applies tracing, CORS, and an IP-based rate limit.
3. Protected handlers extract `AuthedUser`; this verifies the bearer token with Supabase and extracts GitHub identity.
4. The handler validates input and authorization, invokes a service/repository, and returns JSON.
5. Slow or retryable automation is put into BullMQ instead of being completed inside the webhook request.

## 5. Technology stack

| Area | Technology |
| --- | --- |
| Language/runtime | Rust 2021, stable toolchain, Tokio |
| Web framework | Axum 0.8 |
| API schema | Utoipa/OpenAPI and Swagger UI |
| Database | PostgreSQL 16, Toasty ORM/migrations |
| Queue/cache | Redis 7, `bullmq-official` |
| HTTP clients | Reqwest with Rustls |
| Auth | Supabase Auth user endpoint with GitHub identity metadata |
| Chain | Stellar XDR/base libraries and Trustless Work HTTP API |
| Email | Resend with HTML templates |
| Object storage | AWS S3 SDK, compatible with Filebase/MinIO/R2-style endpoints |
| Logging | `tracing`, `tracing-subscriber`, Tower HTTP tracing |
| Rate limiting | Tower Governor plus Redis fixed windows for sensitive user actions |

## 6. Application startup and shutdown

### Startup sequence

`src/main.rs` performs the following in order:

1. `Config::from_env()` loads the repository `.env` file and validates configuration.
2. Telemetry installs the tracing subscriber.
3. `AppState::new()` connects to PostgreSQL, applies embedded pending migrations, creates the Redis/cache clients, attempts to create BullMQ queues, initializes Resend, and builds the S3 client.
4. BullMQ workers start when queue infrastructure is available.
5. The optional development Smee webhook relay starts.
6. Axum binds to `0.0.0.0:$PORT`.
7. Swagger UI is mounted at `/swagger` and OpenAPI JSON at `/api/v1/doc/openapi.json`.

PostgreSQL is mandatory: startup fails if it cannot connect or migrate. BullMQ initialization is intentionally softer: a connection error disables queues and logs the failure. Webhook jobs may fall back to inline processing when the queue is unavailable, but bounty queue calls generally report an unavailable outcome and cannot provide normal asynchronous automation.

### Shutdown sequence

SIGINT or SIGTERM marks the process as shutting down, causes health endpoints to return `503`, stops Axum gracefully, asks workers to close with a 10-second timeout, and closes queue handles. A second Ctrl+C forces process exit.

## 7. Authentication and authorization

### Bearer-token authentication

Protected handlers require:

```http
Authorization: Bearer <supabase-access-token>
```

The `AuthedUser` extractor calls `${SUPABASE_URL}/auth/v1/user` with the bearer token and configured Supabase API key. It accepts GitHub identity data from several Supabase metadata/identity shapes and produces:

- numeric `github_id` (required),
- GitHub username (optional),
- email address (optional).

Invalid, rejected, or unparseable credentials return `401 {"error":"Unauthorized"}`.

`AUTH_BYPASS` exists for local development only. It is rejected unless `NODE_ENV=development`; when enabled it substitutes a fixed test GitHub identity. It must never be enabled in a deployed environment.

### Maintainer authorization

Authentication proves identity, not repository control. Maintainer-only operations also look up the profile/repository relationship. These include reward changes, repository deletion, escrow operations, and manual bounty controls.

### Webhook authentication

`POST /api/v1/webhooks/github` does not use a bearer token. It verifies `X-Hub-Signature-256` as HMAC-SHA256 over the exact raw body using `GITHUB_WEBHOOK_SECRET`. Modified, missing, or incorrectly signed payloads are rejected.

### Public routes

Root/health endpoints, queue statistics, repository issue listing, and `/api/leaderboard` are public in the current router. Do not assume that operational queue counts are private.

## 8. Repository onboarding

There are three ways repository information reaches the system:

1. **GitHub installation webhook** — installation creation imports included repositories and links the installer/owner.
2. **Installation sync API** — the frontend lists repositories visible to an installation and asks TOSS to import one or more selected IDs.
3. **Direct connect API** — accepts repository/owner information and a GitHub token, stores the repository, and attempts webhook setup.

Private repositories and forks are filtered out by installation flows. Installation removal deletes associated TOSS repository records; removing a repository from an installation deletes that repository record. Repository rename information is synchronized opportunistically from repo-scoped webhook payloads.

Once connected, a maintainer can:

- inspect details and capability flags,
- view stored bounty issues,
- list GitHub labels,
- create/update/delete reward mappings,
- deploy/fund/refund/close escrow,
- disconnect the repository.

Default GitHub labels created on a best-effort basis after escrow deployment are `rewarded`, `low`, `medium`, and `high`.

## 9. Escrow lifecycle

### Why signing is split into two calls

Deploy, fund, and close operations use an unsigned/signed protocol:

1. The backend validates the authenticated maintainer and repository state.
2. It asks Trustless Work/building logic for an unsigned Stellar XDR transaction.
3. The frontend wallet signs the XDR; the private user key stays with the user.
4. The frontend submits signed XDR to a second backend endpoint.
5. The backend submits it and updates local state.

### Lifecycle

```mermaid
stateDiagram-v2
    [*] --> NoEscrow
    NoEscrow --> Deployed: create unsigned + submit deploy
    Deployed --> Funded: fund unsigned + submit fund
    Funded --> Active: rewards reserve balance / milestones created
    Active --> Funded: payout or bounty cancellation
    Funded --> Refunded: refund
    Deployed --> Closed: close unsigned + submit close
    Funded --> Closed: close unsigned + submit close
```

The repository stores the contract ID and a cached decimal balance. Balance versioning is used for concurrency-safe reservations/refunds. Funding records may be stored in `escrow_funders`. Refund cancels unpaid bounties and reports the refunded amount/cancel count.

New escrows use the platform address and a 100-basis-point (1%) TOSS platform fee. The configured token address selects the escrow asset; the code and UI language primarily describe USDC.

Sensitive escrow endpoints additionally use a Redis per-user rate limit of five operations per minute. Stellar payout accounts are validated where applicable.

## 10. Bounty lifecycle from start to finish

### Normal happy path

```mermaid
flowchart TD
    A[Fund repository escrow] --> B[Configure reward labels]
    B --> C[Apply rewarded + amount label]
    C --> D[Create bounty and reserve cached balance]
    D --> E[Assign contributor]
    E --> F{Contributor wallet exists?}
    F -- No --> G[Park and notify]
    G --> F
    F -- Yes --> H[Push/refresh on-chain milestone]
    H --> I[Contributor opens linked PR]
    I --> J[PR merged and issue closed completed]
    J --> K[Reload DB, GitHub and chain state]
    K --> L{Every invariant valid?}
    L -- No, temporary --> M[Wait and recheck]
    L -- No, unsafe --> N[Block without moving funds]
    L -- Yes --> O[Release payout]
    O --> P[Mark paid and notify/comment]
```

### Bounty creation and pricing

An issue becomes a priced bounty when it carries `rewarded` plus a configured reward label. A `manual` flow can obtain the amount from `@trustless-oss <amount>` in an issue body or privileged comment. An unpriced bounty may exist with zero reward while waiting for a valid amount.

Creation reserves the reward from the repository's cached available balance. If funds are insufficient, the operation does not create an unsafe reservation and maintainers are notified/commented. Before a milestone is locked, maintainers may change a pending reward. Once chain state exists, amount changes are restricted to avoid database/escrow divergence.

### Assignment and wallet

An `issues.assigned` event ensures a contributor profile and records the assignee. If the issue was assigned before it became a bounty, later bounty creation adopts the existing assignee.

`POST /api/v1/wallet/connect` upserts a payout wallet, validates Stellar/USDC compatibility for Stellar, saves the authenticated user's email when available, and enqueues all of that contributor's non-paid/non-cancelled bounties. This means a parked bounty resumes immediately after wallet connection.

### Milestone creation

The automation pushes a milestone only when there is an assignee, a positive reward, a deployed escrow, and a usable payout wallet. It records the returned milestone index and posts a GitHub comment. If the wallet changed before payout and the original reward is still safely replaceable, the state machine can refresh the receiver.

### Completion and payout

A merged PR must reference the issue using a closing phrase such as `Closes #123`, and its author must match the assigned contributor. The issue must also be closed as completed. These events enqueue evaluation; they do not directly release funds.

Immediately before release, automation checks current escrow milestone state, receiver, amount, local paid state, recorded merge state, and live GitHub issue state. A fully released chain milestone with stale PostgreSQL is repaired locally instead of paid again.

The normal payout releases the complete milestone. A privileged comment can request a fixed or percentage contributor payment; this uses a disputed milestone resolution to split the original reward between contributor and maintainer. Split payout needs both wallets and preserves the original total.

### Cancellation, rejection, unassignment, and hold

- Removing/unassigning a contributor clears the assignment, zeros an existing milestone where applicable, and returns the bounty to a pending/open condition.
- Deleting an unpaid GitHub issue cancels the bounty and restores reserved balance.
- The `rejected` label, privileged reject command, or authenticated reject endpoint cancels an unpaid bounty and refunds the pool.
- The hold endpoint places an on-chain dispute hold on a milestone.
- Paid/cancelled bounties are treated as settled no-ops.

## 11. GitHub webhook and comment behavior

### Processed webhook events

| Event | Actions used | Effect |
| --- | --- | --- |
| `installation` | `created`, `deleted` | Import eligible repositories or clean up installation repositories. |
| `installation_repositories` | `added`, `removed` | Add/remove selected repositories. |
| `issues` | `opened`, `labeled`, `assigned`, `unassigned`, `closed`, `deleted` | Create/update/cancel bounty state and enqueue automation. |
| `issue_comment` | `created` | Handle bot commands and manual amounts. |
| `pull_request` | merged `closed` | Validate linked issue/author, record merge, enqueue automation. |
| `label` | changes | Logged; issue label application is what triggers bounty processing. |

Installation events are processed inline after signature verification. Other events are queued by GitHub delivery ID. If the webhook queue is unavailable, the handler processes them inline.

### Recognized comment commands

The bot recognizes case-insensitive commands addressed to `@trustless-oss`:

| Form | Who can use it | Purpose |
| --- | --- | --- |
| `@trustless-oss <amount>` | Owner/member/collaborator | Set/create a manual bounty amount. |
| `@trustless-oss pay <amount>` or `<percent>%` (also split/work/completion aliases) | Owner/member/collaborator | Request full/partial work-completion payout distribution. |
| `@trustless-oss reject` (and aliases) | Owner/member/collaborator | Reject/cancel eligible work. |
| `@trustless-oss retry` | Owner/member/collaborator | Re-enqueue safe evaluation. |
| `@trustless-oss wallet` (and aliases) | Any non-bot commenter | Return wallet/profile guidance. |
| `@trustless-oss help` | Any non-bot commenter | Return command help. |

GitHub `author_association` is used for comment privileges. Bot-authored comments are ignored to prevent loops.

## 12. Background jobs and reliability

### Queues and jobs

| Queue | Job names | Purpose |
| --- | --- | --- |
| `toss-webhooks` | `github-webhook` | Parse and apply non-installation GitHub events. |
| `toss-bounty` | `advance-issue`, `push-milestone`, `release-payout` | Run the bounty state machine and money-related steps. |
| `toss-notify` | `send-notification-email` | Deliver notification email asynchronously. |

Webhook worker concurrency comes from `BULLMQ_CONCURRENCY` (default 4). The bounty worker runs at concurrency 1 so on-chain operations remain ordered. Notification delivery has its own worker.

### Retry policy

- Webhook jobs: 5 attempts, exponential backoff starting at 2 seconds.
- Bounty jobs: 5 attempts, exponential backoff starting at 5 seconds.
- Notification email: 5 attempts, exponential backoff starting at 5 seconds.
- Validation/auth/not-found/webhook-shape errors are classified unrecoverable.
- integration/database/internal failures are classified retryable.
- Failed jobs may remain for up to seven days (bounded to 1,000); completed webhook jobs are retained up to one day, while completed bounty/email jobs are removed.

### Deduplication and coalescing

- GitHub delivery IDs produce stable job IDs, so redelivery is recognized.
- Bounty jobs use the job type plus internal bounty UUID as their ID.
- A short Redis enqueue lock reduces races across application instances.
- If an advance job is already active, a dirty flag records that a new event arrived; the worker re-evaluates up to three passes and drains later events.
- A real event promotes a delayed recheck rather than waiting for its original timer.
- A hop budget prevents push/advance/release handoff loops.

### Parking

Temporary states—such as a missing wallet, unmerged PR, or open issue—delay the same advance job. Delays start at 60 seconds, double, cap at 30 minutes, and stop after 12 scheduled rechecks. After that, the next real GitHub/wallet event can resume the flow. Unsafe contradictions become `Blocked` and do not continually retry.

### Idempotency principle

Every money job calls the evaluator again immediately before acting. If conditions changed, it stands down. Already released chain state causes database repair, not a second release. This is the main protection against duplicate webhooks, retry races, and stale queued work.

## 13. Notifications and email

Notifications are stored with a unique `dedupe_key`, recipient, kind, text, optional reference/data, read state, and email state. The service avoids actor self-notifications and applies a role-and-kind delivery policy:

- some events are bell-only,
- important contributor/maintainer events are bell plus email,
- irrelevant role/event pairs produce no notification.

Kinds include bounty assigned/unassigned/locked/cancelled/rejected, amount updated/missing, wallet required, payout released/blocked, insufficient funds, and escrow funded/refunded.

Email jobs use Resend and HTML templates under `src/infra/email/templates/`. If the queue cannot accept the job, delivery is spawned directly. Missing email addresses are optionally resolved through GitHub and cached; otherwise the notification is marked `skipped_no_email`. Successful, skipped, and failed states are persisted.

## 14. Dashboard and leaderboard

Maintainer dashboard routes scope data to repositories linked to the authenticated GitHub profile. They derive:

- activity rows (`rewarded`, `locked`, `released`, `assigned`, `unassigned`, `rejected`),
- funds locked and released,
- issue and contributor counts,
- monthly cumulative TVL versus payouts,
- a contributor leaderboard.

The public `/api/leaderboard` aggregates across the platform, exposes aggregate contribution data, paginates with `page`/`page_size`, and caches each page in Redis for 180 seconds.

## 15. Uploads and support tickets

### Direct uploads

`POST /api/v1/upload` returns a presigned S3 `PUT` URL lasting 300 seconds. The object key is namespaced as `<folder>/<github-id>/<uuid>`. The API validates:

- size is 1 byte through 5 GiB,
- content type is non-empty,
- folder is non-empty and contains only ASCII letters/numbers, `-`, or `_`.

The client must upload the exact declared content length/type and returned headers directly to object storage. The configured bucket needs appropriate CORS for the frontend.

### Support tickets

Authenticated users can create, read, patch, and resolve their own tickets. Tickets support organization, category, subject, message, optional uploaded attachment key, and optional bounty UUID. Ownership is enforced using the caller's profile ID. Input is trimmed and length-limited (category 100, subject 200, message 10,000, organization 200, attachment key 1,024 characters).

## 16. Database model

PostgreSQL is the source of durable application state. Toasty schema models live under `src/shared/models/schema/`; generated SQL is under `toasty/`.

| Table | Purpose | Key constraints/fields |
| --- | --- | --- |
| `profiles` | GitHub-backed user identity and editable contributor details | Unique `github_id`, unique `username`, contact/profile fields. |
| `wallets` | Chain payout addresses | Unique `(profile_id, chain, address)`, primary flag. |
| `repositories` | GitHub repo plus escrow state | Unique GitHub repo ID/full name, installation ID, contract, cached balance, version. |
| `repo_maintainers` | Profile-to-repository authorization | Composite key `(repo_id, profile_id)`, role. |
| `reward_levels` | Label-to-amount configuration | Unique `(repo_id, label)`. |
| `bounties` | GitHub issue and payout lifecycle | Unique GitHub issue ID and `(repo_id, issue_number)`; reward, assignee, milestone, timestamps, payout hash. |
| `escrow_funders` | Funding deposit history | Wallet, amount, unique transaction hash, optional profile. |
| `notifications` | In-app/email delivery record | Unique dedupe key, email/read status. |
| `activity` | Generic append-style audit/event records | Repo/actor, type, JSON payload, timestamp. |
| `failed_tasks` | Persistable failed-task metadata | Type, reference, JSON payload, attempts/status/error. |
| `tickets` | User-owned support requests | Profile, category/message, attachment/bounty references, status. |

### Important bounty fields

`bounties` contains the internal UUID, repository/reward links, GitHub issue IDs, title, reserved/current reward amount, status, assignee, milestone index, assigned/merged/paid timestamps, and payout transaction hash.

Logical statuses include `open`, `assigned`, `merged`, `paid`, and `cancelled`; current automation often advances from assigned directly to paid after verifying merge/closure. The on-chain milestone remains the financial truth for whether funds were released.

### Migration behavior

The server embeds and applies pending migrations at every startup. The `migrate` binary can generate/apply migrations using `DATABASE_URL` or `TOASTY_CONNECTION_URL`. Model changes should be followed by migration generation and committed `toasty/` artifacts.

## 17. Redis usage

Redis serves multiple independent roles:

- BullMQ queue storage and locks.
- General JSON cache (for example, public leaderboard pages).
- Fixed-window per-user limits for wallet and escrow actions.
- Enqueue locks to reduce multi-instance duplicate jobs.
- Per-bounty dirty flags used to coalesce events arriving during an active job.

`BULLMQ_PREFIX` namespaces queue and related keys so multiple environments can share a Redis server safely. A Redis outage degrades queue automation, caching, and per-user limit checks. Some limits fail open by design; the global in-process IP limit still applies per instance.

## 18. Complete HTTP endpoint map

`Bearer` means Supabase bearer authentication. `Maintainer` additionally requires repository authorization. Full schemas and examples are in [APIs.md](APIs.md) and runtime Swagger.

### System

| Method | Path | Access | Purpose |
| --- | --- | --- | --- |
| GET | `/` | Public | Basic service response. |
| GET | `/health` | Public | Combined dependency/environment health. |
| GET | `/api/v1/health` | Public | Versioned combined health. |
| GET | `/api/v1/health/database` | Public | PostgreSQL check and latency. |
| GET | `/api/v1/health/redis` | Public | Redis check and latency. |
| GET | `/api/v1/health/trustless-work` | Public | Trustless Work check and latency. |
| GET | `/api/v1/queue/stats` | Public | Live queue counts. |
| GET | `/swagger` | Public | Interactive API UI. |
| GET | `/api/v1/doc/openapi.json` | Public | OpenAPI document. |

### Repositories and rewards

| Method | Path | Access | Purpose |
| --- | --- | --- | --- |
| GET | `/api/v1/repos` | Bearer | Paginated repositories visible to caller. |
| POST | `/api/v1/repos/connect` | Bearer | Directly connect a repository. |
| GET | `/api/v1/repos/installation-repos` | Bearer | List eligible installation repositories. |
| POST | `/api/v1/repos/sync-installation` | Bearer | Import selected installation repositories. |
| GET | `/api/v1/repos/{repoId}` | Bearer | Details, escrow status, capability flags. |
| DELETE | `/api/v1/repos/{repoId}` | Maintainer | Disconnect repository. |
| GET | `/api/v1/repos/{repoId}/issues` | Public | Paginated stored bounties/issues. |
| PUT | `/api/v1/repos/{repoId}/rewards` | Maintainer | Upsert label/amount reward. |
| GET | `/api/v1/repos/rewards/{repoId}` | Bearer | List reward configuration. |
| GET | `/api/v1/repos/{repoId}/github-labels` | Bearer | Fetch live GitHub labels. |
| DELETE | `/api/v1/repos/{repoId}/rewards/{label}` | Maintainer | Delete reward and optionally GitHub label. |

### Contributor and bounty

| Method | Path | Access | Purpose |
| --- | --- | --- | --- |
| POST | `/api/v1/wallet/connect` | Bearer | Save wallet and resume parked bounties. |
| GET | `/api/v1/contributor/me` | Bearer | Profile, wallets, and assigned bounties. |
| PUT | `/api/v1/contributor/profile` | Bearer | Update profile fields. |
| POST | `/api/v1/milestones/push` | Assigned contributor | Save payout intent/wallet and queue automation. |
| POST | `/api/v1/issues/{issueId}/retry` | Maintainer | Safely re-evaluate internal bounty UUID. |
| POST | `/api/v1/issues/{issueId}/reject` | Maintainer | Reject unpaid bounty and restore pool. |
| POST | `/api/v1/issues/{issueId}/hold` | Maintainer | Put milestone in dispute/hold. |

### Escrow

| Method | Path | Access | Purpose |
| --- | --- | --- | --- |
| POST | `/api/v1/escrow/create-unsigned` | Maintainer | Build deployment XDR. |
| POST | `/api/v1/escrow/submit-deploy` | Maintainer | Submit signed deployment and record contract. |
| POST | `/api/v1/escrow/fund-unsigned` | Maintainer | Build funding XDR. |
| POST | `/api/v1/escrow/submit-fund` | Maintainer | Submit signed funding and update balance. |
| POST | `/api/v1/escrow/refund` | Maintainer | Refund escrow and cancel eligible bounties. |
| POST | `/api/v1/escrow/close-unsigned` | Maintainer | Build close XDR. |
| POST | `/api/v1/escrow/submit-close` | Maintainer | Submit signed close. |

### Notifications, dashboard, tickets, uploads, webhook

| Method | Path | Access | Purpose |
| --- | --- | --- | --- |
| GET | `/api/v1/notifications` | Bearer | Paginated notification list. |
| GET | `/api/v1/notifications/unread-count` | Bearer | Unread total. |
| POST | `/api/v1/notifications/read-all` | Bearer | Mark all read. |
| POST | `/api/v1/notifications/{id}/read` | Bearer | Mark owned notification read. |
| GET | `/api/v1/dashboard/activity` | Bearer | Maintainer-scoped activity. |
| GET | `/api/v1/dashboard/summary` | Bearer | Maintainer summary metrics. |
| GET | `/api/v1/dashboard/funds-movement` | Bearer | Monthly TVL/payout series. |
| GET | `/api/v1/dashboard/leaderboard` | Bearer | Maintainer-scoped leaderboard. |
| GET | `/api/leaderboard` | Public | Cached global leaderboard. |
| POST | `/api/v1/tickets` | Bearer | Create ticket. |
| GET | `/api/v1/tickets/{ticketId}` | Bearer/owner | Read ticket. |
| PATCH | `/api/v1/tickets/{ticketId}` | Bearer/owner | Edit ticket. |
| POST | `/api/v1/tickets/{ticketId}/resolve` | Bearer/owner | Resolve ticket. |
| POST | `/api/v1/upload` | Bearer | Get short-lived direct upload URL. |
| POST | `/api/v1/webhooks/github` | GitHub HMAC | Receive GitHub event. |

### Common response conventions

- Application errors: `{"error":"human-readable message"}`.
- Normal statuses: `400` invalid input/state, `401` authentication, `403` authorization, `404` missing record, `429` limit, `500` integration/server error.
- Health endpoints return `503` for unavailable required dependencies or shutdown.
- UUID path parameters refer to TOSS records unless explicitly described as GitHub IDs.
- Monetary values use decimal types and are often serialized as strings to preserve precision.
- Repository lists use `limit`/`offset`; leaderboards use `page`/`page_size`.

## 19. Configuration reference

Start from `.env.example`. Values marked required below are required by `Config::from_env()` even if a feature is not exercised.

| Variable | Required/default | Purpose |
| --- | --- | --- |
| `NODE_ENV` | Required | Runtime mode; `development` enables development-only options. |
| `PORT` | Required u16 | HTTP listen port. |
| `LOG_LEVEL` | Required | Default tracing filter for the application crate. |
| `DATABASE_URL` | Required | PostgreSQL connection; common URL variants are normalized. |
| `REDIS_URL` | Required | Redis cache/queue endpoint. |
| `BULLMQ_PREFIX` | Default `bull` | Redis key namespace. |
| `BULLMQ_CONCURRENCY` | Default `4` | Webhook worker concurrency. |
| `SUPABASE_URL` | Required | Supabase project URL. |
| `SUPABASE_PUBLISHABLE_KEY` | One Supabase key required | Auth user lookup API key; anon/legacy/service-role variable fallbacks exist. |
| `GITHUB_APP_ID` | Required | GitHub App identifier. |
| `GITHUB_APP_PRIVATE_KEY` | Required | GitHub App signing key; preserve PEM newlines/format. |
| `GITHUB_WEBHOOK_SECRET` | Required | Webhook HMAC secret. |
| `PLATFORM_STELLAR_PUBLIC_KEY` | Required | Platform address and platform fee receiver. |
| `PLATFORM_STELLAR_SECRET_KEY` | Required | Platform signing operations. |
| `DISPUTE_RESOLVER_STELLAR_PUBLIC_KEY` | Required | Dispute resolver identity. |
| `DISPUTE_RESOLVER_STELLAR_SECRET_KEY` | Required | Dispute-resolution signing. |
| `STELLAR_NETWORK` | Required | Network selection; `mainnet` changes explorer/network behavior. |
| `TOKEN_ADDRESS` | Required | Escrow token/asset address. |
| `TRUSTLESS_WORK_API_KEY` | Required | Trustless Work authorization. |
| `TRUSTLESS_WORK_BASE_URL` | Required | Trustless Work API base URL. |
| `TRUSTLESS_WORK_HEALTH_ESCROW_CONTRACT` | Built-in fallback | Contract used for the health probe. |
| `APP_URL` | Required | Frontend links in comments/emails; also CORS fallback. |
| `WEBHOOK_URL` | Optional | Public webhook URL used during setup. |
| `CORS_ALLOWED_ORIGINS` | Optional | Comma-separated origins; falls back to `APP_URL`, then localhost behavior. |
| `DEV_WEBHOOK_PROXY_ENABLED` | Required boolean | Effective only in development. |
| `AUTH_BYPASS` | Default false | Development-only fixed identity bypass. |
| `SMEE_SOURCE_URL` | Required | Development webhook relay source. |
| `SMEE_TARGET_URL` | Required by current config | Local relay target. |
| `RESEND_API_KEY` | Required | Email provider credential. |
| `EMAIL_FROM` | Default `Trustless-OSS <info@trustless-oss.xyz>` | Sender identity. |
| `AWS_ACCESS_KEY_ID` | Required | S3-compatible credential. |
| `AWS_SECRET_ACCESS_KEY` | Required | S3-compatible credential. |
| `AWS_REGION` | Default `us-east-1` | Storage region (`auto` is common for R2). |
| `S3_ENDPOINT_URL` | Optional | Custom S3-compatible endpoint. |
| `S3_BUCKET_NAME` | Required | Upload target bucket. |
| `TOASTY_CONNECTION_URL` | CLI-only optional override | Migration CLI database URL. |

Never commit populated secrets. `.env` is intended to remain local.

## 20. Local development

### Prerequisites

- Rust stable toolchain.
- Docker with Compose.
- PostgreSQL/Redis local containers.
- Integration credentials for the external features being tested.

### First run

```bash
cp .env.example .env
docker compose up -d
cargo run
```

Docker Compose starts PostgreSQL 16 on host port `5435` and Redis 7 on `6379`, both with persistent named volumes. A typical local database URL is:

```text
postgres://postgres:postgres@localhost:5435/trustless_oss
```

Verify:

```bash
curl http://localhost:5000/
curl http://localhost:5000/api/v1/health
```

Use `http://localhost:5000/swagger` for interactive endpoint documentation (substitute configured port).

### Migration commands

```bash
cargo run --bin migrate -- migration generate --name describe_change
cargo run --bin migrate -- migration apply
```

### Infrastructure commands

```bash
docker compose ps
docker compose logs -f postgres redis
docker compose down
```

`docker compose down -v` removes local database and Redis volumes; use it only when a destructive local reset is intended.

## 21. Testing and verification

Recommended checks:

```bash
cargo fmt --check
cargo check
cargo test
```

Unit tests cover parsing, labels, payout splits, state-machine decisions, authentication metadata extraction, OpenAPI coverage, rate limiting, and related helpers.

`tests/queue_bullmq.rs` exercises real Redis behavior: webhook dedupe, per-bounty job collapse, concurrent enqueue safety, delayed-job promotion, retry/backoff, dirty drains, and stalled-job recovery. These tests skip when Redis is unreachable; run `docker compose up -d redis` to exercise them.

For a financial lifecycle change, also test at least:

1. duplicate/redelivered webhook,
2. missing wallet then wallet connected,
3. PR author mismatch,
4. closed-but-unmerged PR,
5. insufficient escrow balance,
6. on-chain released but database stale,
7. repeated payout/retry request,
8. unassign/delete/reject after a milestone exists.

## 22. Observability and operations

### Logs

Structured `tracing` events include repository/issue/job IDs, triggers, decisions, attempts, and errors. `RUST_LOG` overrides the configured fallback filter; otherwise the crate uses `LOG_LEVEL` and Tower HTTP logs at info.

### Health

The combined health response checks PostgreSQL, Redis, environment values, and Trustless Work. Trustless Work degradation is reported in `checks`, but the overall health flag is driven by required dependencies such as database/Redis and environment. Individual dependency routes are useful for probes and diagnosis.

### Queue monitoring

`GET /api/v1/queue/stats` returns waiting, active, completed, failed, and delayed counts for available queues. BullMQ-compatible tooling can inspect Redis using the configured prefix.

### Deployment expectations

- Run the same binary for API and workers; workers are started inside the server process.
- Ensure all instances share PostgreSQL, Redis, webhook secret, and BullMQ prefix for that environment.
- Use different prefixes when intentionally sharing Redis across environments.
- Allow outbound HTTPS to Supabase, GitHub, Trustless Work, Stellar endpoints, Resend, and storage.
- Configure the public GitHub webhook URL and storage CORS correctly.
- Back up PostgreSQL; Redis queue data is operationally important but PostgreSQL/on-chain state are the durable business sources.

## 23. Security model

### Implemented controls

- Remote verification of Supabase access tokens.
- GitHub numeric identity extraction rather than trusting request-body identity.
- Repository-maintainer relationship checks for privileged operations.
- Constant-time HMAC verification through the HMAC library.
- Global IP limiter: burst 30, refill one token/second.
- Redis per-user limits on wallet and escrow operations.
- Client-side wallet signing for deploy/fund/close.
- Live chain/GitHub revalidation immediately before payout.
- Stable queue IDs, locks, dirty flags, retry classification, and no-op behavior for settled work.
- Presigned uploads scoped by authenticated GitHub ID with short expiry and validated folder/size.
- Consistent error responses that avoid returning secret material.

### Operational responsibilities

- Protect all Stellar secret keys, GitHub App private key/webhook secret, Supabase keys, Resend key, Trustless Work key, and S3 credentials.
- Never enable auth bypass outside development.
- Restrict database/Redis network access and use TLS-capable production URLs.
- Treat queue stats and public issue/leaderboard data as intentionally internet-visible unless routing is changed.
- Configure CORS to exact trusted frontend origins.
- Rotate credentials after accidental disclosure and audit on-chain/platform activity.
- Review any change to evaluator conditions, balance reservation, dispute resolution, or signing code as financially sensitive.

## 24. Source tree guide

```text
Toss-Backend/
├── src/
│   ├── main.rs                 # Process startup/server/workers/shutdown
│   ├── app.rs                  # Router and global middleware
│   ├── config.rs               # Environment parsing/validation
│   ├── state.rs                # Shared DB/Redis/queue/email/S3/HTTP state
│   ├── error.rs                # Error taxonomy and JSON mapping
│   ├── lifecycle.rs            # Shutdown state and signals
│   ├── telemetry.rs            # Tracing setup
│   ├── bin/cli.rs              # Toasty migration CLI
│   ├── docs/openapi.rs         # OpenAPI registry
│   ├── middleware/             # Auth and rate limits
│   ├── routes/                 # Health and queue endpoints
│   ├── infra/
│   │   ├── db.rs, migrate.rs   # Database and startup migrations
│   │   ├── queue.rs, jobs/     # BullMQ hub and job processors
│   │   ├── redis/              # Client/cache/health
│   │   ├── stellar/            # Account checks and signing
│   │   ├── email/              # Resend provider/templates
│   │   └── s3_client/          # Storage client
│   ├── modules/
│   │   ├── github/             # App auth, webhook dispatch, handlers, comments
│   │   ├── repo/               # Repository/reward API and persistence
│   │   ├── bounty/             # State machine, controls, payout, repository
│   │   ├── escrow/             # API and Trustless Work integration
│   │   ├── contributor/        # Profile/wallet API
│   │   ├── notification/       # Bell/email policy and persistence
│   │   ├── dashboard/          # Metrics/leaderboards
│   │   ├── ticket/             # Support tickets
│   │   └── upload.rs           # Presigned uploads
│   └── shared/                 # Constants, pagination, domain/schema models
├── tests/queue_bullmq.rs        # Redis-backed integration tests
├── toasty/                      # Migration history, snapshot, SQL
├── docs/                        # Project/API/schema/job documentation
├── docker-compose.yml           # Local PostgreSQL and Redis
├── Toasty.toml                  # Migration tool configuration
├── Cargo.toml                   # Crate/dependencies/binaries
└── .env.example                 # Configuration template
```

## 25. Failure modes and troubleshooting

| Symptom | Likely cause | What to check |
| --- | --- | --- |
| Startup panics with config error | Required variable missing/empty/malformed | Compare `.env` with the configuration table; validate `PORT`, booleans, and PEM formatting. |
| Startup fails before listening | PostgreSQL unavailable or migration error | `DATABASE_URL`, container health/logs, migration SQL. |
| API starts but automation does not run | BullMQ initialization failed or Redis unavailable | Startup logs, `/api/v1/health/redis`, queue stats, `REDIS_URL` and prefix. |
| `401 Unauthorized` | Missing/expired token or GitHub identity absent in Supabase user | Supabase user endpoint, provider metadata, bearer header. |
| `403` on repository action | Caller is authenticated but not stored as maintainer | `repo_maintainers` relation and installation/connect flow. |
| GitHub returns webhook failures | Secret mismatch or wrong public URL | Exact raw request, `X-Hub-Signature-256`, webhook secret, delivery logs. |
| Bounty waits for wallet | No usable primary/available payout wallet | Contributor profile, Stellar address/USDC readiness, wallet-connect response. |
| Bounty waits after merge | PR lacks closing reference, wrong author, issue not closed as completed, or webhook missing | PR body, assignee, GitHub event deliveries, evaluator logs. |
| Bounty is blocked | Unsafe DB/chain mismatch, missing escrow, receiver/amount inconsistency | Decision reason in logs/notification, contract viewer, stored bounty/repository. |
| Funding/payout fails transiently | Trustless Work/Stellar/network error | Health route, external response logs, queue retry/failed counts. |
| Email absent | No profile email, Resend failure, queue unavailable, deduped notification | Notification `email_status`, profile email, notification worker logs. |
| Presigned upload fails | Bucket CORS, wrong headers/length, expired URL, endpoint/region mismatch | Use returned headers exactly within five minutes; verify S3 configuration. |
| Queue integration tests silently skip | Redis not reachable | Start Compose Redis and verify `REDIS_URL`. |

## 26. Current implementation notes

These details are important when comparing code, README, older docs, and generated OpenAPI:

1. The current router includes dashboard, public leaderboard, tickets, notifications, uploads, contributor profile update, GitHub-label listing, and reward deletion. Some older summaries omit them.
2. The runtime OpenAPI registry does not currently register every routed operation (notably contributor profile update and some reward/label routes), so the route table in this document is broader than Swagger.
3. Queue-stat response types/documentation mention a `sync` queue, but the current queue hub creates webhook, bounty, and notification queues. Enabled stats currently report webhook and `escrow-operations` (bounty) counts; disabled stats include a zero-valued `sync` field. Notification queue counts are not exposed.
4. `.env.example` comments describe `SMEE_TARGET_URL` as defaultable, but current `Config::from_env()` requires both Smee source and target strings. Likewise, most integrations are required at startup even when not being actively tested.
5. `Config` stores AWS access-key variables to validate their presence, while the AWS SDK obtains credentials through its normal environment/provider chain.
6. The combined health check reports Trustless Work as degraded without necessarily making the overall response unhealthy; database and Redis failures do make it `503`.
7. Queue-disabled webhook processing has an inline fallback. Bounty enqueue operations return `Unavailable`; callers may still return success, so operators must watch Redis/queue health.
8. The state machine is the authoritative payout gate. Retry endpoints and bot commands request evaluation; they do not bypass the live safety checks.

---

## Further reading

- [End-to-end API guide](APIs.md)
- [Background job design](jobs.md)
- [Schema overview](schema.md)
- [Table-by-table schema notes](tables_schema.md)
- [Repository README](../README.md)

