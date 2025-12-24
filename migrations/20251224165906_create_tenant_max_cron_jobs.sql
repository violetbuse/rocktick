-- Modify "tenants" table
ALTER TABLE "tenants" ADD COLUMN "max_cron_jobs" integer NOT NULL DEFAULT 1000;
