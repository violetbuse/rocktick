use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{ApiError, Context};

#[derive(Debug, Serialize, ToSchema)]
struct JobVerification {
    verified: bool,
}

impl IntoResponse for JobVerification {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, axum::Json(self)).into_response()
    }
}

/// Verify job
///
/// pass in the value of the 'Rocktick-Job-Id' header and
/// this method will return whether it actually originated
/// from us.
#[utoipa::path(
  get,
  path = "/api/verify/{job_id}",
  params(("job_id", description = "Value of the requests 'Rocktick-Job-Id' header")),
  responses((status = 200, body = JobVerification),
    (status = "4XX", body = ApiError),
    (status = "5XX", body = ApiError)),
  tag = "verify incoming"
)]
async fn verify_request(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
) -> Result<JobVerification, ApiError> {
    let job = sqlx::query!(
        r#"
    SELECT *
    FROM scheduled_jobs
    WHERE id = $1
      AND lock_nonce IS NOT NULL;"#,
        job_id
    )
    .fetch_optional(&ctx.pool)
    .await;

    if let Err(sql_err) = job {
        eprintln!("Error while trying to verify request {job_id}: {sql_err:?}");
        return Err(ApiError::internal_server_error(None));
    }

    let job = job.unwrap();

    if job.is_none() {
        return Ok(JobVerification { verified: false });
    }

    let _job = job.unwrap();

    Ok(JobVerification { verified: true })
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new().routes(routes!(verify_request))
}
