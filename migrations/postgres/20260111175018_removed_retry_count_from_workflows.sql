-- Modify "workflows" table
ALTER TABLE "workflows" DROP CONSTRAINT "workflows_retry_count_check", DROP COLUMN "retry_count";
