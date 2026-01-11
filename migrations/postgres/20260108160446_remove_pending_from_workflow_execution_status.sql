-- Modify "workflow_executions" table
ALTER TABLE "workflow_executions" DROP CONSTRAINT "workflow_executions_status_check", ADD CONSTRAINT "workflow_executions_status_check" CHECK (status = ANY (ARRAY['waiting'::text, 'scheduled'::text, 'completed'::text, 'failed'::text]));
