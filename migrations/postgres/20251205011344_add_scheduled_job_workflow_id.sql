-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "workflow_id" character varying(255) NULL;
