use std::time::Duration;

use sqlx::{Pool, Postgres};

use crate::scheduler::Scheduler;

#[derive(Clone, Copy)]
pub struct ScheduledPastRetention;

#[async_trait::async_trait]
impl Scheduler for ScheduledPastRetention {
    const WAIT: Duration = Duration::from_mins(5);

    async fn run_once(pool: &Pool<Postgres>, reached_end: &mut bool) -> anyhow::Result<()> {
        let mut tx = pool.begin().await?;

        let scheduled_job = sqlx::query!(
            r#"
        SELECT
          job.id as id,
          exec.request_id as req_id,
          exec.response_id as res_id
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

        sqlx::query!(
            r#"
        UPDATE scheduled_jobs
        SET deleted_at = now()
        WHERE id = $1
        "#,
            job.id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
          UPDATE http_requests
          SET
            body = '<deleted>',
            headers = '{}'
          WHERE id = $1
          "#,
            job.req_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
          UPDATE http_responses
          SET
            body = '<deleted>',
            headers = '{}'
          WHERE id = $1
          "#,
            job.res_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}
