ALTER TABLE "repos" DROP COLUMN "reward_low";
ALTER TABLE "repos" DROP COLUMN "reward_medium";
ALTER TABLE "repos" DROP COLUMN "reward_high";
CREATE TABLE "rewards" (
    "id" UUID NOT NULL,
    "repo_id" UUID NOT NULL,
    "label" TEXT NOT NULL,
    "amount" NUMERIC NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_rewards_by_repo_id_and_label" ON "rewards" ("repo_id", "label");
CREATE INDEX "index_rewards_by_repo_id" ON "rewards" ("repo_id");
CREATE INDEX "index_rewards_by_label" ON "rewards" ("label");
