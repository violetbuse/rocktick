use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use chrono::DateTime;
use nanoid::nanoid;
use sqlx::{Pool, Postgres};

async fn schedule_one_off_job(pool: &Pool<Postgres>, reached_end: &mut bool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    let job_to_schedule = sqlx::query!(
        r#"
    SELECT
      job.id as id,
      job.region as region,
      job.execute_at as execute_at,
      job.timeout_ms as timeout_ms,
      job.max_retries as max_retries,
      job.max_response_bytes as max_response_bytes,
      job.request_id as request_id
    FROM one_off_jobs as job
    LEFT JOIN
      scheduled_jobs as scheduled
      ON job.id = scheduled.one_off_job_id
    WHERE scheduled.id IS NULL
    LIMIT 1 FOR UPDATE OF job SKIP LOCKED;
    "#
    )
    .fetch_optional(&mut *tx)
    .await?;

    if job_to_schedule.is_none() {
        *reached_end = true;
        return Ok(());
    }

    let to_schedule = job_to_schedule.unwrap();

    println!("Scheduling {}", to_schedule.id);

    let scheduled_time = DateTime::from_timestamp_secs(to_schedule.execute_at);

    let new_job_id = format!("scheduled_{}", nanoid!());

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
          $9
        );
      "#,
        new_job_id,
        hash,
        to_schedule.region,
        Some(to_schedule.id),
        scheduled_time,
        to_schedule.request_id,
        to_schedule.timeout_ms,
        to_schedule.max_retries,
        to_schedule.max_response_bytes
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

pub async fn scheduling_loop(pool: Pool<Postgres>) -> anyhow::Result<()> {
    let mut reached_end = false;
    loop {
        schedule_one_off_job(&pool, &mut reached_end).await?;
        if reached_end {
            reached_end = false;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}
