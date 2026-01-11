use tokio::task::JoinHandle;

use crate::scheduler::{Config, SchedulerContext, spawn_scheduler};

mod one_off;
mod scheduled;

pub fn get_retention_schedulers(
    ctx: &SchedulerContext,
    config: &Config,
) -> Vec<JoinHandle<anyhow::Result<()>>> {
    vec![
        spawn_scheduler::<one_off::OneOffPastRetention>(ctx, config.past_retention_count),
        spawn_scheduler::<scheduled::ScheduledPastRetention>(ctx, config.past_retention_count),
    ]
}
