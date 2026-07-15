-- repos: one row per connected GitHub repo
create table if not exists repos (
  id                  uuid primary key default gen_random_uuid(),
  github_repo_id      bigint unique not null,
  github_installation_id bigint,
  full_name           text not null,
  owner_github_id     bigint not null,
  owner_username      text not null,
  owner_type          text check (owner_type in ('User', 'Organization')),
  installer_github_id bigint,
  is_fork             boolean default false,
  is_private          boolean default false,
  escrow_contract_id  text,
  escrow_balance      numeric default 0,
  reward_low          numeric default 0,
  reward_medium       numeric default 0,
  reward_high         numeric default 0,
  created_at          timestamptz default now()
);

-- contributors: GitHub users who receive bounties
create table if not exists contributors (
  id                uuid primary key default gen_random_uuid(),
  github_user_id    bigint unique not null,
  github_username   text not null,
  stellar_wallet    text,
  created_at        timestamptz default now()
);

-- issues: bounty-enabled GitHub issues
create table if not exists issues (
  id                    uuid primary key default gen_random_uuid(),
  repo_id               uuid references repos(id) on delete cascade,
  github_issue_id       bigint not null,
  github_issue_number   int not null,
  title                 text not null,
  reward_amount         numeric not null,
  difficulty_label      text check (difficulty_label in ('low', 'medium', 'high', 'custom')),
  milestone_index       int,
  status                text default 'pending' check (status in ('pending', 'active', 'completed', 'cancelled')),
  created_at            timestamptz default now(),
  unique(repo_id, github_issue_id)
);

-- assignments: which contributor is working on which issue
create table if not exists assignments (
  id                    uuid primary key default gen_random_uuid(),
  issue_id              uuid references issues(id) on delete cascade,
  contributor_id        uuid references contributors(id),
  assigned_at           timestamptz default now(),
  pr_number             int,
  pr_merged_at          timestamptz,
  payout_status         text default 'pending' check (payout_status in ('pending', 'released', 'failed')),
  completion_percentage numeric check (completion_percentage >= 0 and completion_percentage <= 100) default null,
  unique(issue_id)
);

-- Ensure completion_percentage column exists for existing tables (idempotent)
ALTER TABLE assignments ADD COLUMN IF NOT EXISTS completion_percentage numeric check (completion_percentage >= 0 and completion_percentage <= 100) default null;

-- ============================================================
-- NOTE: Row Level Security (RLS) policies use Supabase-specific
-- functions (auth.jwt(), auth.role()) and must be applied directly
-- in the Supabase dashboard or via the Supabase CLI — NOT here.
-- Applying them in this migration will fail on plain PostgreSQL
-- (e.g. Render managed Postgres) which lacks the `auth` schema.
-- ============================================================

-- ============================================================
-- Indexes for performance
-- ============================================================
-- These indexes are critical for optimizing the listIssuesHandler query
-- which fetches issues with assignments and contributors in a single call.
-- They prevent N+1 queries by enabling efficient joins on foreign keys.

create index if not exists idx_repos_github_id       on repos(github_repo_id);
create index if not exists idx_issues_repo            on issues(repo_id);
create index if not exists idx_issues_github_id       on issues(github_issue_id);
create index if not exists idx_assignments_issue      on assignments(issue_id);
create index if not exists idx_contributors_github_id on contributors(github_user_id);

-- Payout CCTP Columns
ALTER TABLE contributors ADD COLUMN IF NOT EXISTS payout_chain text default 'stellar';
ALTER TABLE contributors ADD COLUMN IF NOT EXISTS payout_address text;
