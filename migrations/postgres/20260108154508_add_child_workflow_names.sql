-- Modify "workflow_dependencies" table
ALTER TABLE "workflow_dependencies" ADD CONSTRAINT "child_workflow_and_name" CHECK (((child_workflow_id IS NOT NULL) AND (child_workflow_name IS NOT NULL)) OR ((child_workflow_id IS NULL) AND (child_workflow_name IS NULL))), ADD COLUMN "child_workflow_name" text NULL;
