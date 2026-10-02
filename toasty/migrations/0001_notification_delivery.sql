ALTER TABLE "notifications"
    ADD COLUMN IF NOT EXISTS "dedupe_key" TEXT,
    ADD COLUMN IF NOT EXISTS "email_status" TEXT NOT NULL DEFAULT 'not_requested',
    ADD COLUMN IF NOT EXISTS "email_sent_at" TIMESTAMPTZ(6);

UPDATE "notifications"
SET "dedupe_key" = 'legacy:' || "id"::TEXT
WHERE "dedupe_key" IS NULL;

ALTER TABLE "notifications"
    ALTER COLUMN "dedupe_key" SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS "index_notifications_by_dedupe_key"
    ON "notifications" ("dedupe_key");
