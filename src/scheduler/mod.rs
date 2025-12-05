mod cron;
mod one_off;
mod retries;
mod tenants;

use sqlx::{Pool, Postgres};
use tokio::select;

use crate::SchedulerOptions;

#[derive(Debug, Clone)]
pub struct Config {
    pool: Pool<Postgres>,
}

impl Config {
    pub async fn from_cli(_options: SchedulerOptions, pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    select! {
      one_off_res = one_off::scheduling_loop(config.pool.clone()) => {one_off_res?;},
      cron_res = cron::scheduling_loop(config.pool.clone()) => {cron_res?;},
      retry_res = retries::scheduling_loop(config.pool.clone()) => {retry_res?;},
      tenant_res = tenants::scheduling_loop(config.pool.clone()) => {tenant_res?;},
    }

    Ok(())
}
