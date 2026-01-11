mod no_executions;
mod pending_execution;
mod waited_execution;

use tokio::task::JoinHandle;

use crate::scheduler::{Config, SchedulerContext, spawn_scheduler};

pub fn get_workflow_schedulers(
    ctx: &SchedulerContext,
    config: &Config,
) -> Vec<JoinHandle<anyhow::Result<()>>> {
    vec![
        spawn_scheduler::<no_executions::NoExecutionScheduler>(ctx, config.workflow_count),
        spawn_scheduler::<pending_execution::PendingExecutionScheduler>(ctx, config.workflow_count),
        spawn_scheduler::<waited_execution::WaitedExecutionScheduler>(ctx, config.workflow_count),
    ]
}
