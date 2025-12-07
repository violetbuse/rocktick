#![allow(dead_code)]

use anyhow::{Ok, anyhow};
use clap::{Parser, Subcommand};
use sqlx::postgres::PgPoolOptions;
use tokio::select;

mod api;
mod broker;
mod executor;
mod id;
mod pg;
mod scheduler;

#[derive(Debug, Clone, Parser)]
#[command(
    version,
    about,
    subcommand_required = false,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(flatten)]
    dev: DevOptions,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Runs the dev server
    Dev(DevOptions),
    /// Runs only the executor service
    Executor(ExecutorOptions),
    /// Runs only the broker service
    Broker(BrokerOptions),
    /// Runs only the scheduler service
    Scheduler(SchedulerOptions),
    /// Runs only the api service
    Api(ApiOptions),
    /// Migrate the postgres database
    Migrate(MigrationOptions),
}

#[derive(Debug, Clone, Parser)]
pub struct DevOptions {
    #[arg(long, default_value_t = 3000)]
    api_port: usize,
    #[arg(long, default_value_t = 30001)]
    broker_port: usize,
    #[arg(long, default_value = "local-1")]
    region: String,
    #[arg(long, env = "DATABASE_URL")]
    postgres_url: Option<String>,
    #[arg(long, default_value_t = true)]
    postgres_temporary: bool,
    #[arg(long, env = "AUTH_KEY")]
    auth_key: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct ExecutorOptions {
    broker_url: String,
    region: String,
}

impl TryFrom<DevOptions> for ExecutorOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            broker_url: format!("http://[::1]:{}", value.broker_port),
            region: value.region,
        })
    }
}

#[derive(Debug, Clone, Parser)]
pub struct BrokerOptions {
    port: usize,
    postgres_url: String,
}

impl TryFrom<DevOptions> for BrokerOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            port: value.broker_port,
            postgres_url: value
                .postgres_url
                .ok_or(anyhow!("No postgres url provided!"))?,
        })
    }
}

#[derive(Debug, Clone, Parser)]
pub struct SchedulerOptions {
    #[arg(long, env = "DATABASE_URL")]
    postgres_url: String,
}

impl TryFrom<DevOptions> for SchedulerOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            postgres_url: value
                .postgres_url
                .ok_or(anyhow!("No postgres url provided!"))?,
        })
    }
}

#[derive(Debug, Clone, Parser)]
pub struct ApiOptions {
    #[arg(long, env = "PORT", default_value_t = 3000)]
    port: usize,
    #[arg(long, env = "VALID_REGIONS")]
    valid_regions: String,
    #[arg(long, env = "DATABASE_URL")]
    postgres_url: String,
    #[arg(long, env = "AUTH_KEY")]
    auth_key: Option<String>,
}

impl TryFrom<DevOptions> for ApiOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            port: value.api_port,
            postgres_url: value
                .postgres_url
                .ok_or(anyhow!("No postgres url provided!"))?,
            valid_regions: value.region,
            auth_key: value.auth_key,
        })
    }
}

#[derive(Debug, Clone, Parser)]
pub struct MigrationOptions {
    #[arg(long, env = "DATABASE_URL")]
    postgres_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv_override();

    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Dev(_)) => {
            let mut dev_options = cli
                .command
                .and_then(|cmd| {
                    if let Commands::Dev(dev_opts) = cmd {
                        Some(dev_opts)
                    } else {
                        None
                    }
                })
                .unwrap_or(cli.dev);

            if dev_options.postgres_url.is_none()
                || dev_options
                    .postgres_url
                    .clone()
                    .is_some_and(|val| val.is_empty())
            {
                let connection_url = pg::run_embedded(dev_options.postgres_temporary).await?;
                let temp_pool = pg::create_pool(connection_url.clone()).await?;
                println!("Migrating database...");
                pg::migrate_pg(&temp_pool).await?;
                dev_options.postgres_url = Some(connection_url)
            }

            let postgres_url = dev_options
                .postgres_url
                .clone()
                .expect("Somehow no postgres url is present.");
            println!("Connecting to {postgres_url}");

            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(&postgres_url)
                .await?;

            let api_config =
                api::Config::from_cli(dev_options.clone().try_into()?, pool.clone()).await;
            let broker_config =
                broker::Config::from_cli(dev_options.clone().try_into()?, pool.clone()).await;
            let executor_config = executor::Config::from_cli(dev_options.clone().try_into()?).await;
            let scheduler_config =
                scheduler::Config::from_cli(dev_options.clone().try_into()?, pool.clone()).await;

            select! {
              api_res = api::start(api_config) => {
                println!("Api Service Stopped.");
                api_res?;
              },
              broker_res = broker::start(broker_config) => {
                println!("Broker Service Stopped.");
                broker_res?;
              },
              executor_res = executor::start(executor_config) => {
                println!("Executor Service Stopped.");
                executor_res?;
              },
              scheduler_res = scheduler::start(scheduler_config) => {
                println!("Scheduler Service Stopped.");
                scheduler_res?;
              },
              _ = tokio::signal::ctrl_c() => println!("Received Ctrl-C.")
            }
        }
        Some(Commands::Api(api_config)) => {
            let pool = pg::create_pool(api_config.postgres_url.clone()).await?;
            let config = api::Config::from_cli(api_config, pool).await;
            api::start(config).await?;
            println!("Api Service Stopped.");
        }
        Some(Commands::Broker(broker_config)) => {
            let pool = pg::create_pool(broker_config.postgres_url.clone()).await?;
            let config = broker::Config::from_cli(broker_config, pool).await;
            broker::start(config).await?;
            println!("Broker Service Stopped.")
        }
        Some(Commands::Executor(executor_config)) => {
            let config = executor::Config::from_cli(executor_config).await;
            executor::start(config).await?;
            println!("Executor Service Stopped.");
        }
        Some(Commands::Scheduler(scheduler_config)) => {
            let pool = pg::create_pool(scheduler_config.postgres_url.clone()).await?;
            let config = scheduler::Config::from_cli(scheduler_config, pool).await;
            scheduler::start(config).await?;
            println!("Scheduler Service Stopped.");
        }
        Some(Commands::Migrate(migrate_config)) => {
            let pool = pg::create_pool(migrate_config.postgres_url).await?;
            pg::migrate_pg(&pool).await?;
        }
    }

    println!("Program stopped.");

    Ok(())
}
