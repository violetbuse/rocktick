use sqlx::{Pool, Postgres};

use crate::{
    id,
    scheduler::{Scheduler, SchedulerContext},
};

#[derive(Clone, Copy)]
pub struct KeyRotationScheduler;

#[async_trait::async_trait]
impl Scheduler for KeyRotationScheduler {
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        *reached_end = true;
        Ok(())
    }
}
