use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use sqlx::{Pool, Postgres};
use tokio::sync::mpsc;
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::Status;

use crate::{
    broker::{BrokerService, workflow},
    grpc, id,
    secrets::Secret,
    signing::SignatureBuilder,
};

pub type GetJobsStream = ReceiverStream<Result<grpc::JobSpec, Status>>;

pub async fn get_jobs(
    svc: &BrokerService,
    req: tonic::Request<grpc::GetJobsRequest>,
) -> Result<tonic::Response<GetJobsStream>, Status> {
    let (tx, rx) = mpsc::channel(32);

    let data = req.into_inner();

    let region = data.region;
    let pool = svc.pool.clone();

    let key_ring = svc.key_ring.clone();
    let fallback_signing_secret = svc.fallback_signing_secret.clone();
    tokio::spawn(async move {
        let mut stream = sqlx::query!(
            r#"
        WITH active_tenants AS (
          SELECT id, tokens FROM tenants
          WHERE tokens > 0
          FOR UPDATE SKIP LOCKED
        ),
        candidate_ids AS (
          SELECT job_lat.*
          FROM active_tenants t
          CROSS JOIN LATERAL (
            SELECT
              job.id,
              job.scheduled_at
            FROM scheduled_jobs job
            WHERE job.tenant_id = t.id
              AND job.lock_nonce IS NULL
              AND job.execution_id IS NULL
              AND (
                (job.region = $1 AND job.scheduled_at <= now() + interval '3 seconds')
                OR (job.scheduled_at <= now() - interval '5 seconds')
              )
            ORDER BY job.scheduled_at ASC, job.id ASC
            LIMIT t.tokens
          ) job_lat
          UNION ALL
          SELECT id, scheduled_at
          FROM scheduled_jobs
          WHERE tenant_id IS NULL
            AND lock_nonce IS NULL
            AND execution_id IS NULL
            AND (
              (region = $1 AND scheduled_at <= now() + interval '3 seconds')
              OR (scheduled_At <= now() - interval '5 seconds')
            )
          ORDER BY scheduled_at ASC, id ASC
          LIMIT 100
        ),
        jobs_to_lock AS (
          SELECT id FROM scheduled_jobs
          WHERE id IN (SELECT id FROM candidate_ids)
          FOR UPDATE SKIP LOCKED
        ),
        updated_jobs AS (
          UPDATE scheduled_jobs
          SET
            lock_nonce = extract(epoch from now()),
            times_locked = times_locked + 1
          WHERE id IN (SELECT id FROM jobs_to_lock)
          RETURNING id, tenant_id, lock_nonce
        ),
        updated_tenants AS (
          UPDATE tenants tenant
          SET tokens = GREATEST(0, tokens - sub.used_tokens)
          FROM (
            SELECT tenant_id, count(*) AS used_tokens
            FROM updated_jobs
            WHERE tenant_id IS NOT NULL
            GROUP BY tenant_id
          ) sub
          WHERE tenant.id = sub.tenant_id
          RETURNING tenant.id
        )
        SELECT
          job.id as job_id,
          updated_jobs.lock_nonce,
          job.scheduled_at,
          job.timeout_ms,
          job.max_response_bytes,
          tenant.id as "tenant_id?",
          tenant.max_timeout as "max_timeout?",
          tenant.max_max_response_bytes as "max_max_response_bytes?",
          secret.id as "secret_id?",
          secret.master_key_id as "master_key_id?",
          secret.secret_version as "secret_version?",
          secret.encrypted_dek as "encrypted_dek?",
          secret.encrypted_data as "encrypted_data?",
          secret.dek_nonce as "dek_nonce?",
          secret.data_nonce as "data_nonce?",
          secret.algorithm as "algorithm?",
          req.method,
          req.url,
          req.headers,
          req.body
        FROM scheduled_jobs job
        JOIN updated_jobs
          ON updated_jobs.id = job.id
        JOIN http_requests AS req
          ON req.id = job.request_id
        LEFT JOIN tenants tenant
          ON tenant.id = job.tenant_id
        LEFT JOIN secrets secret
          ON secret.id = tenant.current_signing_key
        ORDER BY job.scheduled_at ASC;
        "#,
            region
        )
        .fetch(&pool);

        while let Some(next) = stream.next().await {
            if let Ok(job) = next {
                let timeout = job.timeout_ms.or(job.max_timeout).unwrap_or(60_000);
                // default 32mb if no tenant limit is set.
                let max_response_bytes = job
                    .max_response_bytes
                    .or(job.max_max_response_bytes)
                    .map(|bytes| bytes as i64);

                let tenant_signing_secret: Option<Secret> = if let Some(id) = job.secret_id
                    && let Some(master_key_id) = job.master_key_id
                    && let Some(secret_version) = job.secret_version
                    && let Some(encrypted_dek) = job.encrypted_dek
                    && let Some(encrypted_data) = job.encrypted_data
                    && let Some(dek_nonce) = job.dek_nonce
                    && let Some(data_nonce) = job.data_nonce
                    && let Some(algorithm) = job.algorithm
                {
                    Some(Secret {
                        id,
                        master_key_id,
                        secret_version,
                        encrypted_dek,
                        encrypted_data,
                        dek_nonce,
                        data_nonce,
                        algorithm,
                    })
                } else {
                    None
                };

                let signing_secret: Option<String> = if let Some(tenant_id) = job.tenant_id {
                    if let Some(signing_secret) = tenant_signing_secret {
                        match signing_secret.decrypt(&key_ring) {
                            Ok(decrypted) => Some(decrypted),
                            Err(err) => {
                                tracing::error! {
                                  %err,
                                  %tenant_id,
                                  signing_secret_id = signing_secret.id,
                                  "Error decrypting signing secret for tenant."
                                };
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    Some(fallback_signing_secret.clone())
                };

                let signature: Option<String> = if let Some(signing_key) = signing_secret {
                    let signature_result = SignatureBuilder {
                        signing_key,
                        method: job.method.clone(),
                        time: Utc::now(),
                        url: job.url.clone(),
                        body: job.body.clone(),
                    }
                    .signature_header();

                    match signature_result {
                        Ok(signature) => Some(signature),
                        Err(signing_error) => {
                            tracing::error! {
                              job_id = job.job_id.clone(),
                              %signing_error,
                              "Error signing request",
                            };
                            None
                        }
                    }
                } else {
                    None
                };

                let mut req_headers = job
                    .headers
                    .iter()
                    .filter_map(|s| {
                        let mut parts = s.splitn(2, ":");
                        let key = parts.next()?.trim().to_string();
                        let value = parts.next()?.trim().to_string();
                        Some((key, value))
                    })
                    .collect::<HashMap<String, String>>();

                req_headers.insert("Rocktick-Job-Id".to_string(), job.job_id.clone());

                if let Some(signature_header) = signature {
                    req_headers.insert("Rocktick-Signature".to_string(), signature_header);
                }

                let job_spec = grpc::JobSpec {
                    job_id: job.job_id,
                    lock_nonce: job.lock_nonce.unwrap() as i64,
                    scheduled_at: job.scheduled_at.timestamp(),
                    method: job.method,
                    url: job.url,
                    headers: req_headers,
                    body: job.body,
                    timeout_ms: timeout,
                    max_response_bytes,
                };

                if tx.send(Ok(job_spec)).await.is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });

    Ok(tonic::Response::new(ReceiverStream::new(rx)))
}

pub type RecordExecutionStream = ReceiverStream<Result<grpc::RecordExecutionResponse, Status>>;

pub async fn record_execution(
    svc: &BrokerService,
    req: tonic::Request<tonic::Streaming<grpc::JobExecution>>,
) -> Result<tonic::Response<RecordExecutionStream>, Status> {
    let (tx, rx) = mpsc::channel(16);

    let mut executions = req.into_inner();
    let pool = svc.pool.clone();

    tokio::spawn(async move {
        while let Some(job_execution) = executions.next().await {
            if let Ok(execution) = job_execution {
                let pool = pool.clone();
                let response = tx.clone();
                tokio::spawn(async move {
                    let id = execution.job_id.clone();
                    let success: anyhow::Result<()> = async {
                        let mut tx = pool.begin().await?;

                        let scheduled = sqlx::query!(
                            r#"
                      SELECT id, lock_nonce, tenant_id, workflow_execution_id
                      FROM scheduled_jobs
                      WHERE id = $1
                        AND lock_nonce = $2
                      FOR UPDATE;
                      "#,
                            execution.job_id,
                            execution.lock_nonce as i32
                        )
                        .fetch_one(&mut *tx)
                        .await?;

                        let request_id = id::generate("request");
                        let req_headers: Vec<String> = execution
                            .req_headers
                            .iter()
                            .filter_map(|(k, v)| {
                                if k.starts_with("Rocktick-") {
                                    None
                                } else {
                                    Some(format!("{k}: {v}"))
                                }
                            })
                            .collect();

                        sqlx::query!(
                            r#"
                        INSERT INTO http_requests
                          (id, method, url, headers, body)
                        VALUES
                          ($1, $2, $3 ,$4, $5)
                        "#,
                            request_id,
                            execution.req_method,
                            execution.req_url,
                            &req_headers,
                            execution.req_body
                        )
                        .execute(&mut *tx)
                        .await?;

                        let mut response_id = None;

                        if let Some(response) = execution.response.clone() {
                            let res_id = id::generate("response");
                            response_id = Some(res_id.clone());

                            let headers: Vec<String> = response
                                .headers
                                .iter()
                                .map(|(k, v)| format!("{k}: {v}"))
                                .collect();

                            sqlx::query!(
                                r#"
                              INSERT INTO http_responses
                                (id, status, headers, body)
                              VALUES
                                ($1, $2, $3, $4);
                              "#,
                                res_id,
                                response.status as i64,
                                &headers,
                                response.body,
                            )
                            .execute(&mut *tx)
                            .await?;
                        }

                        let execution_id = id::generate("execution");
                        let executed_at = DateTime::from_timestamp_secs(execution.executed_at);

                        if executed_at.is_none() {
                            tracing::error! {
                              execution_executed_at = execution.executed_at,
                                "Drone returned invalid executed_at time",
                            };
                        }

                        let executed_at = executed_at.unwrap_or(Utc::now());

                        sqlx::query!(
                            r#"
                          INSERT INTO job_executions
                            (id, executed_at, success, response_id, response_error, request_id)
                          VALUES
                            ($1, $2, $3, $4, $5, $6);
                        "#,
                            execution_id.clone(),
                            executed_at,
                            execution.success,
                            response_id,
                            execution.response_error,
                            request_id
                        )
                        .execute(&mut *tx)
                        .await?;

                        if let Some(workflow_execution_id) = scheduled.workflow_execution_id {
                            let workflow_response_body: Result<String, String> =
                                if let Some(res) = execution.response.clone() {
                                    Ok(res.body)
                                } else if let Some(res_error) = execution.response_error.clone() {
                                    Err(res_error)
                                } else {
                                    Err("Drone did not respond with an response".to_string())
                                };

                            workflow::handle_workflow_execution_side_effect(
                                workflow_execution_id,
                                workflow_response_body,
                                executed_at,
                                &mut tx,
                            )
                            .await?;
                        }

                        sqlx::query!(
                            r#"
                          UPDATE scheduled_jobs
                          SET
                            execution_id = $2,
                            lock_nonce = NULL
                          WHERE id = $1;
                        "#,
                            scheduled.id,
                            execution_id
                        )
                        .execute(&mut *tx)
                        .await?;

                        tx.commit().await?;

                        Ok(())
                    }
                    .await;

                    if let Err(error) = success {
                        tracing::error! {
                          job_id = id,
                          %error,
                          "Error committing execution to the database for job."
                        };
                    } else {
                        let execution_response = grpc::RecordExecutionResponse {
                            job_id: execution.job_id,
                        };

                        if response.send(Ok(execution_response)).await.is_err() {
                            tracing::error! {
                              job_id = id,
                              "Error sending execution response for job."
                            };
                        }
                    }
                });
            }
        }
    });

    Ok(tonic::Response::new(ReceiverStream::new(rx)))
}

pub async fn run_job_cleanup_loop(pool: Pool<Postgres>) -> anyhow::Result<()> {
    loop {
        let mut tx = pool.begin().await?;

        tokio::time::sleep(Duration::from_secs(15)).await;
        let result = sqlx::query!(
            r#"
              WITH cleanup_candidates AS (
                SELECT
                  job.id AS job_id,
                  job.tenant_id
                FROM scheduled_jobs AS job
                LEFT JOIN tenants tenant
                  ON tenant.id = job.tenant_id
                WHERE job.lock_nonce IS NOT NULL
                  AND job.execution_id IS NULL
                  AND to_timestamp(job.lock_nonce)
                      + make_interval(secs => COALESCE(job.timeout_ms, tenant.max_timeout, 120000) / 1000)
                      -- 90 second safety interval just in case it takes a while to report or smth.
                      + interval '90 seconds'
                      < now()
              ),
              locked_candidates AS (
                SELECT *
                FROM cleanup_candidates
                FOR UPDATE SKIP LOCKED
              ),
              reset_jobs AS (
                UPDATE scheduled_jobs as job
                SET lock_nonce = NULL
                FROM locked_candidates
                WHERE job.id = locked_candidates.job_id
                RETURNING locked_candidates.tenant_id
              ),
              refunds AS (
                SELECT tenant_id, count(*) AS refund_tokens
                FROM reset_jobs
                WHERE tenant_id IS NOT NULL
                GROUP BY tenant_id
              )
              UPDATE tenants
              SET tokens = LEAST(tenants.max_tokens, tokens + refunds.refund_tokens)
              FROM refunds
              WHERE tenants.id = refunds.tenant_id
          "#
        )
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() > 0 {
            tracing::warn! {
              count = result.rows_affected(),
              "Cleaned up jobs which were not executed properly."
            };
        }

        tx.commit().await?;
    }
}
