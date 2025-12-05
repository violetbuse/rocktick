-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ADD COLUMN "start_at" timestamp NOT NULL DEFAULT now();
