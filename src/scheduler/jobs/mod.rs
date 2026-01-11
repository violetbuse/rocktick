use tokio::task::JoinHandle;

use crate::scheduler::{Config, SchedulerContext, spawn_scheduler};

mod cron;
mod one_off;
mod retries;

pub fn get_job_schedulers(
    ctx: &SchedulerContext,
    config: &Config,
) -> Vec<JoinHandle<anyhow::Result<()>>> {
    vec![
        spawn_scheduler::<cron::CronScheduler>(ctx, config.cron_count),
        spawn_scheduler::<one_off::OneOffScheduler>(ctx, config.one_off_count),
        spawn_scheduler::<retries::RetryScheduler>(ctx, config.retry_count),
    ]
}
