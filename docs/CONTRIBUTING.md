# Contributing

Thanks for contributing to the Trustless-OSS Axum backend! This document covers
the database workflow. For general setup, see the [README](../README.md).

## Database layer

The backend uses **[Diesel](https://diesel.rs/)** (via
[`diesel-async`](https://github.com/weiznich/diesel_async) with a `bb8`
connection pool) for all database access. Queries are written with Diesel's
query builder against the generated schema in [`src/schema.rs`](../src/schema.rs)
— there is no hand-written SQL in the application code and `sqlx` is no longer a
dependency.

### Running migrations

Migrations live under [`migrations/`](../migrations) as Diesel `up.sql` /
`down.sql` pairs. The application **runs all pending migrations automatically on
startup** (see `run_migrations` in [`src/main.rs`](../src/main.rs)), so for local
development you usually just need:

```bash
docker compose up -d   # start Postgres + Redis
cargo run              # applies pending migrations, then serves
```

### Working with migrations manually (Diesel CLI)

To create or run migrations by hand, install the Diesel CLI (Postgres backend
only) and point it at your database:

```bash
cargo install diesel_cli --no-default-features --features postgres

export DATABASE_URL="postgres://postgres:postgres@localhost:5435/trustless_oss"

diesel migration run       # apply pending migrations
diesel migration revert    # roll back the most recent migration
diesel migration redo      # revert + re-apply (tests the down.sql)
```

> The old `sqlx migrate` / `sqlx::migrate!` workflow has been removed. Do not add
> raw `.sql` migration files at the top level of `migrations/`; use the Diesel
> `<timestamp>_<name>/{up,down}.sql` directory format instead.

### Adding a new migration

```bash
diesel migration generate <descriptive_name>
```

Edit the generated `up.sql` and `down.sql`, then regenerate the schema so the
query builder stays in sync:

```bash
diesel print-schema > src/schema.rs
```

Commit both the migration files and the updated `src/schema.rs`.

### Models

Diesel models mirroring the tables live in
[`src/shared/models.rs`](../src/shared/models.rs) (`Repo`, `Contributor`,
`Issue`, `Assignment`). They derive `Queryable` and `Selectable` and are kept in
sync with `src/schema.rs`. When you change a table, update both the migration and
the corresponding model.
