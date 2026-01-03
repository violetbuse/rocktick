use std::time::Duration;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use tonic::Request;

use crate::{
    broker::{DroneCheckinRequest, broker_client::BrokerClient},
    drone::DroneState,
};

async fn check_in(state: &DroneState) -> anyhow::Result<Duration> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;

    let checkin_response = client
        .drone_checkin(Request::new(DroneCheckinRequest {
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
