-- Create "workflows" table
CREATE TABLE "workflows" (
  "id" character varying(255) NOT NULL,
  "region" text NOT NULL,
  "tenant_id" character varying(255) NULL,
  "implementation_url" text NOT NULL,
  "input" jsonb NOT NULL,
  "context" jsonb NOT NULL,
  "status" text NOT NULL,
  "result" jsonb NULL,
  "error" text NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "workflows_tenant_id_fkey" FOREIGN KEY ("tenant_id") REFERENCES "tenants" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "workflows_check" CHECK ((result IS NULL) OR (status = 'completed'::text)),
  CONSTRAINT "workflows_check1" CHECK ((error IS NULL) OR (status = 'failed'::text)),
  CONSTRAINT "workflows_status_check" CHECK (status = ANY (ARRAY['pending'::text, 'completed'::text, 'failed'::text]))
);
-- Create "workflow_executions" table
CREATE TABLE "workflow_executions" (
  "id" character varying(255) NOT NULL,
  "region" text NOT NULL,
  "workflow_id" character varying(255) NOT NULL,
  "execution_index" integer NOT NULL DEFAULT 0,
  "tenant_id" character varying(255) NULL,
  "status" text NOT NULL,
  "executed_at" timestamptz NULL,
  "result_json" jsonb NULL,
  "failure_reason" text NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "workflow_executions_workflow_id_execution_index_key" UNIQUE ("workflow_id", "execution_index"),
  CONSTRAINT "workflow_executions_tenant_id_fkey" FOREIGN KEY ("tenant_id") REFERENCES "tenants" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "workflow_executions_workflow_id_fkey" FOREIGN KEY ("workflow_id") REFERENCES "workflows" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "workflow_executions_status_check" CHECK (status = ANY (ARRAY['pending'::text, 'waiting'::text, 'scheduled'::text, 'completed'::text, 'failed'::text]))
);
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD CONSTRAINT "workflow_and_workflow_execution" CHECK (((workflow_id IS NULL) AND (workflow_execution_id IS NULL)) OR ((workflow_id IS NOT NULL) AND (workflow_execution_id IS NOT NULL))), ADD COLUMN "workflow_id" character varying(255) NULL, ADD COLUMN "workflow_execution_id" character varying(255) NULL, ADD CONSTRAINT "scheduled_jobs_workflow_execution_id_fkey" FOREIGN KEY ("workflow_execution_id") REFERENCES "workflow_executions" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION, ADD CONSTRAINT "scheduled_jobs_workflow_id_fkey" FOREIGN KEY ("workflow_id") REFERENCES "workflows" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
-- Create "workflow_dependencies" table
CREATE TABLE "workflow_dependencies" (
  "id" character varying(255) NOT NULL,
  "workflow_execution_id" character varying(255) NOT NULL,
  "child_workflow_id" character varying(255) NULL,
  "wait_until" timestamptz NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "workflow_dependencies_child_workflow_id_fkey" FOREIGN KEY ("child_workflow_id") REFERENCES "workflows" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "workflow_dependencies_workflow_execution_id_fkey" FOREIGN KEY ("workflow_execution_id") REFERENCES "workflow_executions" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "child_workflow_or_wait_until" CHECK (((child_workflow_id IS NOT NULL) AND (wait_until IS NULL)) OR ((child_workflow_id IS NULL) AND (wait_until IS NOT NULL)))
);
