-- Modify "workflow_executions" table
ALTER TABLE "workflow_executions" ALTER COLUMN "execution_index" DROP DEFAULT, ADD COLUMN "is_retry" boolean NOT NULL;
-- Modify "workflows" table
ALTER TABLE "workflows" DROP CONSTRAINT "workflows_check2", ADD CONSTRAINT "workflows_retry_count_check" CHECK (retry_count >= 0);
