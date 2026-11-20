# TOSS — Database Schema Reference

> **Stack:** PostgreSQL · UUID primary keys · TIMESTAMPTZ for all datetimes · NUMERIC(20,7) for on-chain amounts
>
> **Conventions:**
> - Every FK references the `id` (UUID) PK of the parent table — never a natural key like `github_id`
> - `?` suffix = nullable column
> - `failed_tasks` records durable recovery state for background work
> - Derived values (share %, statistics) live in queries / materialized views — not stored columns

---

## Table of Contents

1. [profiles](#1-profiles)
2. [wallets](#2-wallets)
3. [repositories](#3-repositories)
4. [repo_maintainers](#4-repo_maintainers)
5. [reward_levels](#5-reward_levels)
6. [bounties](#6-bounties)
7. [notifications](#7-notifications)
8. [failed_tasks](#8-failed_tasks)
9. [contributor_stats (Materialized View)](#9-contributor_stats-materialized-view)
10. [Relationships Overview](#10-relationships-overview)
11. [Indexes](#11-indexes)

---

## 1. `profiles`

Stores every TOSS user regardless of role. A user can be a contributor, maintainer, funder, or all three — role is determined by relationships, not a flag on this table.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | Internal surrogate key |
| `github_id` | BIGINT | UNIQUE NOT NULL | GitHub's numeric user ID |
| `username` | TEXT | UNIQUE NOT NULL | GitHub login handle e.g. `torvalds` |
| `full_name` | TEXT | nullable | Display name from GitHub |
| `email` | TEXT | nullable | From GitHub OAuth scope |
| `avatar_url` | TEXT | nullable | GitHub avatar URL |
| `bio` | TEXT | nullable | User-written description |
| `location` | TEXT | nullable | |
| `skills` | TEXT[] | nullable | e.g. `{Rust, TypeScript, Solidity}` |
| `telegram` | TEXT | nullable | Handle without @ |
| `discord` | TEXT | nullable | user#discriminator or new handle |
| `twitter` | TEXT | nullable | Handle without @ |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |
| `updated_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | Updated on any profile edit |

```sql
CREATE TABLE profiles (
  id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  github_id   BIGINT      UNIQUE NOT NULL,
  username    TEXT        UNIQUE NOT NULL,
  full_name   TEXT,
  email       TEXT,
  avatar_url  TEXT,
  bio         TEXT,
  location    TEXT,
  skills      TEXT[],
  telegram    TEXT,
  discord     TEXT,
  twitter     TEXT,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

---

## 2. `wallets`

A user can have multiple wallets across multiple chains. One wallet per chain can be marked primary for payouts.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | |
| `profile_id` | UUID | FK → profiles.id, NOT NULL | Cascade delete |
| `chain` | TEXT | NOT NULL | e.g. `stellar`, `evm` |
| `address` | TEXT | NOT NULL | On-chain address |
| `is_primary` | BOOLEAN | NOT NULL, DEFAULT false | Primary payout wallet per chain |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |
| `updated_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

**Unique constraint:** `(profile_id, chain, address)` — same address can't be added twice per user per chain.

```sql
CREATE TABLE wallets (
  id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  profile_id  UUID        NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  chain       TEXT        NOT NULL,
  address     TEXT        NOT NULL,
  is_primary  BOOLEAN     NOT NULL DEFAULT false,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (profile_id, chain, address)
);
```

---

## 3. `repositories`

A GitHub repo registered with the TOSS GitHub App. Holds the link between GitHub identity and the on-chain escrow contract.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | |
| `github_repo_id` | BIGINT | UNIQUE NOT NULL | GitHub's internal numeric repo ID |
| `github_install_id` | BIGINT | nullable | GitHub App installation ID |
| `full_name` | TEXT | UNIQUE NOT NULL | Format: `owner/repo` e.g. `torvalds/linux` |
| `escrow_contract_id` | TEXT | nullable | On-chain escrow contract address |
| `escrow_balance` | NUMERIC(20,7) | nullable | Cached balance — see balance_synced_at |
| `balance_synced_at` | TIMESTAMPTZ | nullable | When escrow_balance was last synced from chain |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

```sql
CREATE TABLE repositories (
  id                  UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
  github_repo_id      BIGINT        UNIQUE NOT NULL,
  github_install_id   BIGINT,
  full_name           TEXT          UNIQUE NOT NULL,
  escrow_contract_id  TEXT,
  escrow_balance      NUMERIC(20,7),
  balance_synced_at   TIMESTAMPTZ,
  created_at          TIMESTAMPTZ   NOT NULL DEFAULT now()
);
```

---

## 4. `repo_maintainers`

Junction table — many-to-many between `profiles` and `repositories`. Tracks who has maintainer access to which repo and at what role level.

> No surrogate `id` needed — composite PK is sufficient and prevents duplicate entries.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `repo_id` | UUID | PK, FK → repositories.id | Cascade delete |
| `profile_id` | UUID | PK, FK → profiles.id | Cascade delete |
| `role` | TEXT | NOT NULL, DEFAULT 'maintainer' | `owner` or `maintainer` |
| `added_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

```sql
CREATE TABLE repo_maintainers (
  repo_id     UUID        NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
  profile_id  UUID        NOT NULL REFERENCES profiles(id)     ON DELETE CASCADE,
  role        TEXT        NOT NULL DEFAULT 'maintainer',
  added_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (repo_id, profile_id),
  CONSTRAINT role_check CHECK (role IN ('owner', 'maintainer'))
);
```

---

## 5. `reward_levels`

Per-repo bounty tiers defined by the maintainer. A label (e.g. `bug`, `critical`) maps to a fixed payout amount drawn from the repo's escrow.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | |
| `repo_id` | UUID | FK → repositories.id, NOT NULL | Cascade delete |
| `label` | TEXT | NOT NULL | Matches GitHub issue label e.g. `bounty:bug` |
| `amount` | NUMERIC(20,7) | NOT NULL | Payout in native token (XLM etc.) |
| `updated_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

**Unique constraint:** `(repo_id, label)` — one amount per label per repo.

```sql
CREATE TABLE reward_levels (
  id          UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
  repo_id     UUID          NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
  label       TEXT          NOT NULL,
  amount      NUMERIC(20,7) NOT NULL,
  updated_at  TIMESTAMPTZ   NOT NULL DEFAULT now(),
  UNIQUE (repo_id, label)
);
```

---

## 6. `bounties`

One row per GitHub issue that has been tagged as a bounty. Tracks the full lifecycle from open → assigned → merged → paid.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | |
| `repo_id` | UUID | FK → repositories.id, NOT NULL | |
| `reward_level_id` | UUID | FK → reward_levels.id, nullable | Null if maintainer sets custom amount |
| `github_issue_id` | BIGINT | UNIQUE NOT NULL | GitHub's global issue node ID |
| `github_issue_number` | INT | NOT NULL | Repo-scoped issue number, needed for GH API calls |
| `status` | TEXT | NOT NULL, DEFAULT 'open' | See status lifecycle below |
| `assignee_id` | UUID | FK → profiles.id, nullable | Null until a contributor is assigned |
| `assigned_at` | TIMESTAMPTZ | nullable | When assignee was set |
| `merged_at` | TIMESTAMPTZ | nullable | When the linked PR was merged |
| `paid_at` | TIMESTAMPTZ | nullable | When payout worker completed transfer |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

**Status lifecycle:**
```
open → assigned → merged → paid
 └──────────────────────→ cancelled
```

**Unique constraint:** `(repo_id, github_issue_number)`

```sql
CREATE TABLE bounties (
  id                   UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  repo_id              UUID        NOT NULL REFERENCES repositories(id)  ON DELETE CASCADE,
  reward_level_id      UUID        REFERENCES reward_levels(id)          ON DELETE SET NULL,
  github_issue_id      BIGINT      UNIQUE NOT NULL,
  github_issue_number  INT         NOT NULL,
  status               TEXT        NOT NULL DEFAULT 'open',
  assignee_id          UUID        REFERENCES profiles(id)               ON DELETE SET NULL,
  assigned_at          TIMESTAMPTZ,
  merged_at            TIMESTAMPTZ,
  paid_at              TIMESTAMPTZ,
  created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (repo_id, github_issue_number),
  CONSTRAINT status_check CHECK (status IN ('open','assigned','merged','paid','cancelled'))
);
```

---

## 7. `notifications`

In-app notifications for any user. Append-only — never updated except flipping `is_read`.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | |
| `profile_id` | UUID | FK → profiles.id, NOT NULL | Recipient |
| `kind` | TEXT | NOT NULL | `assigned` `paid` `bounty_posted` `mention` `payout_failed` |
| `title` | TEXT | NOT NULL | Short one-line summary |
| `body` | TEXT | nullable | Optional detail |
| `ref_id` | UUID | nullable | Link to relevant bounty / repo |
| `is_read` | BOOLEAN | NOT NULL, DEFAULT false | |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

```sql
CREATE TABLE notifications (
  id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  profile_id  UUID        NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  kind        TEXT        NOT NULL,
  title       TEXT        NOT NULL,
  body        TEXT,
  ref_id      UUID,
  is_read     BOOLEAN     NOT NULL DEFAULT false,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

---

## 8. `failed_tasks`

Dead-letter store for background jobs (BullMQ) that exhausted retries. Ops team can inspect, fix, and re-queue from here.

| Column | Type | Constraints | Notes |
|---|---|---|---|
| `id` | UUID | PK, DEFAULT gen_random_uuid() | |
| `task_type` | TEXT | NOT NULL | `sync` `payout` `notify` `bounty` |
| `ref_id` | UUID | nullable | ID of the related entity (bounty, repo, etc.) |
| `payload` | JSONB | NOT NULL | Original job payload |
| `error` | TEXT | nullable | Last error message |
| `attempts` | INT | NOT NULL, DEFAULT 1 | How many times it was tried |
| `status` | TEXT | NOT NULL, DEFAULT 'pending' | `pending` `resolved` `dead` |
| `last_attempt_at` | TIMESTAMPTZ | nullable | |
| `created_at` | TIMESTAMPTZ | NOT NULL, DEFAULT now() | |

```sql
CREATE TABLE failed_tasks (
  id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  task_type        TEXT        NOT NULL,
  ref_id           UUID,
  payload          JSONB       NOT NULL,
  error            TEXT,
  attempts         INT         NOT NULL DEFAULT 1,
  status           TEXT        NOT NULL DEFAULT 'pending',
  last_attempt_at  TIMESTAMPTZ,
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  CONSTRAINT status_check CHECK (status IN ('pending', 'resolved', 'dead'))
);
```

---

## 9. `contributor_stats` (Materialized View)

Derived from `bounties` — never manually updated. Refresh after every payout event.

```sql
CREATE MATERIALIZED VIEW contributor_stats AS
SELECT
  p.id                                                        AS profile_id,
  p.username,
  COUNT(*)    FILTER (WHERE b.status = 'merged')              AS merged,
  COUNT(*)    FILTER (WHERE b.status = 'assigned')            AS assigned,
  COUNT(*)    FILTER (WHERE b.status = 'paid')                AS paid,
  COALESCE(SUM(rl.amount) FILTER (WHERE b.status = 'paid'), 0) AS earned
FROM profiles p
LEFT JOIN bounties      b  ON b.assignee_id     = p.id
LEFT JOIN reward_levels rl ON rl.id             = b.reward_level_id
GROUP BY p.id, p.username;

-- Refresh after payout worker completes
-- REFRESH MATERIALIZED VIEW CONCURRENTLY contributor_stats;

CREATE UNIQUE INDEX ON contributor_stats(profile_id);
```

---

## 10. Relationships Overview

```
profiles ──────────────────────────── wallets
   │  (1 profile → many wallets)
   │
   ├──── repo_maintainers ──────────── repositories
   │      (many-to-many)                    ├──── reward_levels
   │                                        │         │
   │                                        └──── bounties ←── assignee (profiles)
   │
   └──── notifications


failed_tasks          (standalone — no FKs, ref_id is untyped UUID)
contributor_stats     (materialized view — reads from bounties + reward_levels + profiles)
```

---

## 11. Indexes

```sql
-- profiles
CREATE INDEX idx_profiles_github_id  ON profiles(github_id);

-- wallets
CREATE INDEX idx_wallets_profile     ON wallets(profile_id);

-- repositories
CREATE INDEX idx_repos_github_id     ON repositories(github_repo_id);

-- repo_maintainers
CREATE INDEX idx_rm_profile          ON repo_maintainers(profile_id);

-- reward_levels
CREATE INDEX idx_rewards_repo        ON reward_levels(repo_id);

-- bounties
CREATE INDEX idx_bounties_repo       ON bounties(repo_id);
CREATE INDEX idx_bounties_assignee   ON bounties(assignee_id) WHERE assignee_id IS NOT NULL;
CREATE INDEX idx_bounties_status     ON bounties(status);

-- notifications
CREATE INDEX idx_notifs_profile      ON notifications(profile_id, created_at DESC);
CREATE INDEX idx_notifs_unread       ON notifications(profile_id) WHERE is_read = false;

-- failed_tasks
CREATE INDEX idx_failed_status       ON failed_tasks(status) WHERE status != 'resolved';
CREATE INDEX idx_failed_type         ON failed_tasks(task_type);
```
