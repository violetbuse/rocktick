-- Disable the enforcement of foreign-keys constraints
PRAGMA foreign_keys = off;
-- Create "new_execution_responses" table
CREATE TABLE `new_execution_responses` (
  `id` text NOT NULL,
  `status` integer NOT NULL,
  `header_map` text NOT NULL,
  `body` text NOT NULL,
  PRIMARY KEY (`id`),
  CHECK (
    json_valid(header_map) AND
    json_type(header_map) = 'object'
  )
) STRICT;
-- Copy rows from old table "execution_responses" to new temporary table "new_execution_responses"
INSERT INTO `new_execution_responses` (`id`, `status`, `header_map`, `body`) SELECT `id`, `status`, `header_map`, `body` FROM `execution_responses`;
-- Drop "execution_responses" table after copying rows
DROP TABLE `execution_responses`;
-- Rename temporary table "new_execution_responses" to "execution_responses"
ALTER TABLE `new_execution_responses` RENAME TO `execution_responses`;
-- Create "new_executions" table
CREATE TABLE `new_executions` (
  `job_id` text NOT NULL,
  `success` integer NOT NULL,
  `lock_nonce` integer NOT NULL,
  `response_id` text NULL,
  `response_error` text NULL,
  `req_method` text NOT NULL,
  `req_url` text NOT NULL,
  `req_header_map` text NOT NULL,
  `req_body` text NULL,
  `executed_at` integer NOT NULL,
  `is_local` integer NOT NULL,
  `replicated_times` integer NOT NULL,
  `sync_status` text NOT NULL DEFAULT 'local',
  `sync_time` integer NULL,
  `sync_nonce` integer NULL,
  PRIMARY KEY (`job_id`),
  CONSTRAINT `0` FOREIGN KEY (`response_id`) REFERENCES `execution_responses` (`id`) ON UPDATE NO ACTION ON DELETE CASCADE,
  CHECK (success IN (0, 1)),
  CHECK (
    json_valid(req_header_map) AND
    json_type(req_header_map) = 'object'
  ),
  CHECK (is_local IN (0, 1)),
  CHECK (sync_status IN ('local', 'pending', 'synced')),
  CONSTRAINT `one_of_response_id_or_response_error` CHECK (
    (response_id IS NOT NULL AND response_error IS NULL) OR
    (response_id IS NULL AND response_error IS NOT NULL)
  )
) STRICT;
-- Copy rows from old table "executions" to new temporary table "new_executions"
INSERT INTO `new_executions` (`job_id`, `success`, `lock_nonce`, `response_id`, `response_error`, `req_method`, `req_url`, `req_header_map`, `req_body`, `executed_at`, `is_local`, `replicated_times`, `sync_status`, `sync_time`, `sync_nonce`) SELECT `job_id`, `success`, `lock_nonce`, `response_id`, `response_error`, `req_method`, `req_url`, `req_header_map`, `req_body`, `executed_at`, `is_local`, `replicated_times`, `sync_status`, `sync_time`, `sync_nonce` FROM `executions`;
-- Drop "executions" table after copying rows
DROP TABLE `executions`;
-- Rename temporary table "new_executions" to "executions"
ALTER TABLE `new_executions` RENAME TO `executions`;
-- Enable back the enforcement of foreign-keys constraints
PRAGMA foreign_keys = on;
