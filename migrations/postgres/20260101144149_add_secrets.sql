-- Create "secrets" table
CREATE TABLE "secrets" (
  "id" character varying(255) NOT NULL,
  "master_key_id" integer NOT NULL,
  "secret_version" integer NOT NULL,
  "encrypted_dek" bytea NOT NULL,
  "encrypted_data" bytea NOT NULL,
  "dek_nonce" bytea NOT NULL,
  "data_nonce" bytea NOT NULL,
  "algorithm" text NOT NULL,
  PRIMARY KEY ("id")
);
-- Modify "tenants" table
ALTER TABLE "tenants" ADD CONSTRAINT "both_signing_keys_or_just_one" CHECK (((current_signing_key IS NULL) AND (next_signing_key IS NULL)) OR ((current_signing_key IS NOT NULL) AND (next_signing_key IS NOT NULL))), ADD COLUMN "current_signing_key" character varying(255) NULL, ADD COLUMN "next_signing_key" character varying(255) NULL, ADD CONSTRAINT "tenants_current_signing_key_fkey" FOREIGN KEY ("current_signing_key") REFERENCES "secrets" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION, ADD CONSTRAINT "tenants_next_signing_key_fkey" FOREIGN KEY ("next_signing_key") REFERENCES "secrets" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION;
