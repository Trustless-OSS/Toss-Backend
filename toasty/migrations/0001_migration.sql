ALTER TABLE "notifications" ADD COLUMN IF NOT EXISTS "email_status" TEXT NOT NULL DEFAULT 'not_requested';
ALTER TABLE "notifications" ADD COLUMN IF NOT EXISTS "email_sent_at" TIMESTAMPTZ(6);
ALTER TABLE "notifications" ADD COLUMN IF NOT EXISTS "data" JSONB;
ALTER TABLE "notifications" ADD COLUMN IF NOT EXISTS "dedupe_key" TEXT NOT NULL DEFAULT '';
CREATE UNIQUE INDEX IF NOT EXISTS "index_notifications_by_dedupe_key" ON "notifications" ("dedupe_key");
