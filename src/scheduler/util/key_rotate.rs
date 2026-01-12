use crate::{
    scheduler::{Scheduler, SchedulerContext},
    secrets::Secret,
};

#[derive(Clone, Copy)]
pub struct KeyRotationScheduler;

#[async_trait::async_trait]
impl Scheduler for KeyRotationScheduler {
    #[tracing::instrument(name = "KeyRotationScheduler::run_once")]
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        let latest_key = ctx.key_ring.max();

        let master_key_id = latest_key.id;

        let mut tx = ctx.pool.begin().await?;

        let late_secret = sqlx::query!(
            r#"
          SELECT id FROM secrets
          WHERE master_key_id < $1
          LIMIT 1 FOR UPDATE SKIP LOCKED;
          "#,
            master_key_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if late_secret.is_none() {
            *reached_end = true;
            return Ok(());
        }

        let secret = Secret::get(&late_secret.unwrap().id, &mut *tx).await?;

        let rotated = secret.rotate(&ctx.key_ring)?;

        rotated.put(&mut *tx).await?;

        tx.commit().await?;

        Ok(())
    }
}
