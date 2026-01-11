-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" DROP COLUMN "workflow_id";
-- Drop "workflow_dependencies" table
DROP TABLE "workflow_dependencies";
-- Drop "workflows" table
DROP TABLE "workflows";
-- Drop enum type "workflow_status"
DROP TYPE "workflow_status";
