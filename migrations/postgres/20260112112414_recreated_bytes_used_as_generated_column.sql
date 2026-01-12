-- Modify "http_requests" table
ALTER TABLE "http_requests" ADD COLUMN "bytes_used" integer NOT NULL GENERATED ALWAYS AS (COALESCE(octet_length(body), 0)) STORED;
-- Modify "http_responses" table
ALTER TABLE "http_responses" ADD COLUMN "bytes_used" integer NOT NULL GENERATED ALWAYS AS (COALESCE(octet_length(body), 0)) STORED;
