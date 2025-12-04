use std::time::Duration;

use tonic::Request;

use crate::{
    ExecutorOptions,
    broker::{GetJobsRequest, broker_client::BrokerClient},
};

#[derive(Debug, Clone)]
pub struct Config {
    broker_url: String,
    region: String,
}

impl Config {
    pub async fn from_cli(options: ExecutorOptions) -> Self {
        Self {
            broker_url: options.broker_url,
            region: options.region,
        }
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_secs(rand::random_range(0..10))).await;

    loop {
        println!(
            "Executor tick running, connecting to {}",
            config.broker_url.clone()
        );

        let mut client = BrokerClient::connect(config.broker_url.clone()).await?;
        let mut jobs_stream = client
            .get_jobs(Request::new(GetJobsRequest {
                region: config.region.clone(),
            }))
            .await?
            .into_inner();

        while let Some(job) = jobs_stream.message().await? {
            dbg!(job);
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
