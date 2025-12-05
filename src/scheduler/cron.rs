use std::{
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    time::Duration,
};

use chrono::TimeDelta;
use cron::Schedule;
use nanoid::nanoid;
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
          job.created_at as created_at,
          job.start_at as start_at,
          job.request_id as request_id
        FROM
          cron_jobs as job
        JOIN
          unexecuted_job_counts as counts
          ON job.id = counts.cron_id
        WHERE
          counts.unexecuted_count < 60 AND
          job.error IS NULL
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

    let schedule = Schedule::from_str(&cron_job.schedule);

    if let Err(err) = schedule {
        sqlx::query!(
            r#"
        UPDATE cron_jobs
        SET error = $2
        WHERE id = $1;
        "#,
            cron_job.id,
            format!(
                "{} is not a valid cron expression: {:?}",
                cron_job.schedule, err
            )
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(());
    }

    let start_time = latest_scheduled
        .map(|r| r.scheduled_at)
        .unwrap_or(cron_job.created_at)
        .max(cron_job.start_at);

    let schedule = schedule.unwrap();
    let mut count = 0;
    let mut times = Vec::new();
    for datetime in schedule.after(&start_time).take(100) {
        count += 1;
        times.push(datetime);

        let since_start = datetime - start_time;
        if since_start > TimeDelta::seconds(90) && count > 10 {
            break;
        }
    }

    for scheduled_time in times {
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
            cron_job_id,
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
            cron_job.region,
            Some(cron_job.id.clone()),
            scheduled_time,
            cron_job.request_id,
            cron_job.timeout_ms,
            cron_job.max_retries,
            cron_job.max_response_bytes
        )
        .execute(&mut *tx)
        .await?;
    }

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
