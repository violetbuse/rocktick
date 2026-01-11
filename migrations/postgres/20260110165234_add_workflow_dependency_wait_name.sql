-- Modify "workflow_dependencies" table
ALTER TABLE "workflow_dependencies" ADD CONSTRAINT "wait_and_name" CHECK (((wait_name IS NOT NULL) AND (wait_until IS NOT NULL)) OR ((wait_name IS NULL) AND (wait_until IS NULL))), ADD COLUMN "wait_name" text NULL;
