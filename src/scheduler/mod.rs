mod cron;
mod key_rotate;
mod one_off;
mod retention;
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
        cron::CronScheduler,
        key_rotate::KeyRotationScheduler,
        one_off::OneOffScheduler,
        retention::{one_off::OneOffPastRetention, scheduled::ScheduledPastRetention},
        retries::RetryScheduler,
        tenants::TenantScheduler,
    },
    secrets::KeyRing,
};

#[derive(Debug, Clone)]
pub struct Config {
    pool: Pool<Postgres>,
    cron_count: usize,
    tenant_count: usize,
    one_off_count: usize,
    retry_count: usize,
    past_retention_count: usize,
    key_rotation_count: usize,
    key_ring: KeyRing,
}

impl Config {
    pub async fn from_cli(options: SchedulerOptions, pool: Pool<Postgres>) -> Self {
        Self {
            pool,
            cron_count: options.cron_schedulers,
            tenant_count: options.tenant_schedulers,
            one_off_count: options.one_off_schedulers,
            retry_count: options.retry_schedulers,
            past_retention_count: options.past_retention_schedulers,
            key_rotation_count: options.key_rotation_schedulers,
            key_ring: options.key_ring,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerContext {
    pool: Pool<Postgres>,
    key_ring: KeyRing,
}

#[async_trait::async_trait]
pub trait Scheduler {
    const WAIT: Duration = Duration::ZERO;
    const IDLE_DELAY: Duration = Duration::from_secs(3);
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()>;
}

async fn scheduling_loop<S: Scheduler>(ctx: &SchedulerContext) -> anyhow::Result<()> {
    let mut reached_end = false;

    loop {
        S::run_once(ctx, &mut reached_end).await?;
        if reached_end {
            reached_end = false;
            tokio::time::sleep(S::IDLE_DELAY).await;
        }
    }
}

async fn run_multiple<S: Scheduler>(ctx: &SchedulerContext, count: usize) -> anyhow::Result<()> {
    tokio::time::sleep(S::WAIT).await;

    let mut tasks = FuturesUnordered::new();

    for _ in 0..count {
        let ctx = ctx.clone();
        tasks.push(tokio::spawn(
            async move { scheduling_loop::<S>(&ctx).await },
        ));
    }

    if let Some(join_result) = tasks.next().await {
        let inner = join_result?;

        inner?;
    }

    Ok(())
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    let context = SchedulerContext {
        pool: config.pool.clone(),
        key_ring: config.key_ring.clone(),
    };

    let one_off_jobs_sched = run_multiple::<OneOffScheduler>(&context, config.one_off_count);
    let cron_jobs_sched = run_multiple::<CronScheduler>(&context, config.cron_count);
    let retry_jobs_sched = run_multiple::<RetryScheduler>(&context, config.retry_count);
    let tenant_jobs_sched = run_multiple::<TenantScheduler>(&context, config.tenant_count);
    let past_retention_jobs_sched =
        run_multiple::<ScheduledPastRetention>(&context, config.past_retention_count);
    let past_retention_jobs_one_off =
        run_multiple::<OneOffPastRetention>(&context, config.past_retention_count);
    let key_rotation_jobs_sched =
        run_multiple::<KeyRotationScheduler>(&context, config.key_rotation_count);

    select! {
      res = one_off_jobs_sched => res?,
      res = cron_jobs_sched => res?,
      res = retry_jobs_sched => res?,
      res = tenant_jobs_sched => res?,
      res = past_retention_jobs_sched => res?,
      res = past_retention_jobs_one_off => res?,
      res = key_rotation_jobs_sched => res?,
    }

    Ok(())
}
