-- Modify "tenants" table
ALTER TABLE "tenants" ALTER COLUMN "max_timeout" DROP DEFAULT, ALTER COLUMN "default_retries" DROP DEFAULT, ALTER COLUMN "max_max_response_bytes" DROP DEFAULT;
