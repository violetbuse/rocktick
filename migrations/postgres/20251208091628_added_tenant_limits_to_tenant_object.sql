-- Modify "tenants" table
ALTER TABLE "tenants" ADD COLUMN "max_timeout" integer NOT NULL DEFAULT 10000, ADD COLUMN "default_retries" integer NOT NULL DEFAULT 3, ADD COLUMN "max_max_response_bytes" integer NOT NULL DEFAULT 4194304;
