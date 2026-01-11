-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ADD COLUMN "created_at" timestamp NOT NULL DEFAULT now();
-- Modify "one_off_jobs" table
ALTER TABLE "one_off_jobs" ADD COLUMN "created_at" timestamp NOT NULL DEFAULT now();
