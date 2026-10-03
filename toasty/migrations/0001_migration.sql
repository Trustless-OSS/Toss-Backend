ALTER TABLE "notifications" ADD COLUMN "email_status" TEXT NOT NULL;
ALTER TABLE "notifications" ADD COLUMN "email_sent_at" TIMESTAMPTZ(6);
ALTER TABLE "notifications" ADD COLUMN "data" JSONB;
ALTER TABLE "notifications" ADD COLUMN "dedupe_key" TEXT NOT NULL;
CREATE UNIQUE INDEX "index_notifications_by_dedupe_key" ON "notifications" ("dedupe_key");
