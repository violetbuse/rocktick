-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "workflow_id" character varying(255) NULL, ADD CONSTRAINT "scheduled_jobs_workflow_id_fkey" FOREIGN KEY ("workflow_id") REFERENCES "workflows" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
