
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

CREATE TABLE job_executions (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  executed_at TIMESTAMP NOT NULL,
  success BOOLEAN,
  response_id VARCHAR(255) UNIQUE REFERENCES http_responses(id),
  response_error TEXT
);

CREATE TABLE scheduled_jobs (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  hash INTEGER NOT NULL,
  lock_nonce INTEGER,
  region TEXT NOT NULL,
  scheduled_at TIMESTAMP NOT NULL,
  request_id VARCHAR(255) NOT NULL UNIQUE REFERENCES http_requests(id),
  execution_id VARCHAR(255) UNIQUE REFERENCES job_executions(id),
  timeout_ms INTEGER NOT NULL,
  max_retries INTEGER NOT NULL,
  max_response_bytes INTEGER NOT NULL
);
