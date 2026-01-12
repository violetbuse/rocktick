-- Modify "http_requests" table
ALTER TABLE "http_requests" ADD COLUMN "deleted_at" timestamptz NULL;
-- Modify "http_responses" table
ALTER TABLE "http_responses" ADD COLUMN "deleted_at" timestamptz NULL;
