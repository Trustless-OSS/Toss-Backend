CREATE TABLE "repo_maintainers" (
    "repo_id" UUID NOT NULL,
    "profile_id" UUID NOT NULL,
    "role" TEXT NOT NULL,
    "added_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("repo_id", "profile_id")
);
CREATE INDEX "index_repo_maintainers_by_profile_id" ON "repo_maintainers" ("profile_id");
CREATE TABLE "failed_tasks" (
    "id" UUID NOT NULL,
    "task_type" TEXT NOT NULL,
    "ref_id" UUID,
    "payload" JSONB NOT NULL,
    "error" TEXT,
    "attempts" INTEGER NOT NULL,
    "status" TEXT NOT NULL,
    "last_attempt_at" TIMESTAMPTZ(6),
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE INDEX "index_failed_tasks_by_task_type" ON "failed_tasks" ("task_type");
CREATE INDEX "index_failed_tasks_by_status" ON "failed_tasks" ("status");
CREATE TABLE "escrow_funders" (
    "id" UUID NOT NULL,
    "repo_id" UUID NOT NULL,
    "wallet_address" TEXT NOT NULL,
    "chain" TEXT NOT NULL,
    "amount" NUMERIC NOT NULL,
    "tx_hash" TEXT NOT NULL,
    "funded_at" TIMESTAMPTZ(6) NOT NULL,
    "profile_id" UUID,
    PRIMARY KEY ("id")
);
CREATE INDEX "index_escrow_funders_by_repo_id" ON "escrow_funders" ("repo_id");
CREATE UNIQUE INDEX "index_escrow_funders_by_tx_hash" ON "escrow_funders" ("tx_hash");
CREATE INDEX "index_escrow_funders_by_profile_id" ON "escrow_funders" ("profile_id");
CREATE TABLE "profiles" (
    "id" UUID NOT NULL,
    "github_id" BIGINT NOT NULL,
    "username" TEXT NOT NULL,
    "full_name" TEXT,
    "email" TEXT,
    "avatar_url" TEXT,
    "bio" TEXT,
    "location" TEXT,
    "skills" TEXT[],
    "telegram" TEXT,
    "discord" TEXT,
    "twitter" TEXT,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    "updated_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_profiles_by_github_id" ON "profiles" ("github_id");
CREATE UNIQUE INDEX "index_profiles_by_username" ON "profiles" ("username");
CREATE TABLE "notifications" (
    "id" UUID NOT NULL,
    "profile_id" UUID NOT NULL,
    "kind" TEXT NOT NULL,
    "title" TEXT NOT NULL,
    "body" TEXT,
    "ref_id" UUID,
    "is_read" BOOLEAN NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE INDEX "index_notifications_by_profile_id" ON "notifications" ("profile_id");
CREATE TABLE "activity" (
    "id" UUID NOT NULL,
    "repo_id" UUID,
    "actor_id" UUID,
    "event_type" TEXT NOT NULL,
    "payload" JSONB,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE INDEX "index_activity_by_repo_id" ON "activity" ("repo_id");
CREATE INDEX "index_activity_by_actor_id" ON "activity" ("actor_id");
CREATE TABLE "repositories" (
    "id" UUID NOT NULL,
    "github_repo_id" BIGINT NOT NULL,
    "github_install_id" BIGINT,
    "full_name" TEXT NOT NULL,
    "escrow_contract_id" TEXT,
    "escrow_balance" NUMERIC,
    "balance_synced_at" TIMESTAMPTZ(6),
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_repositories_by_github_repo_id" ON "repositories" ("github_repo_id");
CREATE UNIQUE INDEX "index_repositories_by_full_name" ON "repositories" ("full_name");
CREATE TABLE "reward_levels" (
    "id" UUID NOT NULL,
    "repo_id" UUID NOT NULL,
    "label" TEXT NOT NULL,
    "amount" NUMERIC NOT NULL,
    "updated_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_reward_levels_by_repo_id_and_label" ON "reward_levels" ("repo_id", "label");
CREATE INDEX "index_reward_levels_by_repo_id" ON "reward_levels" ("repo_id");
CREATE TABLE "bounties" (
    "id" UUID NOT NULL,
    "repo_id" UUID NOT NULL,
    "reward_level_id" UUID,
    "milestone_index" INTEGER,
    "github_issue_id" BIGINT NOT NULL,
    "github_issue_number" INTEGER NOT NULL,
    "title" TEXT,
    "reward_amount" NUMERIC,
    "status" TEXT NOT NULL,
    "assignee_id" UUID,
    "assigned_at" TIMESTAMPTZ(6),
    "merged_at" TIMESTAMPTZ(6),
    "paid_at" TIMESTAMPTZ(6),
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_bounties_by_repo_id_and_github_issue_number" ON "bounties" ("repo_id", "github_issue_number");
CREATE INDEX "index_bounties_by_repo_id" ON "bounties" ("repo_id");
CREATE UNIQUE INDEX "index_bounties_by_github_issue_id" ON "bounties" ("github_issue_id");
CREATE INDEX "index_bounties_by_github_issue_number" ON "bounties" ("github_issue_number");
CREATE INDEX "index_bounties_by_status" ON "bounties" ("status");
CREATE INDEX "index_bounties_by_assignee_id" ON "bounties" ("assignee_id");
CREATE TABLE "wallets" (
    "id" UUID NOT NULL,
    "profile_id" UUID NOT NULL,
    "chain" TEXT NOT NULL,
    "address" TEXT NOT NULL,
    "is_primary" BOOLEAN NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    "updated_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_wallets_by_profile_id_and_chain_and_address" ON "wallets" ("profile_id", "chain", "address");
CREATE INDEX "index_wallets_by_profile_id" ON "wallets" ("profile_id");
