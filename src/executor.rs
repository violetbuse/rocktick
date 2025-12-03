use std::time::Duration;

use sqlx::{Pool, Postgres};

use crate::ExecutorOptions;

#[derive(Debug, Clone)]
pub struct Config {
    pool: Pool<Postgres>,
}

impl Config {
    pub async fn from_cli(_options: ExecutorOptions, pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

pub async fn start(_config: Config) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_secs(rand::random_range(0..10))).await;

    loop {
        println!("Executor tick running.");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
