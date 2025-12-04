use std::time::Duration;

use sqlx::{Pool, Postgres};
use tokio::select;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;
use tonic::transport::Server;

use crate::BrokerOptions;
use crate::broker::broker_server::{Broker as BrokerTrait, BrokerServer};

tonic::include_proto!("broker");

pub struct Config {
    port: usize,
    pool: Pool<Postgres>,
}

impl Config {
    pub async fn from_cli(options: BrokerOptions, pool: Pool<Postgres>) -> Self {
        Self {
            pool,
            port: options.port,
        }
    }
}

#[derive(Debug)]
struct Broker {
    pool: Pool<Postgres>,
}

#[tonic::async_trait]
impl BrokerTrait for Broker {
    type GetJobsStream = ReceiverStream<Result<JobSpec, Status>>;

    async fn get_jobs(
        &self,
        req: tonic::Request<GetJobsRequest>,
    ) -> Result<tonic::Response<Self::GetJobsStream>, Status> {
        let (tx, rx) = mpsc::channel(8);

        let region = req.into_inner().region;
        let pool = self.pool.clone();

        tokio::spawn(async move {
            let mut stream = sqlx::query!(
                r#"
              WITH jobs_to_lock AS (
                SELECT id
                FROM scheduled_jobs
                WHERE lock_nonce IS NULL
                  AND execution_id IS NULL
                  AND (
                    (region = $1 AND scheduled_at <= now() + interval '5 seconds')
                    OR (scheduled_at <= now() - interval '5 seconds')
                  )
                ORDER BY scheduled_at
                LIMIT $2
              )
              UPDATE scheduled_jobs j
              SET lock_nonce = extract(epoch from now())
              FROM http_requests r, jobs_to_lock t
              WHERE j.id = t.id
                AND j.request_id = r.id
              RETURNING
                j.id as job_id,
                j.lock_nonce as lock_nonce,
                j.scheduled_at as scheduled_at,
                j.timeout_ms as timeout_ms,
                j.max_response_bytes as max_response_bytes,
                r.method as method,
                r.url as url,
                r.headers as headers,
                r.body as body;
              "#,
                region,
                2000
            )
            .fetch(&pool);

            while let Some(next) = stream.next().await {
                if let Ok(job) = next {
                    let job_spec = JobSpec {
                        job_id: job.job_id,
                        lock_nonce: job.lock_nonce.unwrap() as i64,
                        scheduled_at: job.scheduled_at.as_utc().unix_timestamp(),
                        method: job.method,
                        url: job.url,
                        headers: job
                            .headers
                            .iter()
                            .filter_map(|s| {
                                let mut parts = s.splitn(2, ":");
                                let key = parts.next()?.trim().to_string();
                                let value = parts.next()?.trim().to_string();
                                Some((key, value))
                            })
                            .collect(),
                        body: job.body,
                        timeout_ms: job.timeout_ms,
                        max_response_bytes: job.max_response_bytes as i64,
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
        let mut jobs = req.into_inner();
        while let Some(job_execution) = jobs.next().await {
            dbg!(job_execution)?;
        }

        Ok(tonic::Response::new(Empty {}))
    }
}

async fn run_cleanup(pool: Pool<Postgres>) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        println!("Running broker cleanup");
        sqlx::query!(
            r#"
          UPDATE scheduled_jobs
          SET lock_nonce = NULL
          WHERE lock_nonce IS NOT NULL
            AND to_timestamp(lock_nonce) + (timeout_ms / 1000 || ' seconds')::interval
                < now() - interval '30 seconds';
          "#
        )
        .execute(&pool)
        .await?;
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    let addr = format!("[::1]:{}", config.port).parse()?;

    let cleanup_fut = run_cleanup(config.pool.clone());

    let broker = Broker { pool: config.pool };

    let svc = BrokerServer::new(broker);

    let server_fut = Server::builder().add_service(svc).serve(addr);

    select! {
      server_res = server_fut => {server_res?;},
      cleanup_res = cleanup_fut => {cleanup_res?;}
    };

    Ok(())
}
