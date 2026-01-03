use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use replace_err::ReplaceErr;
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::{Pool, Postgres};
use tokio::select;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;
use tonic::transport::Server;

use crate::broker::broker_server::{Broker as BrokerTrait, BrokerServer};
use crate::secrets::{KeyRing, Secret};
use crate::signing::SignatureBuilder;
use crate::{BrokerOptions, GLOBAL_CONFIG, id};

tonic::include_proto!("broker");

pub struct Config {
    port: usize,
    hostname: String,
    pool: Pool<Postgres>,
    key_ring: KeyRing,
    fallback_signing_key: String,
}

impl Config {
    pub async fn from_cli(options: BrokerOptions, pool: Pool<Postgres>) -> Self {
        Self {
            pool,
            hostname: options.hostname,
            port: options.port,
            key_ring: options.key_ring,
            fallback_signing_key: options.fallback_signing_key,
        }
    }
}

#[derive(Debug)]
struct Broker {
    pool: Pool<Postgres>,
    key_ring: KeyRing,
    fallback_signing_secret: String,
}

#[tonic::async_trait]
impl BrokerTrait for Broker {
    async fn drone_checkin(
        &self,
        req: tonic::Request<DroneCheckinRequest>,
    ) -> Result<tonic::Response<DroneCheckinResponse>, Status> {
        let drone_info = req.into_inner();

        let drone_ip: IpAddr =
            drone_info
                .drone_ip
                .parse()
                .replace_err(Status::invalid_argument(format!(
                    "drone ip {} is not a valid ip address",
                    drone_info.drone_ip
                )))?;
        let ip_network: IpNetwork = drone_ip.into();

        sqlx::query!(
            r#"
          INSERT INTO drones (id, ip, region, last_checkin, checkin_by)
          VALUES ($1, $2, $3, now(), now() + interval '15 seconds')
          ON CONFLICT (id) DO UPDATE SET
            ip = EXCLUDED.ip,
            region = EXCLUDED.region,
            last_checkin = now(),
            checkin_by = now() + interval '15 seconds';
        "#,
            drone_info.drone_id,
            ip_network,
            drone_info.drone_region
        )
        .execute(&self.pool)
        .await
        .replace_err(Status::internal("Unable to upsert drone for some reason."))?;

        let drone_time = DateTime::from_timestamp_millis(drone_info.drone_time_ms)
            .expect("Received invalid time from drone???");

        let report_back_in = drone_time + chrono::Duration::seconds(9);

        Ok(tonic::Response::new(DroneCheckinResponse {
            checkin_again_at: report_back_in.timestamp_millis(),
        }))
    }

    type GetJobsStream = ReceiverStream<Result<JobSpec, Status>>;

