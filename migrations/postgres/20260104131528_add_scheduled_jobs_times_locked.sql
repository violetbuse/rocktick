-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "times_locked" integer NOT NULL DEFAULT 0;
