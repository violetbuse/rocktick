-- Create "cron_jobs" table
CREATE TABLE "cron_jobs" (
  "id" character varying(255) NOT NULL,
  "request_id" character varying(255) NOT NULL,
  "schedule" text NOT NULL,
  "timeout_ms" integer NOT NULL,
  "max_retries" integer NOT NULL,
  "max_response_bytes" integer NOT NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "cron_jobs_request_id_key" UNIQUE ("request_id"),
  CONSTRAINT "cron_jobs_request_id_fkey" FOREIGN KEY ("request_id") REFERENCES "http_requests" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION
);
