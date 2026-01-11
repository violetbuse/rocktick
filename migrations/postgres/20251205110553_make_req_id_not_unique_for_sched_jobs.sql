-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" DROP CONSTRAINT "scheduled_jobs_request_id_key";
-- Modify "job_executions" table
ALTER TABLE "job_executions" ADD COLUMN "request_id" character varying(255) NOT NULL, ADD CONSTRAINT "job_executions_request_id_key" UNIQUE ("request_id"), ADD CONSTRAINT "job_executions_request_id_fkey" FOREIGN KEY ("request_id") REFERENCES "http_requests" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
