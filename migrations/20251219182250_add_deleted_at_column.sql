-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ADD COLUMN "deleted_at" timestamptz NULL;
-- Modify "one_off_jobs" table
ALTER TABLE "one_off_jobs" ADD COLUMN "deleted_at" timestamptz NULL;
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "deleted_at" timestamptz NULL;
