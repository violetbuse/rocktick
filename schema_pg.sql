
CREATE TABLE drones (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  ip inet NOT NULL,
  port INTEGER NOT NULL,
  region TEXT NOT NULL,
  last_checkin TIMESTAMPTZ NOT NULL,
  checkin_by TIMESTAMPTZ NOT NULL,
  CONSTRAINT checkin_by_after_last_checkin CHECK (checkin_by > last_checkin)
);

CREATE TABLE secrets (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  master_key_id INTEGER NOT NULL,
  secret_version INTEGER NOT NULL,
  encrypted_dek BYTEA NOT NULL,
  encrypted_data BYTEA NOT NULL,
  dek_nonce BYTEA NOT NULL,
  data_nonce BYTEA NOT NULL,
  algorithm TEXT NOT NULL
);

CREATE TABLE tenants (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  tokens INTEGER NOT NULL,
  max_tokens INTEGER NOT NULL,
  increment INTEGER NOT NULL,
  period INTERVAL NOT NULL,
  next_increment TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  max_timeout INTEGER NOT NULL,
  default_retries INTEGER NOT NULL,
  max_retries INTEGER NOT NULL,
  max_max_response_bytes INTEGER NOT NULL,
  max_request_bytes INTEGER NOT NULL,
  retain_for_days INTEGER NOT NULL,
  max_delay_days INTEGER NOT NULL,
  max_cron_jobs INTEGER NOT NULL,
  current_signing_key VARCHAR(255) REFERENCES secrets(id),
  next_signing_key VARCHAR(255) REFERENCES secrets(id),
  CONSTRAINT both_signing_keys_or_just_one CHECK (
    (current_signing_key IS NULL AND next_signing_key IS NULL) OR
    (current_signing_key IS NOT NULL AND next_signing_key IS NOT NULL)
  )
);

CREATE TABLE http_requests (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  method VARCHAR(10) NOT NULL,
  url TEXT NOT NULL,
  headers TEXT[] NOT NULL,
  body TEXT,
  bytes_used INTEGER NOT NULL GENERATED ALWAYS AS (
    COALESCE(octet_length(body), 0)
  ) STORED,
  deleted_at TIMESTAMPTZ
);

CREATE TABLE http_responses (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  status INTEGER NOT NULL,
  headers TEXT[] NOT NULL,
  body TEXT NOT NULL,
  bytes_used INTEGER NOT NULL GENERATED ALWAYS AS (
    COALESCE(octet_length(body), 0)
  ) STORED,
  deleted_at TIMESTAMPTZ
);

CREATE TABLE one_off_jobs (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  region TEXT NOT NULL,
  tenant_id VARCHAR(255) REFERENCES tenants(id),
  request_id VARCHAR(255) NOT NULL UNIQUE REFERENCES http_requests(id),
  execute_at BIGINT NOT NULL,
  timeout_ms INTEGER,
  max_retries INTEGER NOT NULL,
  max_response_bytes INTEGER,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deleted_at TIMESTAMPTZ
);

CREATE TABLE cron_jobs (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  region TEXT NOT NULL,
  tenant_id VARCHAR(255) REFERENCES tenants(id),
  request_id VARCHAR(255) NOT NULL UNIQUE REFERENCES http_requests(id),
  schedule TEXT NOT NULL,
  timeout_ms INTEGER,
  max_retries INTEGER NOT NULL,
  max_response_bytes INTEGER,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  start_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  error TEXT,
  deleted_at TIMESTAMPTZ
);

CREATE TABLE workflows (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  region TEXT NOT NULL,
  tenant_id VARCHAR(255) REFERENCES tenants(id),
  implementation_url TEXT NOT NULL,
  input JSONB NOT NULL,
  context JSONB NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'completed', 'failed')),
  result JSONB CHECK (result IS NULL OR (status = 'completed' AND result IS NOT NULL)),
  error TEXT CHECK (error IS NULL OR (status = 'failed' AND error IS NOT NULL)),
  max_retries INTEGER NOT NULL CHECK (max_retries >= 0)
);

