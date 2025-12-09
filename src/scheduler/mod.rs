mod cron;
mod one_off;
mod retries;
mod tenants;

use std::time::Duration;

use futures::stream::FuturesUnordered;
use sqlx::{Pool, Postgres};
use tokio::select;
use tokio_stream::StreamExt;

use crate::{
    SchedulerOptions,
    scheduler::{
        cron::CronScheduler, one_off::OneOffScheduler, retries::RetryScheduler,
        tenants::TenantScheduler,
    },
};

#[derive(Debug, Clone)]
pub struct Config {
    pool: Pool<Postgres>,
    cron_count: usize,
    tenant_count: usize,
    one_off_count: usize,
    retry_count: usize,
}

impl Config {
    pub async fn from_cli(options: SchedulerOptions, pool: Pool<Postgres>) -> Self {
        Self {
            pool,
            cron_count: options.cron_schedulers,
            tenant_count: options.tenant_schedulers,
            one_off_count: options.one_off_schedulers,
            retry_count: options.retry_schedulers,
        }
    }
}

#[async_trait::async_trait]
pub trait Scheduler {
    async fn run_once(pool: &Pool<Postgres>, reached_end: &mut bool) -> anyhow::Result<()>;
}

async fn scheduling_loop<S: Scheduler>(pool: &Pool<Postgres>) -> anyhow::Result<()> {
    let mut reached_end = false;

    loop {
        S::run_once(pool, &mut reached_end).await?;
        if reached_end {
            reached_end = false;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}

async fn run_multiple<S: Scheduler>(pool: &Pool<Postgres>, count: usize) -> anyhow::Result<()> {
    let mut tasks = FuturesUnordered::new();

    for _ in 0..count {
        let pool = pool.clone();
        tasks.push(tokio::spawn(
            async move { scheduling_loop::<S>(&pool).await },
        ));
    }

    if let Some(join_result) = tasks.next().await {
        let inner = join_result?;

        inner?;
    }

    Ok(())
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    let one_off_jobs_sched = run_multiple::<OneOffScheduler>(&config.pool, config.one_off_count);
    let cron_jobs_sched = run_multiple::<CronScheduler>(&config.pool, config.cron_count);
    let retry_jobs_sched = run_multiple::<RetryScheduler>(&config.pool, config.retry_count);
    let tenant_jobs_sched = run_multiple::<TenantScheduler>(&config.pool, config.tenant_count);

    select! {
      res = one_off_jobs_sched => res?,
      res = cron_jobs_sched => res?,
      res = retry_jobs_sched => res?,
      res = tenant_jobs_sched => res?,
    }

    Ok(())
}
