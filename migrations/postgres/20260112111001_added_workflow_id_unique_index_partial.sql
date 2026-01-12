-- Create index "idx_single_active_execution" to table: "workflow_executions"
CREATE UNIQUE INDEX "idx_single_active_execution" ON "workflow_executions" ("workflow_id") WHERE (status <> ALL (ARRAY['completed'::text, 'failed'::text]));
