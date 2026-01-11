-- Create "http_requests" table
CREATE TABLE "http_requests" (
  "id" character varying(255) NOT NULL,
  "method" character varying(10) NOT NULL,
  "headers" text[] NOT NULL,
  "body" text NULL,
  PRIMARY KEY ("id")
);
-- Create "http_responses" table
CREATE TABLE "http_responses" (
  "id" character varying(255) NOT NULL,
  "status" integer NOT NULL,
  "headers" text[] NOT NULL,
  "body" text NULL,
  PRIMARY KEY ("id")
);
-- Create "scheduled_jobs" table
CREATE TABLE "scheduled_jobs" (
  "id" character varying(255) NOT NULL,
  "hash" integer NOT NULL,
  "lock_nonce" integer NULL,
  "region" text NOT NULL,
  "request" character varying(255) NOT NULL,
  "response" character varying(255) NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "scheduled_jobs_request_fkey" FOREIGN KEY ("request") REFERENCES "http_requests" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION,
  CONSTRAINT "scheduled_jobs_response_fkey" FOREIGN KEY ("response") REFERENCES "http_responses" ("id") ON UPDATE NO ACTION ON DELETE NO ACTION
);
