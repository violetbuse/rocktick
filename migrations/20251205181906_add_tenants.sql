-- Create "tenants" table
CREATE TABLE "tenants" (
  "id" character varying(255) NOT NULL,
  "tokens" integer NOT NULL,
  "increment" integer NOT NULL,
  "period" interval NOT NULL,
  "next_increment" timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY ("id")
);
-- Modify "cron_jobs" table
ALTER TABLE "cron_jobs" ADD COLUMN "tenant_id" character varying(255) NULL, ADD CONSTRAINT "cron_jobs_tenant_id_fkey" FOREIGN KEY ("tenant_id") REFERENCES "tenants" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
-- Modify "one_off_jobs" table
ALTER TABLE "one_off_jobs" ADD COLUMN "tenant_id" character varying(255) NULL, ADD CONSTRAINT "one_off_jobs_tenant_id_fkey" FOREIGN KEY ("tenant_id") REFERENCES "tenants" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ADD COLUMN "tenant_id" character varying(255) NULL, ADD CONSTRAINT "scheduled_jobs_tenant_id_fkey" FOREIGN KEY ("tenant_id") REFERENCES "tenants" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
