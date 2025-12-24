
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
  max_cron_jobs INTEGER NOT NULL
);

CREATE TABLE http_requests (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  method VARCHAR(10) NOT NULL,
  url TEXT NOT NULL,
  headers TEXT[] NOT NULL,
  body TEXT
);

CREATE TABLE http_responses (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  status INTEGER NOT NULL,
  headers TEXT[] NOT NULL,
  body TEXT NOT NULL
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
  region TEXT NOT NULL,
  tenant_id VARCHAR(255) REFERENCES tenants(id),
  one_off_job_id VARCHAR(255) REFERENCES one_off_jobs(id),
  cron_job_id VARCHAR(255) REFERENCES cron_jobs(id),
  retry_for_id VARCHAR(255) UNIQUE REFERENCES scheduled_jobs(id),
  scheduled_at TIMESTAMPTZ NOT NULL,
  request_id VARCHAR(255) NOT NULL REFERENCES http_requests(id),
  execution_id VARCHAR(255) UNIQUE REFERENCES job_executions(id),
  timeout_ms INTEGER,
  max_retries INTEGER NOT NULL,
  max_response_bytes INTEGER,
  deleted_at TIMESTAMPTZ
);
