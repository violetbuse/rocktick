-- Create "one_off_jobs" table
CREATE TABLE "one_off_jobs" (
  "id" character varying(255) NOT NULL,
  "request_id" character varying(255) NOT NULL,
  "execute_at" bigint NOT NULL,
  "timeout_ms" integer NOT NULL,
  "max_retries" integer NOT NULL,
  "max_response_bytes" integer NOT NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "one_off_jobs_request_id_key" UNIQUE ("request_id"),
  CONSTRAINT "one_off_jobs_request_id_fkey" FOREIGN KEY ("request_id") REFERENCES "http_requests" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION
);
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "one_off_job_id" character varying(255) NULL, ADD CONSTRAINT "scheduled_jobs_one_off_job_id_fkey" FOREIGN KEY ("one_off_job_id") REFERENCES "one_off_jobs" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
