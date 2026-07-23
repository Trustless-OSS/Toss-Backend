# Migration Issues — Rust Axum Backend

High-level work items to bring the Rust + Axum backend to **feature parity** with the TypeScript reference in `backend/`.

These are contributor-facing GitHub issue drafts. Copy the body of any file into a new GitHub issue (or use `gh issue create`).

| # | Issue | Priority | Status |
|---|-------|----------|--------|
| 001 | [Wire & harden GitHub webhook job dispatcher](./001-webhook-dispatcher.md) | P0 | Partially done — issue handlers wired; comment/PR still TODO |
| 002 | [Implement issue comment command handler](./002-issue-comment-commands.md) | P0 | Not started |
| 003 | [Implement PR merge payout handler](./003-pr-merge-payout.md) | P0 | Not started |
| 004 | [Production-grade webhook job queue](./004-production-job-queue.md) | P1 | Not started |
| 005 | [Webhook & bounty lifecycle integration tests](./005-webhook-integration-tests.md) | P1 | Not started |

## Reference implementation

- TypeScript webhook processor: `backend/src/lib/queue-processors.ts`
- TypeScript event handlers: `backend/src/lib/github/webhook.ts`
- Rust dispatcher: `src/modules/github/webhook.rs`
- Rust handlers: `src/modules/github/handlers/`

## Deferred (future issues)

Not tracked here on purpose — file separately later:

- `GET /api/debug/auth` diagnostic endpoint
- Graceful worker drain on shutdown
- CORS restricted to `FRONTEND_URL`
- Request-ID / active-request lifecycle middleware wiring
- Installation filter: User accounts only (reject Organizations)
- Refund wallet policy alignment (funder vs maintainer)
- Dev webhook proxy / load-test tooling
- `escrow-operations` queue worker (unused in both codebases)
