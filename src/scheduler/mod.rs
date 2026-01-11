mod jobs;
mod retention;
mod tenants;
mod util;
mod workflow;

use std::time::Duration;

use futures::future::try_join_all;
use sqlx::{Pool, Postgres};
use tokio::task::JoinHandle;

use crate::{SchedulerOptions, secrets::KeyRing};

#[derive(Debug, Clone)]
pub struct Config {
    pool: Pool<Postgres>,
    cron_count: usize,
    tenant_count: usize,
    one_off_count: usize,
    retry_count: usize,
    past_retention_count: usize,
    key_rotation_count: usize,
    workflow_count: usize,
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
            workflow_count: options.workflow_schedulers,
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

    let mut tasks = Vec::new();

    for _ in 0..count {
        let ctx = ctx.clone();
        tasks.push(tokio::spawn(
            async move { scheduling_loop::<S>(&ctx).await },
        ));
    }

    try_join_all(tasks).await?;

    Ok(())
}

pub fn spawn_scheduler<S: Scheduler>(
    ctx: &SchedulerContext,
    count: usize,
) -> JoinHandle<anyhow::Result<()>> {
    let ctx = ctx.clone();
    tokio::spawn(async move { run_multiple::<S>(&ctx, count).await })
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    let ctx = SchedulerContext {
        pool: config.pool.clone(),
        key_ring: config.key_ring.clone(),
    };

    let mut all_tasks = Vec::new();
    all_tasks.extend(jobs::get_job_schedulers(&ctx, &config));
    all_tasks.extend(retention::get_retention_schedulers(&ctx, &config));
    all_tasks.extend(tenants::get_tenant_schedulers(&ctx, &config));
    all_tasks.extend(util::get_util_schedulers(&ctx, &config));
    all_tasks.extend(workflow::get_workflow_schedulers(&ctx, &config));

    try_join_all(all_tasks).await?;

    Ok(())
}
