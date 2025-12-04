-- Rename a column from "response" to "response_id"
ALTER TABLE "job_executions" RENAME COLUMN "response" TO "response_id";
-- Modify "job_executions" table
ALTER TABLE "job_executions" DROP CONSTRAINT "job_executions_response_fkey", DROP COLUMN "job_id", ADD CONSTRAINT "job_executions_response_id_key" UNIQUE ("response_id"), ADD CONSTRAINT "job_executions_response_id_fkey" FOREIGN KEY ("response_id") REFERENCES "http_responses" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
-- Rename a column from "request" to "request_id"
ALTER TABLE "scheduled_jobs" RENAME COLUMN "request" TO "request_id";
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" DROP CONSTRAINT "scheduled_jobs_request_fkey", ADD COLUMN "execution_id" character varying(255) NOT NULL, ADD CONSTRAINT "scheduled_jobs_execution_id_key" UNIQUE ("execution_id"), ADD CONSTRAINT "scheduled_jobs_request_id_key" UNIQUE ("request_id"), ADD CONSTRAINT "scheduled_jobs_execution_id_fkey" FOREIGN KEY ("execution_id") REFERENCES "job_executions" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION, ADD CONSTRAINT "scheduled_jobs_request_id_fkey" FOREIGN KEY ("request_id") REFERENCES "http_requests" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
