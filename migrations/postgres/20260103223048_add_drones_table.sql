-- Create "drones" table
CREATE TABLE "drones" (
  "id" character varying(255) NOT NULL,
  "ip" inet NOT NULL,
  "region" text NOT NULL,
  "last_checkin" timestamptz NOT NULL,
  "checkin_by" timestamptz NOT NULL,
  PRIMARY KEY ("id"),
  CONSTRAINT "checkin_by_after_last_checkin" CHECK (checkin_by > last_checkin)
);
