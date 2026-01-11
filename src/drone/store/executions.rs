use crate::{drone::store::DroneStore, grpc};
use anyhow::{Context, anyhow};
use chrono::{DateTime, Duration, Utc};
use sqlx::prelude::FromRow;
use std::collections::HashMap;

use crate::{grpc::JobExecution, id};

impl DroneStore {
    pub async fn get_execution(
        &self,
        id: String,
    ) -> anyhow::Result<(JobExecution, ExecutionMetadata)> {
        let mut tx = self.pool.begin().await?;

        let execution: IntermediateExecution = sqlx::query_as(
            r#"
            SELECT
              exec.*,
              res.status as res_status,
              res.header_map as res_header_map,
              res.body as res_body,
              res.bytes_used as res_bytes_used
            FROM executions exec
            LEFT JOIN execution_responses res
              ON exec.response_id = res.id
            WHERE exec.job_id = $1;
            "#,
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        intermediate_to_execution(execution)
    }

    pub async fn insert_execution(
        &self,
        exec: grpc::JobExecution,
        local: bool,
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        let mut response_id = None;

        if let Some(res) = exec.response {
            let id = id::generate("execution_response");
            response_id = Some(id.clone());

            let res_headers = serde_json::to_string(&res.headers)?;

            sqlx::query(
                r#"
                INSERT INTO execution_responses
                  (id, status, header_map, body, bytes_used)
                VALUES
                  ($1, $2, $3, $4, $5);
            "#,
            )
            .bind(id)
            .bind(res.status)
            .bind(res_headers)
            .bind(res.body)
            .bind(res.bytes_used)
            .execute(&mut *tx)
            .await?;
        }

        let req_headers = serde_json::to_string(&exec.req_headers)?;

        sqlx::query(
            r#"
          INSERT INTO executions
            (job_id,
            success,
            lock_nonce,
            response_id,
            response_error,
            req_method,
            req_url,
            req_header_map,
            req_body,
            req_body_bytes_used,
            executed_at,
            is_local,
            replicated_times,
            sync_status,
            sync_time,
            sync_nonce)
          VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16);
        "#,
        )
        .bind(exec.job_id)
        .bind(exec.success)
        .bind(exec.lock_nonce)
        .bind(response_id)
        .bind(exec.response_error)
        .bind(exec.req_method)
        .bind(exec.req_url)
        .bind(req_headers)
        .bind(exec.req_body)
        .bind(exec.req_body_bytes_used)
        .bind(exec.executed_at)
        .bind(local)
        .bind(0)
        .bind("local")
        .bind::<Option<i64>>(None)
        .bind::<Option<i64>>(None)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    pub async fn record_replication(&self, job_id: String) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
      UPDATE executions
      SET replicated_times = replicated_times + 1
      WHERE job_id = $1;
    "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn cleanup_executions_post_sync(&self, sync_nonce: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
        UPDATE executions
        SET
          sync_status = 'local',
          sync_time = NULL,
          sync_nonce = NULL
        WHERE
          sync_nonce = $1
        "#,
        )
        .bind(sync_nonce)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn cleanup_executions(&self) -> Result<(), sqlx::Error> {
        let cleanup_before = (Utc::now() - Duration::hours(1)).timestamp();

        sqlx::query(
            r#"
          UPDATE executions
          SET
            sync_status = 'local',
            sync_time = NULL,
            sync_nonce = NULL
          WHERE
            sync_time < $1;
        "#,
        )
        .bind(cleanup_before)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
          DELETE FROM executions
          WHERE sync_status = 'synced';
        "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
          DELETE FROM execution_responses
          WHERE NOT EXISTS (
            SELECT 1
            FROM executions
            WHERE
              executions.response_id = execution_responses.id
          )
        "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_job_to_sync(&self, sync_nonce: i64) -> anyhow::Result<Option<JobExecution>> {
        let mut tx = self.pool.begin().await?;

        let job_id: Option<String> = sqlx::query_scalar(
            r#"
            UPDATE executions
            SET
              sync_status = 'pending',
              sync_nonce = $1,
              sync_time = $2
            WHERE job_id = (
              SELECT job_id FROM executions
              WHERE sync_status = 'local'
              ORDER BY executed_at ASC
              LIMIT 1
            )
            RETURNING job_id
            "#,
        )
        .bind(sync_nonce)
        .bind(Utc::now().timestamp())
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(id) = job_id {
            let intermediate: IntermediateExecution = sqlx::query_as(
                r#"
                SELECT
                  exec.*,
                  res.status as res_status,
                  res.header_map as res_header_map,
                  res.body as res_body,
                  res.bytes_used as res_bytes_used
                FROM executions exec
                LEFT JOIN execution_responses res
                  ON exec.response_id = res.id
                WHERE exec.job_id = $1;
                "#,
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;

            tx.commit().await?;

            let (execution, _) = intermediate_to_execution(intermediate)?;

            Ok(Some(execution))
        } else {
            tx.commit().await?;
            Ok(None)
        }
    }

    pub async fn mark_successfully_synced(&self, job_id: String) -> anyhow::Result<()> {
        sqlx::query(
            r#"
          UPDATE executions
          SET
            sync_status = 'synced',
            sync_time = NULL,
            sync_nonce = NULL
          WHERE job_id = $1
        "#,
        )
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug, Clone, FromRow)]
struct IntermediateExecution {
    job_id: String,
    success: bool,
    lock_nonce: i64,
    response_id: Option<String>,
    response_error: Option<String>,
    req_method: String,
    req_url: String,
    req_header_map: String,
    req_body: Option<String>,
    req_body_bytes_used: i64,
    executed_at: i64,
    is_local: bool,
    replicated_times: i64,
    sync_status: String,
    sync_time: Option<i64>,
    sync_nonce: Option<i64>,
    res_status: Option<i64>,
    res_header_map: Option<String>,
    res_body: Option<String>,
    res_bytes_used: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum SyncStatus {
    Local,
    Pending,
    Synced,
}

#[derive(Debug, Clone)]
pub struct ExecutionMetadata {
    pub is_local: bool,
    pub replicated_times: i64,
    pub sync_status: SyncStatus,
    pub sync_time: DateTime<Utc>,
    pub sync_nonce: i64,
}

fn intermediate_to_execution(
    exec: IntermediateExecution,
) -> anyhow::Result<(JobExecution, ExecutionMetadata)> {
    let req_headers: HashMap<String, String> = serde_json::from_str(&exec.req_header_map)
        .context("Failed to deserialize request headers")?;

    // let response = if let Some(r) = res {
    //     let headers: HashMap<String, String> = serde_json::from_str(&r.header_map)
    //         .context("Failed to deserialize response headers")?;
    //     Some(grpc::Response {
    //         status: r.status,
    //         headers,
    //         body: r.body,
    //         bytes_used: r.bytes_used,
    //     })
    // } else {
    //     None
    // };

    let response = if let Some(status) = exec.res_status
        && let Some(res_header_map) = exec.res_header_map
        && let Some(body) = exec.res_body
        && let Some(bytes_used) = exec.res_bytes_used
    {
        let headers: HashMap<String, String> = serde_json::from_str(&res_header_map)
            .context("Failed to deserialize response headers")?;
        Some(grpc::Response {
            status,
            headers,
            body,
            bytes_used,
        })
    } else {
        None
    };

    let sync_status = match exec.sync_status.as_str() {
        "local" => SyncStatus::Local,
        "pending" => SyncStatus::Pending,
        "synced" => SyncStatus::Synced,
        _ => return Err(anyhow!("Invalid sync_status: {}", exec.sync_status)),
    };

    let sync_time = if let Some(ts) = exec.sync_time {
        DateTime::from_timestamp(ts, 0)
            .ok_or_else(|| anyhow!("Invalid sync_time timestamp: {}", ts))?
    } else {
        DateTime::from_timestamp(0, 0)
            .ok_or_else(|| anyhow!("Failed to create default timestamp"))?
    };

    let sync_nonce = exec.sync_nonce.unwrap_or(0);

    Ok((
        JobExecution {
            job_id: exec.job_id,
            success: exec.success,
            lock_nonce: exec.lock_nonce,
            response,
            response_error: exec.response_error,
            req_method: exec.req_method,
            req_url: exec.req_url,
            req_headers,
            req_body: exec.req_body,
            req_body_bytes_used: exec.req_body_bytes_used,
            executed_at: exec.executed_at,
        },
        ExecutionMetadata {
            is_local: exec.is_local,
            replicated_times: exec.replicated_times,
            sync_status,
            sync_time,
            sync_nonce,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_and_get_execution() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_insert_and_get_execution").await?;

        let mut req_headers = HashMap::new();
        req_headers.insert("Content-Type".to_string(), "application/json".to_string());

        let mut res_headers = HashMap::new();
        res_headers.insert("Server".to_string(), "Rocktick".to_string());

        let job_id = "job_12345".to_string();

        let execution = JobExecution {
            job_id: job_id.clone(),
            success: true,
            lock_nonce: 42,
            response: Some(grpc::Response {
                status: 200,
                headers: res_headers,
                body: "{\"status\": \"ok\"}".to_string(),
                bytes_used: 15,
            }),
            response_error: None,
            req_method: "POST".to_string(),
            req_url: "https://api.example.com/job".to_string(),
            req_headers,
            req_body: Some("{\"data\": 1}".to_string()),
            req_body_bytes_used: 10,
            executed_at: 1234567890,
        };

        store.insert_execution(execution.clone(), true).await?;

        let (fetched_execution, metadata) = store.get_execution(job_id.clone()).await?;

        assert_eq!(fetched_execution.job_id, execution.job_id);
        assert_eq!(fetched_execution.success, execution.success);
        assert_eq!(fetched_execution.lock_nonce, execution.lock_nonce);
        assert_eq!(fetched_execution.req_method, execution.req_method);
        assert_eq!(fetched_execution.req_url, execution.req_url);
        assert_eq!(fetched_execution.req_body, execution.req_body);
        assert_eq!(fetched_execution.req_headers, execution.req_headers);

        let fetched_response = fetched_execution.response.unwrap();
        let expected_response = execution.response.unwrap();
        assert_eq!(fetched_response.status, expected_response.status);
        assert_eq!(fetched_response.body, expected_response.body);
        assert_eq!(fetched_response.headers, expected_response.headers);

        assert!(metadata.is_local);
        assert_eq!(metadata.replicated_times, 0);
        assert!(matches!(metadata.sync_status, SyncStatus::Local));
        assert_eq!(metadata.sync_nonce, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_record_replication() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_record_replication").await?;
        let job_id = "job_rep".to_string();

        let execution = JobExecution {
            job_id: job_id.clone(),
            success: false,
            lock_nonce: 1,
            response: None,
            response_error: Some("Connection failed".to_string()),
            req_method: "GET".to_string(),
            req_url: "http://test.com".to_string(),
            req_headers: HashMap::new(),
            req_body: None,
            req_body_bytes_used: 0,
            executed_at: 987654321,
        };

        store.insert_execution(execution, false).await?;

        let (_, metadata_initial) = store.get_execution(job_id.clone()).await?;
        assert_eq!(metadata_initial.replicated_times, 0);

        store.record_replication(job_id.clone()).await?;
        let (_, metadata_after) = store.get_execution(job_id.clone()).await?;
        assert_eq!(metadata_after.replicated_times, 1);

        store.record_replication(job_id.clone()).await?;
        let (_, metadata_again) = store.get_execution(job_id.clone()).await?;
        assert_eq!(metadata_again.replicated_times, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_insert_execution_isolated() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_insert_execution_isolated").await?;

        let mut req_headers = HashMap::new();
        req_headers.insert("Accept".to_string(), "*/*".to_string());

        let mut res_headers = HashMap::new();
        res_headers.insert("Content-Type".to_string(), "text/plain".to_string());

        let job_id = "job_isolated_insert".to_string();
        let response_body = "response_body".to_string();
        let req_body = "req_body".to_string();

        let execution = JobExecution {
            job_id: job_id.clone(),
            success: true,
            lock_nonce: 100,
            response: Some(grpc::Response {
                status: 201,
                headers: res_headers.clone(),
                body: response_body.clone(),
                bytes_used: 50,
            }),
            response_error: None,
            req_method: "PUT".to_string(),
            req_url: "http://test.local".to_string(),
            req_headers: req_headers.clone(),
            req_body: Some(req_body.clone()),
            req_body_bytes_used: 20,
            executed_at: 1111111111,
        };

        store.insert_execution(execution.clone(), true).await?;

        // Verify directly via SQL
        // Tuple: (job_id, success, lock_nonce, response_id, response_error, req_method, req_url, req_header_map, req_body, req_body_bytes_used, executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce)
        let row: (
            String,
            bool,
            i64,
            Option<String>,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
            i64,
            i64,
            bool,
            i64,
            String,
            Option<i64>,
            Option<i64>,
        ) = sqlx::query_as(
            r#"
            SELECT
                job_id, success, lock_nonce, response_id, response_error,
                req_method, req_url, req_header_map, req_body, req_body_bytes_used,
                executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce
            FROM executions
            WHERE job_id = ?
            "#,
        )
        .bind(job_id.clone())
        .fetch_one(&store.pool)
        .await?;

        assert_eq!(row.0, job_id);
        assert_eq!(row.1, true); // success
        assert_eq!(row.2, 100); // lock_nonce
        assert!(row.3.is_some()); // response_id should be some
        assert_eq!(row.4, None); // response_error
        assert_eq!(row.5, "PUT"); // req_method
        assert_eq!(row.6, "http://test.local"); // req_url
        // Verify req headers json
        let stored_req_headers: HashMap<String, String> = serde_json::from_str(&row.7)?;
        assert_eq!(stored_req_headers, req_headers);
        assert_eq!(row.8, Some(req_body)); // req_body
        assert_eq!(row.9, 20); // req_body_bytes_used
        assert_eq!(row.10, 1111111111); // executed_at
        assert_eq!(row.11, true); // is_local
        assert_eq!(row.12, 0); // replicated_times
        assert_eq!(row.13, "local"); // sync_status
        assert_eq!(row.14, None); // sync_time
        assert_eq!(row.15, None); // sync_nonce

        // Verify response table
        let response_id = row.3.unwrap();
        let res_row: (String, i64, String, String, i64) = sqlx::query_as(
            "SELECT id, status, header_map, body, bytes_used FROM execution_responses WHERE id = ?",
        )
        .bind(response_id)
        .fetch_one(&store.pool)
        .await?;

        assert_eq!(res_row.1, 201); // status
        let stored_res_headers: HashMap<String, String> = serde_json::from_str(&res_row.2)?;
        assert_eq!(stored_res_headers, res_headers);
        assert_eq!(res_row.3, response_body);
        assert_eq!(res_row.4, 50);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_execution_isolated() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_get_execution_isolated").await?;
        let job_id = "job_isolated_get".to_string();
        let response_id = "resp_isolated_get".to_string();

        let req_headers_map = HashMap::from([("H1".to_string(), "V1".to_string())]);
        let res_headers_map = HashMap::from([("H2".to_string(), "V2".to_string())]);

        let req_headers_json = serde_json::to_string(&req_headers_map)?;
        let res_headers_json = serde_json::to_string(&res_headers_map)?;

        // Manually insert response
        sqlx::query(
            r#"
            INSERT INTO execution_responses (id, status, header_map, body, bytes_used)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&response_id)
        .bind(404)
        .bind(&res_headers_json)
        .bind("Not Found")
        .bind(100)
        .execute(&store.pool)
        .await?;

        // Manually insert execution
        sqlx::query(
            r#"
            INSERT INTO executions
              (job_id, success, lock_nonce, response_id, response_error,
               req_method, req_url, req_header_map, req_body, req_body_bytes_used,
               executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&job_id)
        .bind(false) // success
        .bind(55) // lock_nonce
        .bind(Some(&response_id))
        .bind(Option::<String>::None) // response_error must be null if response_id is set
        .bind("DELETE")
        .bind("http://delete.me")
        .bind(&req_headers_json)
        .bind(Some("del_body"))
        .bind(5)
        .bind(2222222222i64)
        .bind(false) // is_local
        .bind(3) // replicated_times
        .bind("pending") // sync_status
        .bind(Some(3333333333i64)) // sync_time
        .bind(Some(42i64)) // sync_nonce
        .execute(&store.pool)
        .await?;

        // Now test get_execution
        let (execution, metadata) = store.get_execution(job_id.clone()).await?;

        assert_eq!(execution.job_id, job_id);
        assert_eq!(execution.success, false);
        assert_eq!(execution.lock_nonce, 55);
        assert_eq!(execution.req_method, "DELETE");
        assert_eq!(execution.req_url, "http://delete.me");
        assert_eq!(execution.req_headers, req_headers_map);
        assert_eq!(execution.req_body, Some("del_body".to_string()));
        assert_eq!(execution.req_body_bytes_used, 5);
        assert_eq!(execution.executed_at, 2222222222);

        let resp = execution.response.expect("Response should be present");
        assert_eq!(resp.status, 404);
        assert_eq!(resp.headers, res_headers_map);
        assert_eq!(resp.body, "Not Found");
        assert_eq!(resp.bytes_used, 100);

        assert_eq!(metadata.is_local, false);
        assert_eq!(metadata.replicated_times, 3);
        assert!(matches!(metadata.sync_status, SyncStatus::Pending));
        assert_eq!(metadata.sync_time.timestamp(), 3333333333);
        assert_eq!(metadata.sync_nonce, 42);

        Ok(())
    }

    async fn insert_dummy_exec(
        pool: &sqlx::Pool<sqlx::Sqlite>,
        job_id: String,
        sync_status: String,
        sync_nonce: Option<i64>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
                INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error,
                    req_method, req_url, req_header_map, req_body, req_body_bytes_used,
                    executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce
                ) VALUES (?, 1, 1, NULL, 'err', 'GET', 'http://u', '{}', NULL, 0, 100, 1, 0, ?, ?, ?)
                "#,
        )
        .bind(job_id)
        .bind(sync_status)
        .bind(if sync_nonce.is_some() {
            Some(Utc::now().timestamp())
        } else {
            None
        })
        .bind(sync_nonce)
        .execute(pool)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_executions_post_sync() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_cleanup_executions_post_sync").await?;
        let job_id_1 = "job_post_sync_1";
        let job_id_2 = "job_post_sync_2";
        let nonce_target = 100;
        let nonce_other = 200;

        insert_dummy_exec(
            &store.pool,
            job_id_1.to_string(),
            "pending".to_string(),
            Some(nonce_target),
        )
        .await?;
        insert_dummy_exec(
            &store.pool,
            job_id_2.to_string(),
            "pending".to_string(),
            Some(nonce_other),
        )
        .await?;

        store.cleanup_executions_post_sync(nonce_target).await?;

        // Check job 1
        let (_, meta1) = store.get_execution(job_id_1.to_string()).await?;
        assert!(matches!(meta1.sync_status, SyncStatus::Local));
        assert_eq!(meta1.sync_nonce, 0);

        // Check job 2 - should be untouched
        let (_, meta2) = store.get_execution(job_id_2.to_string()).await?;
        assert!(matches!(meta2.sync_status, SyncStatus::Pending));
        assert_eq!(meta2.sync_nonce, nonce_other);

        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_executions() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_cleanup_executions").await?;

        // 1. Stuck pending job (older than 1 hour) -> should become local
        let job_stuck = "job_stuck";
        let old_time = (Utc::now() - Duration::hours(2)).timestamp();
        sqlx::query(
             r#"INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error, req_method, req_url, req_header_map, req_body_bytes_used, executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce
                ) VALUES (?, 1, 1, NULL, 'err', 'GET', 'http://u', '{}', 0, 100, 1, 0, 'pending', ?, 123)"#
        )
        .bind(job_stuck)
        .bind(old_time)
        .execute(&store.pool).await?;

        // 2. Recent pending job -> should stay pending
        let job_recent = "job_recent";
        let recent_time = Utc::now().timestamp();
        sqlx::query(
             r#"INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error, req_method, req_url, req_header_map, req_body_bytes_used, executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce
                ) VALUES (?, 1, 1, NULL, 'err', 'GET', 'http://u', '{}', 0, 100, 1, 0, 'pending', ?, 124)"#
        )
        .bind(job_recent)
        .bind(recent_time)
        .execute(&store.pool).await?;

        // 3. Synced job -> should be deleted
        let job_synced = "job_synced";
        sqlx::query(
             r#"INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error, req_method, req_url, req_header_map, req_body_bytes_used, executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce
                ) VALUES (?, 1, 1, NULL, 'err', 'GET', 'http://u', '{}', 0, 100, 1, 0, 'synced', ?, 125)"#
        )
        .bind(job_synced)
        .bind(recent_time)
        .execute(&store.pool).await?;

        // 4. Orphaned response -> should be deleted
        let orphan_resp_id = "orphan_resp";
        sqlx::query("INSERT INTO execution_responses (id, status, header_map, body, bytes_used) VALUES (?, 200, '{}', 'b', 0)")
            .bind(orphan_resp_id)
            .execute(&store.pool).await?;

        // 5. Response linked to kept job (job_recent) -> should be kept
        let kept_resp_id = "kept_resp";
        sqlx::query("INSERT INTO execution_responses (id, status, header_map, body, bytes_used) VALUES (?, 200, '{}', 'b', 0)")
            .bind(kept_resp_id)
            .execute(&store.pool).await?;

        // Link kept_resp to job_recent (update existing row)
        sqlx::query(
            "UPDATE executions SET response_id = ?, response_error = NULL WHERE job_id = ?",
        )
        .bind(kept_resp_id)
        .bind(job_recent)
        .execute(&store.pool)
        .await?;

        store.cleanup_executions().await?;

        // Verify job_stuck -> local
        let (_, meta_stuck) = store.get_execution(job_stuck.to_string()).await?;
        assert!(matches!(meta_stuck.sync_status, SyncStatus::Local));
        assert!(meta_stuck.sync_time.timestamp() == 0);

        // Verify job_recent -> pending
        let (_, meta_recent) = store.get_execution(job_recent.to_string()).await?;
        assert!(matches!(meta_recent.sync_status, SyncStatus::Pending));

        // Verify job_synced -> deleted
        let res_synced = store.get_execution(job_synced.to_string()).await;
        assert!(res_synced.is_err()); // Should fail to find

        // Verify orphan_resp -> deleted
        let row_orphan: Option<(String,)> =
            sqlx::query_as("SELECT id FROM execution_responses WHERE id = ?")
                .bind(orphan_resp_id)
                .fetch_optional(&store.pool)
                .await?;
        assert!(row_orphan.is_none());

        // Verify kept_resp -> kept
        let row_kept: Option<(String,)> =
            sqlx::query_as("SELECT id FROM execution_responses WHERE id = ?")
                .bind(kept_resp_id)
                .fetch_optional(&store.pool)
                .await?;
        assert!(row_kept.is_some());

        Ok(())
    }

    async fn setup_job_exec(
        pool: &sqlx::Pool<sqlx::Sqlite>,
        id: String,
        exec_time: i64,
        status: String,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
                INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error,
                    req_method, req_url, req_header_map, req_body, req_body_bytes_used,
                    executed_at, is_local, replicated_times, sync_status
                ) VALUES (?, 1, 1, NULL, 'err', 'GET', 'http://u', '{}', NULL, 0, ?, 1, 0, ?)
                "#,
        )
        .bind(id)
        .bind(exec_time)
        .bind(status)
        .execute(pool)
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_get_job_to_sync() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_get_job_to_sync").await?;

