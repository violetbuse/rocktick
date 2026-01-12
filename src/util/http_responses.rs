use sqlx::{Postgres, Transaction};

pub async fn delete_http_resp(
    id: String,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
    UPDATE http_responses
    SET
      body = '',
      headers = '{}',
      deleted_at = now()
    WHERE id = $1
    "#,
        id
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}
