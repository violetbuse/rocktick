use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use sqlx::{Pool, Postgres};

use crate::id;

async fn schedule_retry_job(pool: &Pool<Postgres>, reached_end: &mut bool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    let failed_scheduled_job = sqlx::query!(
        r#"
    SELECT
      job.id as id,
      job.region as region,
      job.one_off_job_id as one_off_job_id,
      job.cron_job_id as cron_job_id,
      job.timeout_ms as timeout_ms,
      job.max_retries as max_retries,
      job.max_response_bytes as max_response_bytes,
      job.request_id as request_id,
      job.tenant_id as tenant_id,
      exec.executed_at as executed_at
    FROM scheduled_jobs as job
    INNER JOIN job_executions as exec ON job.execution_id = exec.id
    LEFT JOIN
      scheduled_jobs as pending_retry
      ON job.id = pending_retry.retry_for_id
    WHERE
      exec.success = false AND
      job.max_retries > 0 AND
      job.workflow_id IS NULL AND
      pending_retry.id IS NULL
    LIMIT 1 FOR UPDATE OF job SKIP LOCKED
    "#
    )
    .fetch_optional(&mut *tx)
    .await?;

    if failed_scheduled_job.is_none() {
        *reached_end = true;
        return Ok(());
    }

    let to_retry = failed_scheduled_job.unwrap();

    println!("Scheduling retry for {}", to_retry.id);

    let retry_query = sqlx::query!(
        r#"
      WITH RECURSIVE retry_chain AS (
        SELECT
          id,
          retry_for_id,
          0 AS attempts_made
        FROM
          scheduled_jobs
        WHERE id = $1

        UNION ALL

        SELECT
          s.id,
          s.retry_for_id,
          r.attempts_made + 1
        FROM
          scheduled_jobs s
        JOIN
          retry_chain r ON s.retry_for_id = r.id
      )
      SELECT COALESCE(MAX(attempts_made), 0) as attempts
      FROM retry_chain;
      "#,
        to_retry.id
    )
    .fetch_one(&mut *tx)
    .await?;

    let attempts_made = retry_query.attempts.unwrap();
    let attempts_remaining = to_retry.max_retries - 1;

    let base_delay_ms = 60 * 1000;
    let wait_time = base_delay_ms * (2 ^ attempts_made as u64);
    let next_time = to_retry.executed_at + Duration::from_millis(wait_time);

    let new_job_id = id::generate("scheduled");

    let mut hasher = DefaultHasher::new();
    new_job_id.hash(&mut hasher);
    let full_hash: u64 = hasher.finish();
    let truncated_hash_u32 = (full_hash & 0xFFFFFFFF) as u32;
    let hash = truncated_hash_u32 as i32;

    sqlx::query!(
        r#"
      INSERT INTO scheduled_jobs
        (
          id,
          hash,
          region,
          one_off_job_id,
          cron_job_id,
          retry_for_id,
          tenant_id,
          scheduled_at,
          request_id,
          timeout_ms,
          max_retries,
          max_response_bytes
        )
      VALUES
        (
          $1,
          $2,
          $3,
          $4,
          $5,
          $6,
          $7,
          $8,
          $9,
          $10,
          $11,
          $12
        );
      "#,
        new_job_id,
        hash,
        to_retry.region,
        to_retry.one_off_job_id,
        to_retry.cron_job_id,
        Some(to_retry.id),
        to_retry.tenant_id,
        next_time,
        to_retry.request_id,
        to_retry.timeout_ms,
        attempts_remaining,
        to_retry.max_response_bytes
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

pub async fn scheduling_loop(pool: Pool<Postgres>) -> anyhow::Result<()> {
    let mut reached_end = false;
    loop {
        schedule_retry_job(&pool, &mut reached_end).await?;
        if reached_end {
            reached_end = false;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}
