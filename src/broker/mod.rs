mod actor;
mod drone;
pub mod grpc;
mod job;
mod workflow;

use sqlx::{Pool, Postgres};
use tokio::select;
use tonic::Status;
use tonic::transport::Server;

use crate::broker::grpc::broker_server::{Broker as BrokerTrait, BrokerServer};
use crate::secrets::KeyRing;
use crate::{BrokerOptions, GLOBAL_CONFIG};

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
struct BrokerService {
    pub pool: Pool<Postgres>,
    pub key_ring: KeyRing,
    pub fallback_signing_secret: String,
}

#[tonic::async_trait]
impl BrokerTrait for BrokerService {
    async fn drone_checkin(
        &self,
        req: tonic::Request<grpc::DroneCheckinRequest>,
    ) -> Result<tonic::Response<grpc::DroneCheckinResponse>, Status> {
        drone::handle_checkin(self, req).await
    }

    type GetJobsStream = job::GetJobsStream;

    async fn get_jobs(
        &self,
        req: tonic::Request<grpc::GetJobsRequest>,
    ) -> Result<tonic::Response<Self::GetJobsStream>, Status> {
        job::get_jobs(self, req).await
    }

    async fn record_execution(
        &self,
        req: tonic::Request<tonic::Streaming<grpc::JobExecution>>,
    ) -> Result<tonic::Response<grpc::Empty>, Status> {
        job::record_execution(self, req).await
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.hostname, config.port).parse()?;

    if GLOBAL_CONFIG.get().unwrap().is_dev {
        println!("Outgoing Signing Key: {}", &config.fallback_signing_key)
    }

    let job_cleanup_fut = job::run_job_cleanup_loop(config.pool.clone());

    let broker = BrokerService {
        pool: config.pool,
        key_ring: config.key_ring,
        fallback_signing_secret: config.fallback_signing_key,
    };

    let svc = BrokerServer::new(broker);

    let server_fut = Server::builder().add_service(svc).serve(addr);

    select! {
      server_res = server_fut => {server_res?;},
      job_cleanup_res = job_cleanup_fut => {job_cleanup_res?;}
    };

    Ok(())
}
