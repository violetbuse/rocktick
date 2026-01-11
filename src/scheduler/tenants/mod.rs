use tokio::task::JoinHandle;

use crate::scheduler::{Config, SchedulerContext, spawn_scheduler};

mod tokens;

pub fn get_tenant_schedulers(
    ctx: &SchedulerContext,
    config: &Config,
) -> Vec<JoinHandle<anyhow::Result<()>>> {
    vec![spawn_scheduler::<tokens::TenantTokenScheduler>(
        ctx,
        config.tenant_count,
    )]
}
