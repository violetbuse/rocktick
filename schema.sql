
CREATE TABLE http_requests (
  id VARCHAR(255) NOT NULL PRIMARY KEY,
  method VARCHAR(10) NOT NULL,
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
  request VARCHAR(255) NOT NULL REFERENCES http_requests(id),
  response VARCHAR(255) REFERENCES http_responses(id)
);
