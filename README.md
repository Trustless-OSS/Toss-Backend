<div align="center">

<img width="280" alt="toss" src="https://github.com/user-attachments/assets/eb15200e-c4c5-4405-aca1-bbb692bd3480" />


[![Rust CI](https://github.com/Trustless-OSS/Toss-Backend/actions/workflows/rust.yml/badge.svg)](https://github.com/Trustless-OSS/Toss-Backend/actions/workflows/rust.yml)
![Rust 2021](https://img.shields.io/badge/Rust-2021-000000?logo=rust&logoColor=white)
![Axum](https://img.shields.io/badge/Axum-0.8-7C3AED)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-4169E1?logo=postgresql&logoColor=white)
![Toasty](https://img.shields.io/badge/Toasty-0.10-EF5B25)
![Redis](https://img.shields.io/badge/Redis-7-DC382D?logo=redis&logoColor=white)

[Overview](#-overview) • [How it works](#-how-it-works) • [Quick start](#-quick-start) • [API map](#-api-map) • [Development](#-development)

</div>

---

## 🌟 Overview

Toss Backend is the orchestration layer for the Trustless OSS platform. It
connects GitHub activity with bounty state, contributor wallets, and escrow
operations while keeping slow or retryable work in background jobs.

| Capability | What it handles |
| --- | --- |
| 🐙 **GitHub integration** | GitHub App installations, repository sync, signed webhooks, issues, assignments, labels, and pull requests |
| 🎯 **Bounty lifecycle** | Reward tiers, contributor assignment, milestone creation, retries, and payout status |
| 🔐 **Authentication** | Supabase bearer-token verification with GitHub identity extraction |
| 💸 **Escrow operations** | Unsigned transaction creation, deployment, funding, refunds, closure, and submission through Trustless Work |
| ⚙️ **Background processing** | Redis-backed webhook jobs, scheduled work, retries, and queue statistics |
| 🩺 **Operations** | Toasty migrations, dependency-aware health checks, request tracing, and graceful shutdown |

## 🔄 How it works

```mermaid
flowchart LR
    GH["🐙 GitHub App<br/>events & webhooks"] -->|HMAC verified| API["⚡ Axum API"]
    UI["🖥️ Trustless OSS<br/>frontend"] -->|Supabase bearer token| API

    API --> DB[("🐘 PostgreSQL<br/>repos, issues & assignments")]
    API --> REDIS[("🔴 Redis<br/>cache & job queue")]
    REDIS --> WORKERS["⚙️ Background workers"]
    WORKERS --> GH

    API --> TW["🤝 Trustless Work API"]
    TW --> STELLAR["🌐 Stellar escrow"]
```

The common bounty journey is:

1. A maintainer connects a repository and configures reward tiers.
2. GitHub sends signed issue, assignment, label, and pull-request events.
3. The backend records bounty state and processes retryable work through Redis.
4. A contributor connects a payout wallet and the milestone is pushed to escrow.
5. Completion events move the bounty toward release and update its payout state.

## 🧰 Tech stack

| Layer | Technology |
| --- | --- |
| API | Rust 2021, Axum 0.8, Tokio |
| Data | PostgreSQL 16, [Toasty](https://github.com/tokio-rs/toasty) 0.10 ORM, Redis 7 |
| Authentication | Supabase Auth, GitHub identity |
| Integrations | GitHub App API, Trustless Work, Stellar |
| Observability | `tracing`, dependency-aware health checks |
| Local infrastructure | Docker Compose (PostgreSQL + Redis only) |

## 🚀 Quick start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) stable toolchain (MSRV for Toasty is ~1.95+)
- [Docker](https://docs.docker.com/get-docker/) with Docker Compose
- GitHub App, Supabase, Stellar, and Trustless Work credentials for the
  integration flows you want to exercise

### 1. Configure the environment

```bash
cp .env.example .env
```

Open `.env` and replace the placeholder credentials. The application validates
its required configuration at startup, so all required values must be present.
Never commit the populated `.env` file.

Local defaults match Docker Compose:

| Service | URL |
| --- | --- |
| PostgreSQL | `postgres://postgres:postgres@localhost:5435/trustless_oss` |
| Redis | `redis://localhost:6379` |

### 2. Start PostgreSQL and Redis

```bash
docker compose up -d
docker compose ps
```

This starts PostgreSQL on `localhost:5435` and Redis on `localhost:6379`.

### 3. Apply database migrations

Schema is managed with Toasty (not applied automatically on server startup):

```bash
# First time / after a clean database
cargo run --bin migrate -- migration generate --name initial   # only if toasty/ is missing
cargo run --bin migrate -- migration apply
```

After model changes under `src/shared/models/schema/`:

```bash
cargo run --bin migrate -- migration generate --name describe_your_change
cargo run --bin migrate -- migration apply
```

Useful extras:

```bash
cargo run --bin migrate -- migration --help
cargo run --bin migrate -- migration reset   # drops all tables (destructive)
```

Run migrate commands from the crate root so `Toasty.toml` is found. Migration
files live under `toasty/` — commit that folder.

### 4. Run the API

```bash
cargo run
```

`cargo run` starts the `toss-backend` server (default binary). It listens at
`http://localhost:5000` by default.

### 5. Verify the service

```bash
curl http://localhost:5000/
curl http://localhost:5000/api/health
```

The root endpoint confirms that the API is running. The detailed health endpoint
also reports PostgreSQL, Redis, environment, and Trustless Work status.

> [!TIP]
> If port `5000` is already in use, change `PORT` in `.env` and use the same
> port in your health-check URL.

## 🔑 Environment guide

The complete template lives in [`.env.example`](.env.example).

| Group | Variables | Purpose |
| --- | --- | --- |
| Runtime | `NODE_ENV`, `PORT`, `LOG_LEVEL` | Server mode, address, and logging |
| Infrastructure | `DATABASE_URL`, `REDIS_URL` | PostgreSQL, cache, and job queue |
| Authentication | `SUPABASE_URL`, `SUPABASE_PUBLISHABLE_KEY` | Bearer-token verification |
| GitHub | `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY`, `GITHUB_WEBHOOK_SECRET` | App authentication and webhook verification |
| Stellar | `STELLAR_NETWORK`, platform and dispute-resolver keypairs | Transaction signing and network selection |
| Trustless Work | `TRUSTLESS_WORK_API_KEY`, `TRUSTLESS_WORK_BASE_URL` | Escrow API access |
| Application | `APP_URL`, `WEBHOOK_URL` | Frontend and public webhook locations |
| Local webhook relay | `DEV_WEBHOOK_PROXY_ENABLED`, `SMEE_SOURCE_URL`, `SMEE_TARGET_URL` | Optional development-only GitHub relay |

`GITHUB_BOT_TOKEN` is optional and is only needed by paths that fetch GitHub
issue state directly.

The migrate binary also accepts `TOASTY_CONNECTION_URL` as an override for
`DATABASE_URL`.

## 🗺️ API map

Protected application routes use `Authorization: Bearer <supabase-access-token>`.
GitHub webhooks instead require a valid `X-Hub-Signature-256` signature.

| Area | Main endpoints |
| --- | --- |
| System | `GET /`, `GET /health`, `GET /api/health`, `GET /api/queue/stats` |
| Repositories | `GET /api/repos`, `POST /api/repos/connect`, `POST /api/repos/sync-installation`, `GET/DELETE /api/repos/{repoId}` |
| Issues and rewards | `GET /api/repos/{repoId}/issues`, `PUT /api/repos/{repoId}/rewards`, `POST /api/issues/{issueId}/retry` |
| Contributors | `POST /api/wallet/connect`, `GET /api/contributor/me` |
| Milestones | `POST /api/milestones/push` |
| Escrow | `POST /api/escrow/create-unsigned`, `/submit-deploy`, `/fund-unsigned`, `/submit-fund`, `/refund`, `/close-unsigned`, `/submit-close` |
| GitHub | `POST /api/webhooks/github` |
| Docs | `GET /swagger` |

## 🗂️ Project structure

```text
Toss-Backend/
├── src/
│   ├── bin/migrate.rs  # Toasty migration CLI
│   ├── modules/        # Repo, GitHub, bounty, contributor, and escrow domains
│   ├── shared/models/  # Entity DTOs + Toasty schema models
│   ├── infra/          # PostgreSQL (Toasty), Redis, queue, cache, Stellar
│   ├── middleware/     # Authentication and request middleware
│   ├── routes/         # Health and operational routes
│   ├── lib.rs          # Shared library crate
│   ├── config.rs       # Environment configuration
│   ├── app.rs          # Axum router assembly
│   └── main.rs         # Server startup and graceful shutdown
├── toasty/             # Generated SQL migrations, snapshots, history
├── Toasty.toml         # Toasty migration config
├── docker-compose.yml  # Local PostgreSQL + Redis
└── .env.example        # Safe configuration template
```

## 🛠️ Development

Run the same core checks used by the project before opening a pull request:

```bash
cargo fmt --check
cargo build
cargo test
```

Useful local commands:

```bash
# API server (default binary)
cargo run

# Migrations
cargo run --bin migrate -- migration generate --name my_change
cargo run --bin migrate -- migration apply

# Follow infrastructure logs
docker compose logs -f postgres redis

# Stop local infrastructure
docker compose down

# Stop and remove local database/cache volumes
docker compose down -v
```

> [!WARNING]
> `docker compose down -v` deletes the local PostgreSQL and Redis volumes.
> `migration reset` drops all tables in the connected database. Use either only
> when you intentionally want a clean local data reset.

## 🤝 Contributing

Issues and pull requests are welcome. Before submitting a change:

1. Keep changes focused on one concern.
2. Add or update tests for changed behavior.
3. Run the formatting, build, and test commands above.
4. Explain any configuration or migration changes in the pull request.
5. Commit updated files under `toasty/` when you change schema models.

Have an idea or found a bug? [Open an issue](https://github.com/Trustless-OSS/Toss-Backend/issues).

---

<div align="center">

Built for open-source contributors by **Trustless OSS**.

</div>
