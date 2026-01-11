-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "max_response_bytes" integer NOT NULL DEFAULT 100;
