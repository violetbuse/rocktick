#![allow(dead_code)]

use std::{net::IpAddr, sync::OnceLock};

use anyhow::{Ok, anyhow};
use clap::{Parser, Subcommand};
use sqlx::postgres::PgPoolOptions;
use tokio::select;

use crate::secrets::KeyRing;

mod api;
mod broker;
mod drone;
mod id;
mod pg;
mod scheduler;
mod secrets;
mod signing;

#[derive(Debug, Clone)]
pub struct GlobalConfig {
    is_dev: bool,
}

pub static GLOBAL_CONFIG: OnceLock<GlobalConfig> = OnceLock::new();

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

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum Commands {
    /// Runs the dev server.
    Dev(DevOptions),
    /// Runs the full server (broker, scheduler, and API)
    /// in one process.
    Server(ServerOptions),
    /// Runs only the broker service.
    Broker(BrokerOptions),
    /// Runs only the scheduler service.
    Scheduler(SchedulerOptions),
    /// Runs only the api service.
    Api(ApiOptions),
    /// Runs only the drone service.
    Drone(DroneOptions),
    /// Migrate the postgres database.
    Migrate(MigrationOptions),
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct DevOptions {
    #[arg(long, default_value_t = 9090)]
    api_port: usize,
    #[arg(long, default_value_t = 30001)]
    broker_port: usize,
    #[arg(long, default_value = "na-east")]
    /// The region the executor will run in.
    region: String,
    #[arg(
      long,
      env = "VALID_REGIONS",
      num_args = 1,
      value_delimiter = ',',
      default_values = vec!["na-east", "na-west", "asia-east", "eu-west"]
    )]
    /// The regions accepted by the api. If you define
    /// this, remember to include the --region parameter.
    valid_regions: Vec<String>,
    #[arg(long, env = "ROCKTICK_PG")]
    postgres_url: Option<String>,
    #[arg(long, env = "POOL_SIZE", default_value_t = 5)]
    pool_size: u32,
    #[arg(long, default_value_t = true)]
    postgres_temporary: bool,
    #[arg(long, env = "AUTH_KEY")]
    auth_key: Option<String>,
    #[arg(
        long,
        env = "ROCKTICK_SIGNING_KEY",
        default_value = "signature_00000000"
    )]
    signing_key: String,
    #[arg(long, value_parser, env = "ROCKTICK_KEY_RING")]
    key_ring: Option<KeyRing>,
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct ServerOptions {
    #[arg(long, env = "ROCKTICK_PG")]
    postgres_url: String,
    #[arg(long, env = "POOL_SIZE", default_value_t = 50)]
    pool_size: u32,
    #[arg(long, env = "API_PORT", default_value_t = 3000)]
    api_port: usize,
    #[arg(long, env = "API_HOSTNAME", default_value = "[::0]")]
    api_hostname: String,
    #[arg(long, env = "VALID_REGIONS", num_args = 1, value_delimiter = ',')]
    valid_regions: Vec<String>,
    #[arg(long, env = "AUTH_KEYS", num_args = 1, value_delimiter = ',')]
    /// A comma separated string of auth keys
    auth_keys: Vec<String>,
    #[arg(long, default_value_t = 30001, env = "BROKER_PORT")]
    broker_port: usize,
    #[arg(long, default_value = "[::0]", env = "BROKER_HOSTNAME")]
    broker_hostname: String,
    #[arg(long, value_parser, env = "KEY_RING")]
    key_ring: KeyRing,
    #[arg(long, env = "FALLBACK_SIGNING_KEY")]
    fallback_signing_key: String,
    #[arg(long, default_value_t = 2, env = "CRON_SCHEDULER_COUNT")]
    cron_schedulers: usize,
    #[arg(long, default_value_t = 2, env = "TENANT_SCHEDULER_COUNT")]
    tenant_schedulers: usize,
    #[arg(long, default_value_t = 2, env = "ONE_OFF_SCHEDULER_COUNT")]
    one_off_schedulers: usize,
    #[arg(long, default_value_t = 2, env = "RETRY_SCHEDULER_COUNT")]
    retry_schedulers: usize,
    #[arg(long, default_value_t = 2, env = "PAST_RETENTION_SCHEDULER_COUNT")]
    past_retention_schedulers: usize,
    #[arg(long, default_value_t = 2, env = "KEY_ROTATION_SCHEDULER_COUNT")]
    key_rotation_schedulers: usize,
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct BrokerOptions {
    #[arg(long, default_value_t = 30001, env = "BROKER_PORT")]
    port: usize,
    #[arg(long, default_value = "[::0]", env = "BROKER_HOSTNAME")]
    hostname: String,
    #[arg(long, env = "ROCKTICK_PG")]
    postgres_url: String,
    #[arg(long, env = "POOL_SIZE", default_value_t = 20)]
    pool_size: u32,
    #[arg(long, value_parser, env = "KEY_RING")]
    key_ring: KeyRing,
    #[arg(long, env = "FALLBACK_SIGNING_KEY")]
    fallback_signing_key: String,
}

impl TryFrom<DevOptions> for BrokerOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            port: value.broker_port,
            hostname: "[::0]".to_string(),
            postgres_url: value
                .postgres_url
                .ok_or(anyhow!("No postgres url provided!"))?,
            pool_size: value.pool_size,
            key_ring: value.key_ring.unwrap_or(KeyRing::dev()),
            fallback_signing_key: value.signing_key,
        })
    }
}

