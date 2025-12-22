-- Modify "tenants" table
ALTER TABLE "tenants" ADD COLUMN "max_delay_days" integer NOT NULL DEFAULT 31;
