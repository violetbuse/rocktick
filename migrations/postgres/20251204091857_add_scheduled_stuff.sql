-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "success" boolean NULL, ADD COLUMN "response_error" text NULL, ADD COLUMN "timeout_ms" integer NOT NULL, ADD COLUMN "max_retries" integer NOT NULL;
