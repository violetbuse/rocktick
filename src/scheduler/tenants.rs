use std::time::Duration;

use chrono::TimeDelta;
use sqlx::{Pool, Postgres};

async fn schedule_tenant_token_increase(
    pool: &Pool<Postgres>,
    reached_end: &mut bool,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    let tenant_to_increase = sqlx::query!(
        r#"
      SELECT *
      FROM tenants
      WHERE next_increment < now()
      LIMIT 1 FOR UPDATE SKIP LOCKED;
      "#
    )
    .fetch_optional(&mut *tx)
    .await?;

    if tenant_to_increase.is_none() {
        *reached_end = true;
        return Ok(());
    }

    let tenant = tenant_to_increase.unwrap();

    let new_tokens = (tenant.tokens + tenant.increment).min(tenant.max_tokens);
    let period = tenant.period;
    let next_time = tenant.next_increment
        + TimeDelta::hours(24 * period.days as i64)
        + TimeDelta::microseconds(period.microseconds);

    sqlx::query!(
        r#"
      UPDATE tenants
      SET tokens = $2, next_increment = $3
      WHERE id = $1;
      "#,
        tenant.id,
        new_tokens,
        next_time
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

pub async fn scheduling_loop(pool: Pool<Postgres>) -> anyhow::Result<()> {
    let mut reached_end = false;
    loop {
        schedule_tenant_token_increase(&pool, &mut reached_end).await?;
        if reached_end {
            reached_end = false;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
}
