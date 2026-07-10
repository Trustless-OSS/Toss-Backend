# ⚡️⚡️ Trustless-OSS Axum Backend ⚡️⚡️

This repository contains the Rust + Axum backend for Trustless OSS. It provides the API surface for the platform, background jobs, and integrations with PostgreSQL, Redis, GitHub, and Stellar.

## Prerequisites

- Rust toolchain
- Docker Compose
- A local environment file based on [.env.example](.env.example)

## Quick start

1. Copy the sample environment configuration:

   ```bash
   cp .env.example .env
   ```

2. Start the supporting services:

   ```bash
   docker compose up -d
   ```

3. Run the backend:

   ```bash
   cargo run
   ```

The server listens on the `PORT` environment variable and defaults to `5000`.

## Health endpoints

- `GET /` — service status
- `GET /health` — health check
- `GET /api/health` — API-style health check

## Useful commands

```bash
cargo fmt
cargo test
cargo build
docker compose down
```