        // Insert jobs with different execution times
        setup_job_exec(&store.pool, "job1".to_string(), 100, "local".to_string()).await?;
        setup_job_exec(&store.pool, "job2".to_string(), 200, "local".to_string()).await?;
        setup_job_exec(&store.pool, "job3".to_string(), 300, "pending".to_string()).await?;

        let nonce = 555;
        let job = store.get_job_to_sync(nonce).await?;
        assert!(job.is_some());
        let j = job.unwrap();
        assert_eq!(j.job_id, "job1"); // Oldest local

        // Verify status changed to pending
        let (_, meta) = store.get_execution("job1".to_string()).await?;
        assert!(matches!(meta.sync_status, SyncStatus::Pending));
        assert_eq!(meta.sync_nonce, nonce);

        // Next fetch
        let job_next = store.get_job_to_sync(666).await?;
        assert!(job_next.is_some());
        let j2 = job_next.unwrap();
        assert_eq!(j2.job_id, "job2");

        // No more local jobs
        let job_none = store.get_job_to_sync(777).await?;
        assert!(job_none.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_mark_successfully_synced() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_mark_successfully_synced").await?;
        let job_id = "job_sync_complete";

        sqlx::query(
             r#"
                INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error,
                    req_method, req_url, req_header_map, req_body, req_body_bytes_used,
                    executed_at, is_local, replicated_times, sync_status, sync_time, sync_nonce
                ) VALUES (?, 1, 1, NULL, 'err', 'GET', 'http://u', '{}', NULL, 0, 100, 1, 0, 'pending', 123456, 789)
                "#
        )
        .bind(job_id)
        .execute(&store.pool)
        .await?;

