use tokio::task::JoinHandle;

use crate::scheduler::{Config, SchedulerContext, spawn_scheduler};

mod key_rotate;

pub fn get_util_schedulers(
    ctx: &SchedulerContext,
    config: &Config,
) -> Vec<JoinHandle<anyhow::Result<()>>> {
    vec![spawn_scheduler::<key_rotate::KeyRotationScheduler>(
        ctx,
        config.key_rotation_count,
    )]
}
