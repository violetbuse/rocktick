-- Create "executions" table
CREATE TABLE `executions` (
  `job_id` text NOT NULL,
  `success` integer NOT NULL,
  `lock_nonce` integer NOT NULL,
  `response_id` text NULL,
  `response_error` text NULL,
  `req_method` text NOT NULL,
  `req_url` text NOT NULL,
  `req_header_map` text NOT NULL,
  `req_body` text NULL,
  `req_body_bytes_used` integer NOT NULL,
  `executed_at` integer NOT NULL,
  `is_local` integer NOT NULL,
  `replicated_times` integer NOT NULL,
  PRIMARY KEY (`job_id`),
  CONSTRAINT `0` FOREIGN KEY (`response_id`) REFERENCES `execution_responses` (`id`) ON UPDATE NO ACTION ON DELETE NO ACTION,
  CHECK (success IN (0, 1)),
  CHECK (
    json_valid(req_header_map) AND
    json_type(req_header_map) = 'object'
  ),
  CHECK (is_local IN (0, 1)),
  CONSTRAINT `one_of_response_id_or_response_error` CHECK (
    (response_id IS NOT NULL AND response_error IS NULL) OR
    (response_id IS NULL AND response_error IS NOT NULL)
  )
) STRICT;
-- Create "execution_responses" table
CREATE TABLE `execution_responses` (
  `id` text NOT NULL,
  `status` integer NOT NULL,
  `header_map` text NOT NULL,
  `body` text NOT NULL,
  `bytes_used` integer NOT NULL,
  PRIMARY KEY (`id`),
  CHECK (
    json_valid(header_map) AND
    json_type(header_map) = 'object'
  )
) STRICT;
