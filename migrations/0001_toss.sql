-- ============================================================
--  Trustless-OSS
-- ============================================================


-- ============================================================
-- TABLE: repos
-- One row per GitHub repo connected to the platform.
-- ============================================================
create table if not exists repos (
  id                      uuid primary key default gen_random_uuid(),
  github_repo_id          bigint unique not null,
  github_installation_id  bigint,
  full_name               text not null,
  owner_github_id         bigint not null,
  owner_username          text not null,
  owner_type              text check (owner_type in ('User', 'Organization')),
  installer_github_id     bigint,
  is_fork                 boolean default false,
  is_private              boolean default false,
  escrow_contract_id      text,
  escrow_balance          numeric default 0,
  created_at              timestamptz default now()
);
-- Note: reward_low / reward_medium / reward_high have been removed.
-- Tier pricing now lives in reward_tiers below, so a label and its
-- amount can never drift apart.

-- Wallet address that funded/holds the repo's escrow contract.
alter table repos
  add column if not exists escrow_funder_wallet text;


-- ============================================================
-- TABLE: reward_tiers
-- Per-repo difficulty tiers (low/medium/high/custom) and the
-- bounty amount each one pays. Repo owners can define tiers
-- beyond the default three (e.g. "epic", "quick-fix").
-- ============================================================
create table if not exists reward_tiers (
  id          uuid primary key default gen_random_uuid(),
  repo_id     uuid not null references repos(id) on delete cascade,
  label       text not null,               -- 'low' | 'medium' | 'high' | custom name
  amount      numeric not null check (amount >= 0),
  sort_order  int default 0,
  created_at  timestamptz default now(),
  updated_at  timestamptz default now(),
  unique (repo_id, label)                  -- no duplicate tier names per repo
);


-- ============================================================
-- TABLE: contributors
-- GitHub users who are eligible to receive bounty payouts.
-- ============================================================
create table if not exists contributors (
  id              uuid primary key default gen_random_uuid(),
  github_user_id  bigint unique not null,
  github_username text not null,
  stellar_wallet  text,
  payout_chain    text default 'stellar',
  payout_address  text,
  created_at      timestamptz default now()
);


-- ============================================================
-- TABLE: issues
-- Bounty-enabled GitHub issues. reward_amount and difficulty_label
-- are SNAPSHOTTED from reward_tiers at creation time — if a repo
-- later edits its tier pricing, already-created issues keep their
-- original value instead of silently changing.
-- ============================================================
create table if not exists issues (
  id                  uuid primary key default gen_random_uuid(),
  repo_id             uuid not null references repos(id) on delete cascade,
  reward_tier_id      uuid references reward_tiers(id) on delete set null,
  github_issue_id     bigint not null,
  github_issue_number int not null,
  title               text not null,
  reward_amount       numeric not null check (reward_amount >= 0),  -- snapshot
  difficulty_label    text not null,                                -- snapshot
  milestone_index     int,
  status              text default 'pending'
                        check (status in ('pending', 'active', 'completed', 'cancelled')),
  created_at          timestamptz default now(),
  unique (repo_id, github_issue_id)
);


-- ============================================================
-- TABLE: assignments
-- Tracks which contributor is working on which issue, and the
-- lifecycle of their PR through to payout.
-- ============================================================
create table if not exists assignments (
  id                     uuid primary key default gen_random_uuid(),
  issue_id               uuid not null references issues(id) on delete cascade,
  contributor_id         uuid not null references contributors(id),
  assigned_at            timestamptz default now(),
  pr_number              int,
  pr_merged_at           timestamptz,
  payout_status          text default 'pending'
                           check (payout_status in ('pending', 'released', 'failed')),
  completion_percentage  numeric
                           check (completion_percentage >= 0 and completion_percentage <= 100)
                           default null,
  unique (issue_id)      -- one active assignment per issue
);


-- ============================================================
-- TRIGGER: keep reward_tiers.updated_at fresh on every edit
-- ============================================================
create or replace function set_updated_at()
returns trigger as $$
begin
  new.updated_at = now();
  return new;
end;
$$ language plpgsql;

drop trigger if exists trg_reward_tiers_updated_at on reward_tiers;
create trigger trg_reward_tiers_updated_at
  before update on reward_tiers
  for each row
  execute function set_updated_at();


-- ============================================================
-- ROW LEVEL SECURITY — DO NOT APPLY HERE
-- RLS policies use Supabase-specific functions (auth.jwt(),
-- auth.role()) and must be set up directly in the Supabase
-- dashboard or CLI. Running them in this file will fail on plain
-- PostgreSQL (e.g. Render managed Postgres), which has no `auth`
-- schema.
-- ============================================================


-- ============================================================
-- INDEXES
-- ============================================================

-- repos
create index if not exists idx_repos_github_id           on repos(github_repo_id);

-- reward_tiers
create index if not exists idx_reward_tiers_repo         on reward_tiers(repo_id);

-- issues
create index if not exists idx_issues_repo               on issues(repo_id);
create index if not exists idx_issues_github_id           on issues(github_issue_id);
create index if not exists idx_issues_status              on issues(status);
create index if not exists idx_issues_reward_tier         on issues(reward_tier_id);

-- assignments
create index if not exists idx_assignments_issue          on assignments(issue_id);
create index if not exists idx_assignments_contributor    on assignments(contributor_id);
create index if not exists idx_assignments_payout_status  on assignments(payout_status);

-- contributors
create index if not exists idx_contributors_github_id     on contributors(github_user_id);


-- ============================================================
-- ONE-TIME MIGRATION (run only if upgrading an existing database
-- that still has repos.reward_low / reward_medium / reward_high
-- with data in them). Uncomment, run once, then delete this block.
-- ============================================================

-- insert into reward_tiers (repo_id, label, amount, sort_order)
-- select id, 'low',    reward_low,    1 from repos where reward_low    is not null
-- union all
-- select id, 'medium', reward_medium, 2 from repos where reward_medium is not null
-- union all
-- select id, 'high',   reward_high,   3 from repos where reward_high   is not null
-- on conflict (repo_id, label) do nothing;

-- alter table repos drop column if exists reward_low;
-- alter table repos drop column if exists reward_medium;
-- alter table repos drop column if exists reward_high;