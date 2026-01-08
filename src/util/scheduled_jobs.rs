use sqlx::{Postgres, Transaction};

use crate::util::{http_requests::delete_http_req, http_responses::delete_http_resp};

pub async fn delete_scheduled_job(
    id: String,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    let scheduled_job = sqlx::query!(
        r#"
  SELECT
    job.id as id,
    exec.request_id as req_id,
    exec.response_id as res_id
  FROM scheduled_jobs job
  JOIN job_executions exec
    ON exec.id = job.execution_id
  WHERE
    job.id = $1
  "#,
        id
    )
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query!(
        r#"
      UPDATE scheduled_jobs
      SET deleted_at = now()
      WHERE id = $1
      "#,
        scheduled_job.id
    )
    .execute(&mut **tx)
    .await?;

    delete_http_req(scheduled_job.req_id, tx).await?;

    if let Some(job_res_id) = scheduled_job.res_id {
        delete_http_resp(job_res_id, tx).await?;
    }

    Ok(())
}
