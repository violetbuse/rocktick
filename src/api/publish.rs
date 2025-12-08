use axum::extract::State;
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    api::{
        ApiError, Context, JsonBody, TenantId,
        models::{OneOffJob, Request},
    },
    id,
};

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct CreateJob {
    region: String,
    execute_at: i64,
    request: Request,
    timeout_ms: Option<i32>,
    max_retries: Option<i32>,
    max_response_bytes: Option<i32>,
}

/// Publish Job
#[utoipa::path(
  post,
  path = "/api/jobs",
  request_body = CreateJob,
  responses(
    (status = 200, description = "Job created", body = OneOffJob),
    (status = "4XX", description = "Bad request", body = ApiError),
    (status = "5XX", description = "Internal server error", body = ApiError)
  ),
  tag = "one off jobs"
)]
async fn create_job(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    JsonBody(create_opts): JsonBody<CreateJob>,
) -> Result<OneOffJob, ApiError> {
    if !ctx.valid_regions.contains(&create_opts.region) {
        let region_list = ctx.valid_regions.join(", ");

        return Err(ApiError::bad_request(Some(&format!(
            "Invalid region: {}, choose one of the following: {}",
            create_opts.region, region_list
        ))));
    }

    let mut txn = ctx.pool.begin().await?;
    let tenant = if let Some(tenant_id) = tenant_id.clone() {
        sqlx::query!("SELECT * FROM tenants WHERE id = $1", tenant_id)
            .fetch_optional(&mut *txn)
            .await?
    } else {
        None
    };

    if let Some(input_timeout) = create_opts.timeout_ms
        && let Some(tenant) = &tenant
        && input_timeout > tenant.max_timeout
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your timeout of {input_timeout}ms is higher than your limit of {}ms",
            tenant.max_timeout
        ))));
    }

    if let Some(input_max_response_bytes) = create_opts.max_response_bytes
        && let Some(tenant) = &tenant
        && input_max_response_bytes > tenant.max_max_response_bytes
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your max response bytes of {input_max_response_bytes} is higher than your limit of {}",
            tenant.max_max_response_bytes
        ))));
    }

    let request_id = id::generate("request");

    let headers: Vec<String> = create_opts
        .request
        .headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();

    sqlx::query!(
        r#"
      INSERT INTO http_requests (id, method, url, headers, body)
      VALUES ($1, $2, $3, $4, $5)
      "#,
        request_id,
        create_opts.request.method,
        create_opts.request.url,
        &headers,
        create_opts.request.body
    )
    .execute(&mut *txn)
    .await?;

    let job_id = id::generate("one_off_job");
    let max_retries = create_opts
        .max_retries
        .or(tenant.map(|t| t.default_retries))
        .unwrap_or(3);

    sqlx::query!(r#"
      INSERT INTO one_off_jobs (id, region, tenant_id, request_id, execute_at, timeout_ms, max_retries, max_response_bytes)
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
      "#,
        job_id,
        create_opts.region.clone(),
        tenant_id,
        request_id,
        create_opts.execute_at,
        create_opts.timeout_ms,
        max_retries,
        create_opts.max_response_bytes
    )
    .execute(&mut *txn)
    .await?;

    txn.commit().await?;

    let job = OneOffJob {
        id: job_id,
        region: create_opts.region.clone(),
        execute_at: create_opts.execute_at,
        request: create_opts.request,
        executions: Vec::new(),
        timeout_ms: create_opts.timeout_ms,
        max_retries,
        max_response_bytes: create_opts.max_response_bytes,
        tenant_id,
    };

    Ok(job)
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new().routes(routes!(create_job))
}
