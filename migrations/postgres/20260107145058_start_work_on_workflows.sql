-- Create enum type "workflow_status"
CREATE TYPE "workflow_status" AS ENUM ('waiting', 'completed', 'failed');
-- Modify "job_executions" table
ALTER TABLE "job_executions" ADD COLUMN "response_json" jsonb NULL;
-- Create "workflows" table
CREATE TABLE "workflows" (
  "id" character varying(255) NOT NULL,
  "region" text NOT NULL,
  "tenant_id" character varying(255) NULL,
  "status" "workflow_status" NOT NULL,
  "input_data" jsonb NOT NULL,
  "workflow_state" jsonb NOT NULL,
  "result" jsonb NULL,
  "error" text NULL,
  "deleted_at" timestamptz NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "workflows_tenant_id_fkey" FOREIGN KEY ("tenant_id") REFERENCES "tenants" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION
);
-- Create "workflow_dependencies" table
CREATE TABLE "workflow_dependencies" (
  "parent_workflow_id" character varying(255) NOT NULL,
  "child_workflow_id" character varying(255) NOT NULL,
  PRIMARY KEY ("parent_workflow_id", "child_workflow_id"),
  CONSTRAINT "workflow_dependencies_child_workflow_id_fkey" FOREIGN KEY ("child_workflow_id") REFERENCES "workflows" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "workflow_dependencies_parent_workflow_id_fkey" FOREIGN KEY ("parent_workflow_id") REFERENCES "workflows" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION
);
