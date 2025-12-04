
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
  body TEXT
);

CREATE TABLE scheduled_jobs (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  hash INTEGER NOT NULL,
  lock_nonce INTEGER,
  region TEXT NOT NULL,
  scheduled_at TIMESTAMP NOT NULL,
  request VARCHAR(255) NOT NULL REFERENCES http_requests(id),
  timeout_ms INTEGER NOT NULL,
  max_retries INTEGER NOT NULL
);

CREATE TABLE job_executions (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  job_id VARCHAR(255) NOT NULL REFERENCES scheduled_jobs(id),
  success BOOLEAN,
  response VARCHAR(255) REFERENCES http_responses(id),
  response_error TEXT
);
