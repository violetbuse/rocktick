use std::time::Duration;

use sqlx::{Pool, Postgres};

async fn schedule_cron_job(pool: &Pool<Postgres>, reached_end: &mut bool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    let cron_job = sqlx::query!(
        r#"
        WITH unexecuted_job_counts AS (
          SELECT
            sj.cron_job_id as cron_id,
            COUNT(sj.id) AS unexecuted_count
          FROM
            scheduled_jobs as sj
          WHERE
            sj.execution_id IS NULL
          GROUP BY
            sj.cron_job_id
        )
        SELECT
          job.id as id,
          job.region as region,
          job.schedule as schedule,
          job.timeout_ms as timeout_ms,
          job.max_retries as max_retries,
          job.max_response_bytes as max_response_bytes,
          req.method as method,
          req.url as url,
          req.headers as headers,
          req.body as body
        FROM
          cron_jobs as job
        INNER JOIN http_requests as req ON job.request_id = req.id
        JOIN
          unexecuted_job_counts as counts
          ON job.id = counts.cron_id
        WHERE
          counts.unexecuted_count < 60
        LIMIT 1 FOR UPDATE OF job SKIP LOCKED;
        "#
    )
    .fetch_optional(&mut *tx)
    .await?;

    if cron_job.is_none() {
        *reached_end = true;
        return Ok(());
    }

    let cron_job = cron_job.unwrap();

    let latest_scheduled = sqlx::query!(
        r#"
    SELECT * FROM scheduled_jobs
    WHERE cron_job_id = $1 AND retry_for_id IS NULL
    ORDER BY scheduled_at DESC LIMIT 1;
      "#,
        cron_job.id
    )
    .fetch_optional(&mut *tx)
    .await?;

    todo!("Actually schedule the cron job.");

    tx.commit().await?;

    Ok(())
}

pub async fn scheduling_loop(pool: Pool<Postgres>) -> anyhow::Result<()> {
    let mut reached_end = false;
    loop {
        schedule_cron_job(&pool, &mut reached_end).await?;
        if reached_end {
            reached_end = false;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}