CREATE TABLE workflow_executions (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  region TEXT NOT NULL,
  workflow_id VARCHAR(255) NOT NULL REFERENCES workflows(id),
  execution_index INTEGER NOT NULL,
  tenant_id VARCHAR(255) REFERENCES tenants(id),
  status TEXT NOT NULL CHECK (status IN ('pending', 'waiting', 'scheduled', 'completed', 'failed')),
  is_retry BOOLEAN NOT NULL,
  executed_at TIMESTAMPTZ,
  result_json JSONB
    CHECK (result_json IS NULL OR
      (result_json IS NOT NULL AND
      (status = 'completed' OR status = 'failed'))),
  failure_reason TEXT
    CHECK (failure_reason IS NULL OR
      (failure_reason IS NOT NULL AND status = 'failed')),
  UNIQUE (workflow_id, execution_index)
);

CREATE UNIQUE INDEX idx_single_active_execution
ON workflow_executions (workflow_id)
WHERE status NOT IN ('completed', 'failed');

CREATE TABLE workflow_dependencies (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  workflow_execution_id VARCHAR(255) NOT NULL REFERENCES workflow_executions(id),
  child_workflow_name TEXT,
  child_workflow_id VARCHAR(255) REFERENCES workflows(id),
  wait_name TEXT,
  wait_until TIMESTAMPTZ,
  CONSTRAINT child_workflow_or_wait_until CHECK (
    (child_workflow_id IS NOT NULL AND wait_until IS NULL) OR
    (child_workflow_id IS NULL AND wait_until IS NOT NULL)
  ),
  CONSTRAINT child_workflow_and_name CHECK (
    (child_workflow_id IS NOT NULL AND child_workflow_name IS NOT NULL) OR
    (child_workflow_id IS NULL AND child_workflow_name IS NULL)
  ),
  CONSTRAINT wait_and_name CHECK (
    (wait_name IS NOT NULL AND wait_until IS NOT NULL) OR
    (wait_name IS NULL AND wait_until IS NULL)
  )
);

CREATE TABLE job_executions (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  executed_at TIMESTAMPTZ NOT NULL,
  success BOOLEAN,
  request_id VARCHAR(255) NOT NULL UNIQUE REFERENCES http_requests(id),
  response_id VARCHAR(255) UNIQUE REFERENCES http_responses(id),
  response_error TEXT
);

CREATE TABLE scheduled_jobs (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  hash INTEGER NOT NULL,
  lock_nonce INTEGER,
  times_locked INTEGER NOT NULL DEFAULT 0,
  region TEXT NOT NULL,
  tenant_id VARCHAR(255) REFERENCES tenants(id),
  one_off_job_id VARCHAR(255) REFERENCES one_off_jobs(id),
  cron_job_id VARCHAR(255) REFERENCES cron_jobs(id),
  workflow_id VARCHAR(255) REFERENCES workflows(id),
  workflow_execution_id VARCHAR(255) REFERENCES workflow_executions(id),
  retry_for_id VARCHAR(255) UNIQUE REFERENCES scheduled_jobs(id),
  scheduled_at TIMESTAMPTZ NOT NULL,
  request_id VARCHAR(255) NOT NULL REFERENCES http_requests(id),
  execution_id VARCHAR(255) UNIQUE REFERENCES job_executions(id),
  timeout_ms INTEGER,
  max_retries INTEGER NOT NULL,
  max_response_bytes INTEGER,
  deleted_at TIMESTAMPTZ,
  CONSTRAINT workflow_and_workflow_execution CHECK (
    (workflow_id IS NULL AND workflow_execution_id IS NULL) OR
    (workflow_id IS NOT NULL AND workflow_execution_id IS NOT NULL)
  )
);