    async fn get_jobs(
        &self,
        req: tonic::Request<GetJobsRequest>,
    ) -> Result<tonic::Response<Self::GetJobsStream>, Status> {
        let (tx, rx) = mpsc::channel(8);

        let data = req.into_inner();

        let region = data.region;
        let pool = self.pool.clone();

        let key_ring = self.key_ring.clone();
        let fallback_signing_secret = self.fallback_signing_secret.clone();
        tokio::spawn(async move {
            let mut stream = sqlx::query!(
                r#"
              WITH jobs_to_lock AS (
                SELECT job.id as id, tenants.id as tenant_id, job.request_id
                FROM scheduled_jobs as job
                LEFT JOIN tenants ON
                  job.tenant_id = tenants.id
                WHERE lock_nonce IS NULL
                  AND execution_id IS NULL
                  AND (tenants.tokens > 0 OR tenants.tokens IS NULL)
                  AND (
                    (region = $1 AND scheduled_at <= now() + interval '3 seconds')
                    OR (scheduled_at <= now() - interval '5 seconds')
                  )
                ORDER BY scheduled_at ASC
                FOR UPDATE OF job SKIP LOCKED
              )
              UPDATE scheduled_jobs AS job
              SET lock_nonce = extract(epoch from now())
              FROM
                jobs_to_lock AS to_lock
              JOIN http_requests AS req
                ON req.id = to_lock.request_id
              LEFT JOIN tenants AS tenant
                ON tenant.id = to_lock.tenant_id
              LEFT JOIN secrets as secret
                ON secret.id = tenant.current_signing_key
              WHERE job.id = to_lock.id
                AND (to_lock.tenant_id IS NULL OR job.tenant_id = to_lock.tenant_id)
              RETURNING
                job.id as job_id,
                job.lock_nonce,
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
                req.body;
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
                        .unwrap_or(33554432);

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
                                    eprintln!(
                                        "Error decrypting signing secret {} for tenant {}: {:?}",
                                        signing_secret.id, tenant_id, err
                                    );
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
                                eprintln!(
                                    "Error signing request {}: {:?}",
                                    job.job_id.clone(),
                                    signing_error
                                );
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

                    let job_spec = JobSpec {
                        job_id: job.job_id,
                        lock_nonce: job.lock_nonce.unwrap() as i64,
                        scheduled_at: job.scheduled_at.timestamp(),
                        method: job.method,
                        url: job.url,
                        headers: req_headers,
                        body: job.body,
                        timeout_ms: timeout,
                        max_response_bytes: max_response_bytes as i64,
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

    async fn record_execution(
        &self,
        req: tonic::Request<tonic::Streaming<JobExecution>>,
    ) -> Result<tonic::Response<Empty>, Status> {
        let mut executions = req.into_inner();
        let pool = self.pool.clone();

        tokio::spawn(async move {
            while let Some(job_execution) = executions.next().await {
                if let Ok(execution) = job_execution {
                    let pool = pool.clone();
                    tokio::spawn(async move {
                        let id = execution.job_id.clone();
                        let success: anyhow::Result<()> = async {
                            let mut tx = pool.begin().await?;

                            let scheduled = sqlx::query!(
                                r#"
                            SELECT id, lock_nonce, tenant_id
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
                                    Some(
                                      format!("{k}: {v}")
                                    )
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

                            if let Some(response) = execution.response {
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
                                    response.body
                                )
                                .execute(&mut *tx)
                                .await?;
                            }

                            let execution_id = id::generate("execution");
                            let executed_at = DateTime::from_timestamp_secs(execution.executed_at);

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

                            if let Some(tenant_id) = scheduled.tenant_id {
                              sqlx::query!(r#"
                                UPDATE tenants
                                SET tokens = tokens - 1
                                WHERE id = $1;
                                "#, tenant_id).execute(&mut *tx).await?;
                            }

                            tx.commit().await?;

                            Ok(())
                        }
                        .await;

                        if let Err(error) = success {
                            eprintln!(
                                "Error committing execution to the database for job id {id}: {error:?}"
                            );
                        }
                    });
                }
            }
        });

        Ok(tonic::Response::new(Empty {}))
    }
}

async fn run_cleanup(pool: Pool<Postgres>) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let result = sqlx::query!(
            r#"
              WITH cleanup_candidates AS (
                SELECT id
                FROM scheduled_jobs
                WHERE lock_nonce IS NOT NULL
                  AND to_timestamp(lock_nonce) + (timeout_ms / 1000 || ' seconds')::interval
                      < now() - interval '30 seconds'
                FOR UPDATE SKIP LOCKED
              )
              UPDATE scheduled_jobs
              SET lock_nonce = NULL
              FROM cleanup_candidates
              WHERE scheduled_jobs.id = cleanup_candidates.id;
          "#
        )
        .execute(&pool)
        .await?;

        if result.rows_affected() > 0 {
            println!(
                "Cleaned up {} jobs which were not executed properly.",
                result.rows_affected()
            );
        }
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.hostname, config.port).parse()?;

    if GLOBAL_CONFIG.get().unwrap().is_dev {
        println!("Outgoing Signing Key: {}", &config.fallback_signing_key)
    }

    let cleanup_fut = run_cleanup(config.pool.clone());

    let broker = Broker {
        pool: config.pool,
        key_ring: config.key_ring,
        fallback_signing_secret: config.fallback_signing_key,
    };

    let svc = BrokerServer::new(broker);

    let server_fut = Server::builder().add_service(svc).serve(addr);

    select! {
      server_res = server_fut => {server_res?;},
      cleanup_res = cleanup_fut => {cleanup_res?;}
    };

    Ok(())
}