impl From<ServerOptions> for BrokerOptions {
    fn from(value: ServerOptions) -> Self {
        Self {
            port: value.broker_port,
            hostname: value.broker_hostname,
            postgres_url: value.postgres_url,
            pool_size: value.pool_size,
            key_ring: value.key_ring,
            fallback_signing_key: value.fallback_signing_key,
        }
    }
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct SchedulerOptions {
    #[arg(long, env = "ROCKTICK_PG")]
    postgres_url: String,
    #[arg(long, env = "POOL_SIZE", default_value_t = 20)]
    pool_size: u32,
    #[arg(long, default_value_t = 1, env = "CRON_SCHEDULER_COUNT")]
    cron_schedulers: usize,
    #[arg(long, default_value_t = 1, env = "TENANT_SCHEDULER_COUNT")]
    tenant_schedulers: usize,
    #[arg(long, default_value_t = 1, env = "ONE_OFF_SCHEDULER_COUNT")]
    one_off_schedulers: usize,
    #[arg(long, default_value_t = 1, env = "RETRY_SCHEDULER_COUNT")]
    retry_schedulers: usize,
    #[arg(long, default_value_t = 1, env = "PAST_RETENTION_SCHEDULER_COUNT")]
    past_retention_schedulers: usize,
    #[arg(long, default_value_t = 1, env = "KEY_ROTATION_SCHEDULER_COUNT")]
    key_rotation_schedulers: usize,
    #[arg(long, value_parser, env = "KEY_RING")]
    key_ring: KeyRing,
}

impl TryFrom<DevOptions> for SchedulerOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            postgres_url: value
                .postgres_url
                .ok_or(anyhow!("No postgres url provided!"))?,
            pool_size: value.pool_size,
            cron_schedulers: 1,
            tenant_schedulers: 1,
            one_off_schedulers: 1,
            retry_schedulers: 1,
            past_retention_schedulers: 1,
            key_rotation_schedulers: 1,
            key_ring: value.key_ring.unwrap_or(KeyRing::dev()),
        })
    }
}

impl From<ServerOptions> for SchedulerOptions {
    fn from(value: ServerOptions) -> Self {
        Self {
            postgres_url: value.postgres_url,
            pool_size: value.pool_size,
            cron_schedulers: value.cron_schedulers,
            tenant_schedulers: value.tenant_schedulers,
            one_off_schedulers: value.one_off_schedulers,
            retry_schedulers: value.retry_schedulers,
            past_retention_schedulers: value.past_retention_schedulers,
            key_rotation_schedulers: value.key_rotation_schedulers,
            key_ring: value.key_ring,
        }
    }
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct ApiOptions {
    #[arg(long, env = "PORT", default_value_t = 3000)]
    port: usize,
    #[arg(long, env = "HOSTNAME", default_value = "[::0]")]
    hostname: String,
    #[arg(long, env = "VALID_REGIONS", num_args = 1, value_delimiter = ',')]
    valid_regions: Vec<String>,
    #[arg(long, env = "ROCKTICK_PG")]
    postgres_url: String,
    #[arg(long, env = "POOL_SIZE", default_value_t = 20)]
    pool_size: u32,
    #[arg(long, env = "AUTH_KEYS", num_args = 1, value_delimiter = ',')]
    /// A comma separated string of auth keys
    auth_keys: Option<Vec<String>>,
    #[arg(long, value_parser, env = "KEY_RING")]
    key_ring: KeyRing,
}

impl TryFrom<DevOptions> for ApiOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            port: value.api_port,
            hostname: "[::0]".to_string(),
            postgres_url: value
                .postgres_url
                .ok_or(anyhow!("No postgres url provided!"))?,
            pool_size: value.pool_size,
            valid_regions: value.valid_regions,
            auth_keys: value.auth_key.map(|s| vec![s]),
            key_ring: value.key_ring.unwrap_or(KeyRing::dev()),
        })
    }
}

