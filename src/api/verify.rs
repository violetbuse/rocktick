use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use http::StatusCode;
use serde::Serialize;
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{ApiError, Context, TenantId};

#[derive(Debug, Serialize, ToSchema)]
struct JobVerification {
    verified: bool,
    /// The hex digest of the request body, or null
    /// if there isn't one for the request.
    hash: Option<String>,
}

impl IntoResponse for JobVerification {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, axum::Json(self)).into_response()
    }
}

#[utoipa::path(
  get,
  description = "Verify that a request originates from rocktick.",
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
    TenantId(tenant_id): TenantId,
) -> Result<JobVerification, ApiError> {
    let job = sqlx::query!(
        r#"
    SELECT
      job.id,
      req.body
    FROM scheduled_jobs as job
    INNER JOIN http_requests as req
      ON req.id = job.request_id
    WHERE job.id = $1
      AND lock_nonce IS NOT NULL
      AND ($2::text IS NULL OR $2 = tenant_id);"#,
        job_id,
        tenant_id
    )
    .fetch_optional(&ctx.pool)
    .await;

    if let Err(sql_err) = job {
        eprintln!("Error while trying to verify request {job_id}: {sql_err:?}");
        return Err(ApiError::internal_server_error(None));
    }

    let job = job.unwrap();

    if job.is_none() {
        return Ok(JobVerification {
            verified: false,
            hash: None,
        });
    }

    let job = job.unwrap();

    let hash = job.body.map(Sha256::digest).map(hex::encode);

    Ok(JobVerification {
        verified: true,
        hash,
    })
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new().routes(routes!(verify_request))
}
