-- Modify "http_requests" table
ALTER TABLE "http_requests" ALTER COLUMN "bytes_used" DROP DEFAULT;
-- Modify "http_responses" table
ALTER TABLE "http_responses" ALTER COLUMN "bytes_used" DROP DEFAULT;
