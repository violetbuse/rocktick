use sqlx::{Postgres, Transaction};

pub async fn delete_http_req(
    id: String,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
    UPDATE http_requests
    SET
      body = '<del>',
      headers = '{}',
      bytes_used = 0
    WHERE id = $1
    "#,
        id
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}
