use std::time::Duration;

use sqlx::{Pool, Postgres};

async fn schedule_cron_job(pool: &Pool<Postgres>, reached_end: &mut bool) -> anyhow::Result<()> {
    println!("Scheduling cron job.");
    *reached_end = true;
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
