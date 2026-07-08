# Trustless Backend Module

This is a parallel Rust + Axum backend module for Trustless-OSS.

It is intentionally scoped as an incremental migration surface that can grow
route-by-route alongside the existing TypeScript backend in `apps/backend`.

## Run

Start the local infra first:

```bash
docker compose up -d postgres redis
```

Then create the Axum env file from the example and run the service:

```bash
cp apps/backend-axum/.env.example apps/backend-axum/.env
cargo run --manifest-path apps/backend-axum/Cargo.toml
```

The server listens on `PORT`, defaulting to `4001`.

## Routes

- `GET /` — module status
- `GET /health` — health check
- `GET /api/health` — API-shaped health check
