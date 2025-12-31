use std::time::Duration;

use sqlx::{Pool, Postgres};

use crate::scheduler::{Scheduler, SchedulerContext};

#[derive(Clone, Copy)]
pub struct OneOffPastRetention;

#[async_trait::async_trait]
impl Scheduler for OneOffPastRetention {
    const WAIT: Duration = Duration::from_mins(5);

    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        let mut tx = ctx.pool.begin().await?;

        let one_off_job = sqlx::query!(
            r#"
          WITH candidates AS (
            SELECT
              one_off.id
            FROM one_off_jobs one_off
            JOIN scheduled_jobs sched
              ON sched.one_off_job_id = one_off.id
            WHERE one_off.deleted_at IS NULL
              AND NOT EXISTS (
                SELECT 1
                FROM scheduled_jobs sched_2
                WHERE sched_2.one_off_job_id = one_off.id
                  AND (
                    sched_2.deleted_at IS NULL OR
                    sched_2.deleted_At >= now() - interval '3 hours'
                  )
              )
              GROUP BY one_off.id, one_off.tenant_id
              LIMIT 10
          )
          SELECT
            one_off.id as id,
            one_off.request_id as req_id
          FROM one_off_jobs one_off
          JOIN candidates c
            ON c.id = one_off.id
          LIMIT 1 FOR UPDATE SKIP LOCKED;
            "#
        )
        .fetch_optional(&mut *tx)
        .await?;

        if one_off_job.is_none() {
            *reached_end = true;
            return Ok(());
        }

        let job = one_off_job.unwrap();

        sqlx::query!(
            r#"
        UPDATE one_off_jobs
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

        tx.commit().await?;

        Ok(())
    }
}
