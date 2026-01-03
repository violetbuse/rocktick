use std::time::Duration;

use crate::drone::DroneState;

pub async fn start_actor_executor(_state: DroneState) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_mins(5)).await;
    }
}