        store.mark_successfully_synced(job_id.to_string()).await?;

        // Verify status
        let (_, meta) = store.get_execution(job_id.to_string()).await?;
        assert!(matches!(meta.sync_status, SyncStatus::Synced));
        assert!(meta.sync_time.timestamp() == 0); // Default when null
        assert_eq!(meta.sync_nonce, 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_duplicate_job_insertion() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_duplicate_job_insertion").await?;
        let job_id = "job_duplicate";

        let execution = JobExecution {
            job_id: job_id.to_string(),
            success: true,
            lock_nonce: 1,
            response: None,
            response_error: Some("err".to_string()),
            req_method: "GET".to_string(),
            req_url: "http://example.com".to_string(),
            req_headers: HashMap::new(),
            req_body: None,
            req_body_bytes_used: 0,
            executed_at: 100,
        };

        store
            .insert_execution(execution.clone(), true)
            .await
            .expect("First insertion should succeed");

        let result = store.insert_execution(execution, true).await;
        assert!(
            result.is_err(),
            "Second insertion with same job_id should fail"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_stuck_sync_retry_flow() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_stuck_sync_retry_flow").await?;
        let job_id = "job_stuck_retry";

        // 1. Insert local job
        setup_job_exec(&store.pool, job_id.to_string(), 100, "local".to_string()).await?;

        // 2. Pick it up for sync (nonce 1)
        let picked = store.get_job_to_sync(1).await?;
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().job_id, job_id);

        let (_, meta) = store.get_execution(job_id.to_string()).await?;
        assert!(matches!(meta.sync_status, SyncStatus::Pending));
        assert_eq!(meta.sync_nonce, 1);

        // 3. Simulate getting stuck (update time to > 1 hour ago)
        let old_time = (Utc::now() - Duration::hours(2)).timestamp();
        sqlx::query("UPDATE executions SET sync_time = ? WHERE job_id = ?")
            .bind(old_time)
            .bind(job_id)
            .execute(&store.pool)
            .await?;

        // 4. Run cleanup
        store.cleanup_executions().await?;

        // Verify it reverted to local
        let (_, meta_after_cleanup) = store.get_execution(job_id.to_string()).await?;
        assert!(matches!(meta_after_cleanup.sync_status, SyncStatus::Local));
        assert_eq!(meta_after_cleanup.sync_nonce, 0);

        // 5. Pick it up again (nonce 2)
        let picked_again = store.get_job_to_sync(2).await?;
        assert!(picked_again.is_some());
        assert_eq!(picked_again.unwrap().job_id, job_id);

        let (_, meta_final) = store.get_execution(job_id.to_string()).await?;
        assert!(matches!(meta_final.sync_status, SyncStatus::Pending));
        assert_eq!(meta_final.sync_nonce, 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_idempotency_non_existent() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_idempotency_non_existent").await?;

        // Should not fail
        store
            .mark_successfully_synced("non_existent_job".to_string())
            .await?;

        // Should not fail
        store.cleanup_executions_post_sync(9999).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_zombie_response_cleanup() -> anyhow::Result<()> {
        let store = DroneStore::in_memory("test_zombie_response_cleanup").await?;

        // 1. Insert execution and response
        let job_id = "job_with_res";
        let res_id = "res_linked";

        let mut res_headers = HashMap::new();
        res_headers.insert("h".to_string(), "v".to_string());
        let res_headers_json = serde_json::to_string(&res_headers)?;

        sqlx::query("INSERT INTO execution_responses (id, status, header_map, body, bytes_used) VALUES (?, 200, ?, 'b', 0)")
            .bind(res_id)
            .bind(&res_headers_json)
            .execute(&store.pool).await?;

        sqlx::query(
             r#"INSERT INTO executions (
                    job_id, success, lock_nonce, response_id, response_error, req_method, req_url, req_header_map, req_body_bytes_used, executed_at, is_local, replicated_times, sync_status
                ) VALUES (?, 1, 1, ?, NULL, 'GET', 'http://u', '{}', 0, 100, 1, 0, 'local')"#
        )
        .bind(job_id)
        .bind(res_id)
        .execute(&store.pool).await?;

        // 2. Insert zombie response (no execution links to it)
        let zombie_res_id = "res_zombie";
        sqlx::query("INSERT INTO execution_responses (id, status, header_map, body, bytes_used) VALUES (?, 200, ?, 'b', 0)")
            .bind(zombie_res_id)
            .bind(&res_headers_json)
            .execute(&store.pool).await?;

        // 3. Manually delete the execution (simulating external deletion or other logic)
        // Note: DELETE CASCADE is not set up on the DB level for executions->responses in schema_sqlite.sql (based on provided context, it says execution_responses doesn't have a FK to executions. executions has FK to responses).
        // Wait, looking at schema:
        // executions has `response_id TEXT REFERENCES execution_responses(id) ON DELETE CASCADE`
        // This means if RESPONSE is deleted, execution might be affected? No, standard SQL is FK is on child.
        // `executions` is the child here referencing `execution_responses`.
        // If we delete `executions` row, the `execution_responses` row is NOT automatically deleted by SQL.
        // So we must rely on `cleanup_executions`.

        sqlx::query("DELETE FROM executions WHERE job_id = ?")
            .bind(job_id)
            .execute(&store.pool)
            .await?;

        // 4. Run cleanup
        store.cleanup_executions().await?;

        // 5. Verify zombie_res_id is gone
        let row_zombie: Option<(String,)> =
            sqlx::query_as("SELECT id FROM execution_responses WHERE id = ?")
                .bind(zombie_res_id)
                .fetch_optional(&store.pool)
                .await?;
        assert!(row_zombie.is_none(), "Zombie response should be deleted");

        // 6. Verify previously linked res_id is ALSO gone (since its parent execution was deleted)
        let row_linked: Option<(String,)> =
            sqlx::query_as("SELECT id FROM execution_responses WHERE id = ?")
                .bind(res_id)
                .fetch_optional(&store.pool)
                .await?;
        assert!(
            row_linked.is_none(),
            "Response linked to deleted execution should be deleted"
        );

        Ok(())
    }
}
