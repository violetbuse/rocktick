use crate::scheduler::{Scheduler, SchedulerContext};

pub struct PendingExecutionScheduler;

#[async_trait::async_trait]
impl Scheduler for PendingExecutionScheduler {
    #[tracing::instrument(name = "PendingExecutionScheduler::run_once")]
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        todo!("Implement pending execution scheduler");
    }
}