impl From<ServerOptions> for ApiOptions {
    fn from(value: ServerOptions) -> Self {
        Self {
            port: value.api_port,
            hostname: value.api_hostname,
            valid_regions: value.valid_regions,
            postgres_url: value.postgres_url,
            pool_size: value.pool_size,
            auth_keys: Some(value.auth_keys),
            key_ring: value.key_ring,
        }
    }
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct DroneOptions {
    #[arg(long, default_value = "http://[::1]:30001", env = "BROKER_URL")]
    broker_url: String,
    #[arg(long, env = "EXECUTOR_REGION")]
    region: String,
    #[arg(long, env = "DRONE_ID")]
    id: String,
    #[arg(long, value_parser, env = "DRONE_IP_ADDR")]
    ip: IpAddr,
}

impl TryFrom<DevOptions> for DroneOptions {
    type Error = anyhow::Error;

    fn try_from(value: DevOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            broker_url: format!("http://[::1]:{}", value.broker_port),
            region: value.region,
            id: "dev-drone".to_string(),
            ip: "127.0.0.1"
                .parse()
                .expect("127.0.0.1 is not a valid ip apparently???"),
        })
    }
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
pub struct MigrationOptions {
    #[arg(long, env = "DATABASE_URL")]
    postgres_url: String,
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let is_dev = cli.command.is_none() || matches!(cli.command, Some(Commands::Dev(_)));

    let global_config = GlobalConfig { is_dev };

    GLOBAL_CONFIG
        .set(global_config)
        .expect("Failed to set global config");

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
                let temp_pool =
                    pg::create_pool(connection_url.clone(), dev_options.pool_size).await?;
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
            let executor_config = drone::Config::from_cli(dev_options.clone().try_into()?).await;
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
              executor_res = drone::start(executor_config) => {
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
        Some(Commands::Server(server_config)) => {
            let pool = pg::create_pool(server_config.postgres_url.clone(), server_config.pool_size)
                .await?;

            let api_config =
                api::Config::from_cli(server_config.clone().into(), pool.clone()).await;
            let broker_config =
                broker::Config::from_cli(server_config.clone().into(), pool.clone()).await;
            let scheduler_config =
                scheduler::Config::from_cli(server_config.clone().into(), pool.clone()).await;

            select! {
              api_res = api::start(api_config) => {
                println!("Api Service Stopped.");
                api_res?;
              },
              broker_res = broker::start(broker_config) => {
                println!("Broker Service Stopped.");
                broker_res?;
              },
              scheduler_res = scheduler::start(scheduler_config) => {
                println!("Scheduler Service Stopped.");
                scheduler_res?;
              },
              _ = tokio::signal::ctrl_c() => println!("Received Ctrl-C.")
            }
        }
        Some(Commands::Api(api_config)) => {
            let pool =
                pg::create_pool(api_config.postgres_url.clone(), api_config.pool_size).await?;
            let config = api::Config::from_cli(api_config, pool).await;
            api::start(config).await?;
            println!("Api Service Stopped.");
        }
        Some(Commands::Broker(broker_config)) => {
            let pool = pg::create_pool(broker_config.postgres_url.clone(), broker_config.pool_size)
                .await?;
            let config = broker::Config::from_cli(broker_config, pool).await;
            broker::start(config).await?;
            println!("Broker Service Stopped.")
        }
        Some(Commands::Scheduler(scheduler_config)) => {
            let pool = pg::create_pool(
                scheduler_config.postgres_url.clone(),
                scheduler_config.pool_size,
            )
            .await?;
            let config = scheduler::Config::from_cli(scheduler_config, pool).await;
            scheduler::start(config).await?;
            println!("Scheduler Service Stopped.");
        }
        Some(Commands::Drone(executor_config)) => {
            let config = drone::Config::from_cli(executor_config).await;
            drone::start(config).await?;
            println!("Executor Service Stopped.");
        }
        Some(Commands::Migrate(migrate_config)) => {
            let pool = pg::create_pool(migrate_config.postgres_url, 2).await?;
            pg::migrate_pg(&pool).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv_override();

    let result = run().await;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
    } else {
        println!("Program stopped.");
    }

    Ok(())
}
