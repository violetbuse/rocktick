mod actors;
mod dronesync;
mod jobs;
pub mod store;
mod util;
mod workflows;

use std::{net::IpAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::anyhow;
use tokio::{
    select,
    sync::{Mutex, RwLock, mpsc},
};

use crate::{DroneOptions, drone::store::DroneStore, grpc};

#[derive(Debug, Clone)]
pub struct Config {
    broker_url: String,
    region: String,
    id: String,
    ip: IpAddr,
    store_location: PathBuf,
    store_in_memory: bool,
}

impl Config {
    pub async fn from_cli(options: DroneOptions) -> Self {
        Self {
            broker_url: options.broker_url,
            region: options.region,
            id: options.id,
            ip: options.ip,
            store_location: options.store_path,
            store_in_memory: options.store_in_memory,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Drone {
    id: String,
    ip: IpAddr,
    region: String,
}

#[derive(Debug, Clone)]
struct DroneState {
    id: String,
    ip: IpAddr,
    exec_results: Arc<Mutex<Vec<grpc::JobExecution>>>,
    broker_url: String,
    region: String,
    store: store::DroneStore,
    drones: Arc<RwLock<Vec<Drone>>>,
    error_tx: mpsc::Sender<anyhow::Error>,
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_secs(rand::random_range(0..4))).await;

    let (error_tx, mut error_rx) = mpsc::channel(1);

    let store: DroneStore = if config.store_in_memory {
        DroneStore::in_memory(config.store_location.to_str().ok_or(anyhow!(
            "unable to turn store location to string {:?}",
            config.store_location
        ))?)
        .await?
    } else {
        DroneStore::from_filename(config.store_location).await?
    };

    let state = DroneState {
        id: config.id,
        ip: config.ip,
        exec_results: Arc::new(Mutex::new(Vec::new())),
        broker_url: config.broker_url.clone(),
        region: config.region.clone(),
        store,
        drones: Arc::new(RwLock::new(Vec::new())),
        error_tx,
    };

    select! {
      jobs_res = jobs::start_job_executor(state.clone()) => {jobs_res?;},
      workflows_res = workflows::start_workflow_executor(state.clone()) => {workflows_res?;},
      actor_res = actors::start_actor_executor(state.clone()) => {actor_res?;},
      checkin_res = dronesync::start_checkin_loop(state.clone()) => {checkin_res?;},
      drone_refresh_res = dronesync::start_refresh_loop(state.clone()) => {drone_refresh_res?;},
      Some(err) = error_rx.recv() => {
        return Err(err);
      }
    }

    Ok(())
}
