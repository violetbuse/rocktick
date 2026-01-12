use std::time::Duration;

use crate::{
    scheduler::{Scheduler, SchedulerContext},
    util::scheduled_jobs::delete_scheduled_job,
};

#[derive(Clone, Copy)]
pub struct ScheduledPastRetention;

#[async_trait::async_trait]
impl Scheduler for ScheduledPastRetention {
    const WAIT: Duration = Duration::from_mins(5);

    #[tracing::instrument(name = "ScheduledPastRetention::run_once")]
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        let mut tx = ctx.pool.begin().await?;

        let scheduled_job = sqlx::query!(
            r#"
        SELECT
          job.id as id
        FROM scheduled_jobs job
        JOIN job_executions exec
          ON exec.id = job.execution_id
        JOIN tenants tenant
          ON tenant.id = job.tenant_id
        WHERE
          job.execution_id IS NOT NULL AND
          job.tenant_id IS NOT NULL AND
          job.deleted_at IS NULL AND
          exec.executed_at < now() - (tenant.retain_for_days * interval '1 day')
        LIMIT 1 FOR UPDATE SKIP LOCKED;
        "#
        )
        .fetch_optional(&mut *tx)
        .await?;

        if scheduled_job.is_none() {
            *reached_end = true;
            return Ok(());
        }

        let job = scheduled_job.unwrap();

        delete_scheduled_job(job.id, &mut tx).await?;

        tx.commit().await?;

        Ok(())
    }
}
