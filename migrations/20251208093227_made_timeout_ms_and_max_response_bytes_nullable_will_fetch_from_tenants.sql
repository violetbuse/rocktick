-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ALTER COLUMN "timeout_ms" DROP NOT NULL, ALTER COLUMN "max_response_bytes" DROP NOT NULL;
-- Modify "one_off_jobs" table
ALTER TABLE "one_off_jobs" ALTER COLUMN "timeout_ms" DROP NOT NULL, ALTER COLUMN "max_response_bytes" DROP NOT NULL;
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ALTER COLUMN "timeout_ms" DROP NOT NULL, ALTER COLUMN "max_response_bytes" DROP NOT NULL;
