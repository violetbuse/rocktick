-- Modify "tenants" table
ALTER TABLE "tenants" ADD COLUMN "max_retries" integer NOT NULL DEFAULT 3;
