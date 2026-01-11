-- Modify "workflow_executions" table
ALTER TABLE "workflow_executions" DROP CONSTRAINT "workflow_executions_check", ADD CONSTRAINT "workflow_executions_check" CHECK ((result_json IS NULL) OR ((result_json IS NOT NULL) AND ((status = 'completed'::text) OR (status = 'failed'::text))));
