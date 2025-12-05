-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ALTER COLUMN "created_at" TYPE timestamptz, ALTER COLUMN "start_at" TYPE timestamptz;
-- Modify "job_executions" table
ALTER TABLE "job_executions" ALTER COLUMN "executed_at" TYPE timestamptz;
-- Modify "one_off_jobs" table
ALTER TABLE "one_off_jobs" ALTER COLUMN "created_at" TYPE timestamptz;
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ALTER COLUMN "scheduled_at" TYPE timestamptz;
