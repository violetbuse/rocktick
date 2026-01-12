use std::cmp::max;

use chrono::{TimeDelta, Utc};

use crate::scheduler::{Scheduler, SchedulerContext};

#[derive(Clone, Copy)]
pub struct TenantTokenScheduler;

#[async_trait::async_trait]
impl Scheduler for TenantTokenScheduler {
    #[tracing::instrument(name = "TenantTokenScheduler::run_once")]
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        let mut tx = ctx.pool.begin().await?;

        let tenant_to_increase = sqlx::query!(
            r#"
      SELECT *
      FROM tenants
      WHERE
        next_increment < now() AND
        tokens < max_tokens
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

        tracing::info! {
          tenant_id = tenant.id,
          new_tokens,
          "Increasing tenant tokens."
        };

        let period_time_delta =
            TimeDelta::days(period.days as i64) + TimeDelta::microseconds(period.microseconds);

        let time_since_scheduled_increment = Utc::now() - tenant.next_increment;
        let next_time = if new_tokens == tenant.max_tokens {
            Utc::now() + max(TimeDelta::minutes(5), period_time_delta)
        } else if time_since_scheduled_increment > TimeDelta::minutes(5) {
            Utc::now() + period_time_delta
        } else {
            tenant.next_increment + period_time_delta
        };

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
}
