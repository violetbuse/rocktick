-- Modify "scheduled_jobs" table
ALTER TABLE "scheduled_jobs" ALTER COLUMN "execution_id" DROP NOT NULL;
