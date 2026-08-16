CREATE TABLE "repos" (
    "id" UUID NOT NULL,
    "github_repo_id" BIGINT NOT NULL,
    "github_installation_id" BIGINT,
    "full_name" TEXT NOT NULL,
    "owner_github_id" BIGINT NOT NULL,
    "owner_username" TEXT NOT NULL,
    "owner_type" TEXT,
    "installer_github_id" BIGINT,
    "is_fork" BOOLEAN NOT NULL,
    "is_private" BOOLEAN NOT NULL,
    "escrow_contract_id" TEXT,
    "escrow_funder_wallet" TEXT,
    "escrow_balance" NUMERIC NOT NULL,
    "reward_low" NUMERIC NOT NULL,
    "reward_medium" NUMERIC NOT NULL,
    "reward_high" NUMERIC NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_repos_by_github_repo_id" ON "repos" ("github_repo_id");
CREATE INDEX "index_repos_by_github_installation_id" ON "repos" ("github_installation_id");
CREATE INDEX "index_repos_by_owner_username" ON "repos" ("owner_username");
CREATE TABLE "issues" (
    "id" UUID NOT NULL,
    "repo_id" UUID NOT NULL,
    "github_issue_id" BIGINT NOT NULL,
    "github_issue_number" INTEGER NOT NULL,
    "title" TEXT NOT NULL,
    "reward_amount" NUMERIC NOT NULL,
    "difficulty_label" TEXT,
    "milestone_index" INTEGER,
    "status" TEXT NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_issues_by_repo_id_and_github_issue_id" ON "issues" ("repo_id", "github_issue_id");
CREATE INDEX "index_issues_by_repo_id" ON "issues" ("repo_id");
CREATE INDEX "index_issues_by_github_issue_number" ON "issues" ("github_issue_number");
CREATE TABLE "contributors" (
    "id" UUID NOT NULL,
    "github_user_id" BIGINT NOT NULL,
    "github_username" TEXT NOT NULL,
    "stellar_wallet" TEXT,
    "payout_chain" TEXT NOT NULL,
    "payout_address" TEXT,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_contributors_by_github_user_id" ON "contributors" ("github_user_id");
CREATE TABLE "assignments" (
    "id" UUID NOT NULL,
    "issue_id" UUID NOT NULL,
    "contributor_id" UUID,
    "assigned_at" TIMESTAMPTZ(6),
    "pr_number" INTEGER,
    "pr_merged_at" TIMESTAMPTZ(6),
    "payout_status" TEXT NOT NULL,
    "completion_percentage" NUMERIC,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_assignments_by_issue_id" ON "assignments" ("issue_id");
CREATE INDEX "index_assignments_by_contributor_id" ON "assignments" ("contributor_id");
