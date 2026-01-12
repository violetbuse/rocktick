-- Modify "http_requests" table
ALTER TABLE "http_requests" DROP COLUMN "bytes_used";
-- Modify "http_responses" table
ALTER TABLE "http_responses" DROP COLUMN "bytes_used";
