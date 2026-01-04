mod actors;
mod checkin;
pub mod grpc;
mod jobs;
mod util;
mod workflows;

use std::{net::IpAddr, sync::Arc, time::Duration};

use tokio::{
    select,
    sync::{Mutex, mpsc},
};

use crate::{DroneOptions, broker::grpc as broker_grpc};

#[derive(Debug, Clone)]
pub struct Config {
    broker_url: String,
    region: String,
    id: String,
    ip: IpAddr,
}

impl Config {
    pub async fn from_cli(options: DroneOptions) -> Self {
        Self {
            broker_url: options.broker_url,
            region: options.region,
            id: options.id,
            ip: options.ip,
        }
    }
}

#[derive(Debug, Clone)]
struct DroneState {
    id: String,
    ip: IpAddr,
    exec_results: Arc<Mutex<Vec<broker_grpc::JobExecution>>>,
    broker_url: String,
    region: String,
    error_tx: mpsc::Sender<anyhow::Error>,
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_secs(rand::random_range(0..4))).await;

    let (error_tx, mut error_rx) = mpsc::channel(1);

    let state = DroneState {
        id: config.id,
        ip: config.ip,
        exec_results: Arc::new(Mutex::new(Vec::new())),
        broker_url: config.broker_url.clone(),
        region: config.region.clone(),
        error_tx,
    };

    select! {
      jobs_res = jobs::start_job_executor(state.clone()) => {jobs_res?;},
      workflows_res = workflows::start_workflow_executor(state.clone()) => {workflows_res?;},
      actor_res = actors::start_actor_executor(state.clone()) => {actor_res?;},
      checkin_res = checkin::start_checkin_loop(state.clone()) => {checkin_res?;},
      Some(err) = error_rx.recv() => {
        return Err(err);
      }
    }

    Ok(())
}
