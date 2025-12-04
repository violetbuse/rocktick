-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" DROP COLUMN "response", DROP COLUMN "success", DROP COLUMN "response_error";
-- Create "job_executions" table
CREATE TABLE "job_executions" (
  "id" character varying(255) NOT NULL,
  "job_id" character varying(255) NOT NULL,
  "success" boolean NULL,
  "response" character varying(255) NULL,
  "response_error" text NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "job_executions_job_id_fkey" FOREIGN KEY ("job_id") REFERENCES "scheduled_jobs" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "job_executions_response_fkey" FOREIGN KEY ("response") REFERENCES "http_responses" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION
);
