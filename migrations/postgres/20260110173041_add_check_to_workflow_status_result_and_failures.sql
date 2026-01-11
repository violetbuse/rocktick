-- Modify "workflow_executions" table
ALTER TABLE "workflow_executions" ADD CONSTRAINT "workflow_executions_check" CHECK ((result_json IS NULL) OR ((result_json IS NOT NULL) AND (status = 'completed'::text))), ADD CONSTRAINT "workflow_executions_check1" CHECK ((failure_reason IS NULL) OR ((failure_reason IS NOT NULL) AND (status = 'failed'::text)));
