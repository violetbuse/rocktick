use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    api::{
        ApiError, ApiListResponse, Context, JsonBody, TenantId, executions,
        models::{Execution, OneOffJob, Request},
    },
    id,
};

struct IntermediateOneOffJob {
    id: String,
    region: String,
    tenant_id: Option<String>,
    method: String,
    url: String,
    headers: Vec<String>,
    body: Option<String>,
    execute_at: i64,
    timeout_ms: Option<i32>,
    max_retries: i32,
    max_response_bytes: Option<i32>,
    created_at: DateTime<Utc>,
}

impl IntermediateOneOffJob {
    pub fn to_one_off_job(&self, executions: &[Execution]) -> OneOffJob {
        let mut executions: Vec<Execution> = executions
            .iter()
            .filter(|exec| exec.one_off_job_id == Some(self.id.clone()))
            .cloned()
            .collect();

        executions.sort_by(|a, b| a.scheduled_at.cmp(&b.scheduled_at).reverse());

        OneOffJob {
            id: self.id.clone(),
            region: self.region.clone(),
            execute_at: self.execute_at,
            request: Request {
                method: self.method.clone(),
                url: self.url.clone(),
                headers: self
                    .headers
                    .iter()
                    .filter_map(|entry| {
                        let mut parts = entry.splitn(2, ":");
                        let key = parts.next()?.trim().to_string();
                        let value = parts.next()?.trim().to_string();
                        Some((key, value))
                    })
                    .collect(),
                body: self.body.clone(),
            },
            executions,
            timeout_ms: self.timeout_ms,
            max_retries: self.max_retries,
            max_response_bytes: self.max_response_bytes,
            tenant_id: self.tenant_id.clone(),
        }
    }
}

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

#[derive(Debug, Deserialize, IntoParams)]
struct QueryParams {
    cursor: Option<String>,
    limit: Option<i64>,
}

/// List Jobs
#[utoipa::path(
  get,
  path = "/api/jobs",
  params(QueryParams),
  responses((status = 200, body = ApiListResponse<OneOffJob>),
    (status = "4XX", body = ApiError),
    (status = "5XX", body = ApiError)),
  tag = "one off jobs"
)]
async fn list_jobs(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    Query(params): Query<QueryParams>,
) -> Result<ApiListResponse<OneOffJob>, ApiError> {
    let limit = params.limit.unwrap_or(15).min(250);

    let jobs = sqlx::query_as!(
        IntermediateOneOffJob,
        r#"
      SELECT
        job.id,
        job.region,
        job.tenant_id,
        req.method,
        req.url,
        req.headers,
        req.body,
        job.execute_at,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.created_at
      FROM one_off_jobs as job
      INNER JOIN http_requests as req
        ON req.id = job.request_id
      WHERE
        ($2::text IS NULL OR job.tenant_id = $2)
        AND ($3::text IS NULL OR job.id > $3)
      ORDER BY job.id DESC
      LIMIT $1;
      "#,
        limit,
        tenant_id.clone(),
        params.cursor
    )
    .fetch_all(&ctx.pool)
    .await?;

    let job_ids = jobs.iter().map(|j| j.id.clone()).collect();

    dbg!(&job_ids);

    let executions = executions::get_executions(job_ids, tenant_id, 5, &ctx.pool).await?;

    dbg!(&executions);

    let jobs: Vec<OneOffJob> = jobs.iter().map(|j| j.to_one_off_job(&executions)).collect();

    let last_job = jobs.last().map(|j| j.id.clone());

    Ok(ApiListResponse {
        count: jobs.len(),
        data: jobs,
        cursor: last_job,
    })
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new().routes(routes!(create_job, list_jobs))
}
