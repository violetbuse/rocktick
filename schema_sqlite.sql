
CREATE TABLE executions (
  job_id TEXT NOT NULL PRIMARY KEY,
  success INTEGER NOT NULL CHECK (success IN (0, 1)),
  lock_nonce INTEGER NOT NULL,
  response_id TEXT REFERENCES execution_responses(id) ON DELETE CASCADE,
  response_error TEXT,
  req_method TEXT NOT NULL,
  req_url TEXT NOT NULL,
  req_header_map TEXT NOT NULL CHECK (
    json_valid(req_header_map) AND
    json_type(req_header_map) = 'object'
  ),
  req_body TEXT,
  req_body_bytes_used INTEGER NOT NULL,
  executed_at INTEGER NOT NULL,
  is_local INTEGER NOT NULL CHECK (is_local IN (0, 1)),
  replicated_times INTEGER NOT NULL,
  sync_status TEXT NOT NULL DEFAULT 'local'
    CHECK (sync_status IN ('local', 'pending', 'synced')),
  sync_time INTEGER,
  sync_nonce INTEGER,

  CONSTRAINT one_of_response_id_or_response_error CHECK (
    (response_id IS NOT NULL AND response_error IS NULL) OR
    (response_id IS NULL AND response_error IS NOT NULL)
  )
) STRICT;

CREATE TABLE execution_responses (
  id TEXT NOT NULL PRIMARY KEY,
  status INTEGER NOT NULL,
  header_map TEXT NOT NULL CHECK (
    json_valid(header_map) AND
    json_type(header_map) = 'object'
  ),
  body TEXT NOT NULL,
  bytes_used INTEGER NOT NULL
) STRICT;
