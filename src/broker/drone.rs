use std::net::IpAddr;

use chrono::DateTime;
use replace_err::ReplaceErr;
use sqlx::types::ipnetwork::IpNetwork;
use tonic::Status;

use crate::broker::{BrokerService, DroneCheckinRequest, DroneCheckinResponse};

pub async fn handle_checkin(
    svc: &BrokerService,
    req: tonic::Request<DroneCheckinRequest>,
) -> Result<tonic::Response<DroneCheckinResponse>, Status> {
    let drone_info = req.into_inner();

    let drone_ip: IpAddr = drone_info
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
    .execute(&svc.pool)
    .await
    .replace_err(Status::internal("Unable to upsert drone for some reason."))?;

    let drone_time = DateTime::from_timestamp_millis(drone_info.drone_time_ms)
        .expect("Received invalid time from drone???");

    let report_back_in = drone_time + chrono::Duration::seconds(9);

    Ok(tonic::Response::new(DroneCheckinResponse {
        checkin_again_at: report_back_in.timestamp_millis(),
    }))
}
