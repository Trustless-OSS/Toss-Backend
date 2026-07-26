-- Durable idempotency ledger for GitHub webhook deliveries.
--
-- This is a Postgres-backed backstop for the Redis-backed queue's own
-- dedup keys. Redis holds the fast-path "have I seen this delivery id"
-- check; this table guarantees that a handler's money-moving side effects
-- (bounty creation, escrow reservation, milestone release, payouts,
-- refunds) run at most once per GitHub delivery id even if the Redis
-- dedup key is lost (flush, eviction, instance replacement).
create table if not exists webhook_deliveries (
  id uuid primary key default gen_random_uuid(),
  delivery_id text not null,
  event text not null,
  action text,
  -- queued | processing | completed | dead_lettered
  status text not null default 'processing',
  job_id text,
  attempts integer not null default 1,
  first_attempt_at timestamptz not null default now(),
  last_attempt_at timestamptz not null default now(),
  completed_at timestamptz,
  last_error text,
  correlation_id text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

-- A GitHub delivery id is globally unique per delivery attempt; this is
-- the constraint that makes "claim this delivery" a single atomic
-- INSERT ... ON CONFLICT DO NOTHING rather than a read-then-write race.
create unique index if not exists webhook_deliveries_delivery_id_key
  on webhook_deliveries (delivery_id);

create index if not exists webhook_deliveries_status_idx
  on webhook_deliveries (status);
