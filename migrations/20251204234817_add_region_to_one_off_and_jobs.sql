-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ADD COLUMN "region" text NOT NULL;
-- Modify "one_off_jobs" table
ALTER TABLE "one_off_jobs" ADD COLUMN "region" text NOT NULL;
