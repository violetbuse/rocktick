use std::{net::IpAddr, time::Duration};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use tonic::Request;

use crate::{
    drone::{Drone, DroneState},
    grpc::{self, broker_client::BrokerClient},
};

async fn check_in(state: &DroneState) -> anyhow::Result<Duration> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;

    let checkin_response = client
        .drone_checkin(Request::new(grpc::DroneCheckinRequest {
            drone_id: state.id.clone(),
            drone_ip: state.ip.to_string(),
            drone_region: state.region.clone(),
            drone_time_ms: Utc::now().timestamp_millis(),
        }))
        .await?
        .into_inner();

    let checkin_time = DateTime::from_timestamp_millis(checkin_response.checkin_again_at)
        .ok_or(anyhow!("drone checkin returned faulty checkin again time"))?;
    let time_until_checkin = checkin_time - Utc::now();

    Ok(time_until_checkin.to_std().unwrap_or(Duration::ZERO))
}

pub async fn start_checkin_loop(state: DroneState) -> anyhow::Result<()> {
    let mut time_to_next_checkin = check_in(&state).await?;

    loop {
        tokio::time::sleep(time_to_next_checkin).await;
        time_to_next_checkin = check_in(&state).await?;
    }
}

async fn refresh_drones(state: &DroneState) -> anyhow::Result<()> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;

    let mut drones_stream = client
        .get_drones(Request::new(grpc::GetDronesRequest {
            drone_id: state.id.clone(),
        }))
        .await?
        .into_inner();

    let mut drones = Vec::new();

    while let Ok(Some(drone)) = drones_stream.message().await {
        let ip: Result<IpAddr, _> = drone.ip.parse();

        if let Ok(ip) = ip {
            drones.push(Drone {
                id: drone.id,
                ip,
                region: drone.region,
            });
        }
    }

    let mut guard = state.drones.write().await;
    *guard = drones;
    drop(guard);

    Ok(())
}

pub async fn start_refresh_loop(state: DroneState) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        refresh_drones(&state).await?;
    }
}
