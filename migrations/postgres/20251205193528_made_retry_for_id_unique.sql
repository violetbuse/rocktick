-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD CONSTRAINT "scheduled_jobs_retry_for_id_key" UNIQUE ("retry_for_id");
