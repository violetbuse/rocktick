-- Modify "http_requests" table
ALTER TABLE "http_requests" ADD COLUMN "bytes_used" integer NOT NULL DEFAULT 0;
-- Modify "http_responses" table
ALTER TABLE "http_responses" ADD COLUMN "bytes_used" integer NOT NULL DEFAULT 0;
-- Modify "job_executions" table
ALTER TABLE "job_executions" DROP COLUMN "response_json";
