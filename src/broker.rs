use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{Pool, Postgres};
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
        _req: tonic::Request<GetJobsRequest>,
    ) -> Result<tonic::Response<Self::GetJobsStream>, Status> {
        let (tx, rx) = mpsc::channel(8);

        tokio::spawn(async move {
            let run_to = rand::random_range(1..6);
            for i in 0..run_to {
                tx.send(Ok(JobSpec {
                    job_id: i.to_string(),
                    lock_nonce: rand::random(),
                    scheduled_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64,
                    method: "GET".to_string(),
                    url: "https://example.com/hi".to_string(),
                    headers: HashMap::new(),
                    body: "hiiii".to_string(),
                    timeout_ms: 10_000,
                }))
                .await
                .unwrap();

                tokio::time::sleep(Duration::from_millis(750)).await;
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

pub async fn start(config: Config) -> anyhow::Result<()> {
    let addr = format!("[::1]:{}", config.port).parse()?;

    let broker = Broker { pool: config.pool };

    let svc = BrokerServer::new(broker);

    Server::builder().add_service(svc).serve(addr).await?;

    Ok(())
}
