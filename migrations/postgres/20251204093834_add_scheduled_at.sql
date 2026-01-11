-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "scheduled_at" timestamp NOT NULL;
