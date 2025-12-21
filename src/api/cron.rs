use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use croner::Cron;
use serde::Deserialize;
use std::str::FromStr;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    api::{
        ApiError, ApiListResponse, Context, JsonBody, TenantId, executions,
        models::{CronJob, Execution, HttpRequest},
    },
    id,
};

struct IntermediateCronJob {
    id: String,
    region: String,
    tenant_id: Option<String>,
    req_id: String,
    method: String,
    url: String,
    headers: Vec<String>,
    body: Option<String>,
    schedule: String,
    timeout_ms: Option<i32>,
    max_retries: i32,
    max_response_bytes: Option<i32>,
    created_at: DateTime<Utc>,
    error: Option<String>,
}

impl IntermediateCronJob {
    pub fn to_cron_job(&self, executions: &[Execution]) -> CronJob {
        let mut executions: Vec<Execution> = executions
            .iter()
            .filter(|exec| exec.cron_job_id == Some(self.id.clone()))
            .cloned()
            .collect();

        executions.sort_by(|a, b| a.scheduled_at.cmp(&b.scheduled_at).reverse());

        CronJob {
            id: self.id.clone(),
            region: self.region.clone(),
            schedule: self.schedule.clone(),
            request: HttpRequest {
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
struct CreateCronJob {
    region: Option<String>,
    schedule: String,
    request: HttpRequest,
    timeout_ms: Option<i32>,
    max_retries: Option<i32>,
    max_response_bytes: Option<i32>,
}

#[utoipa::path(
  post,
  path = "/api/cron",
  request_body = CreateCronJob,
  responses(
    (status = 200, description = "Cron job created", body = CronJob),
    (status = "4XX", description = "Bad request", body = ApiError),
    (status = "5XX", description = "Internal server error", body = ApiError)
  ),
  tag = "cron jobs"
)]
async fn create_cron_job(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    JsonBody(create_opts): JsonBody<CreateCronJob>,
) -> Result<CronJob, ApiError> {
    let region = create_opts
        .region
        .or(ctx.valid_regions.first().cloned())
        .expect("There are no valid regions.");

    if !ctx.valid_regions.contains(&region) {
        let region_list = ctx.valid_regions.join(", ");

        return Err(ApiError::bad_request(Some(&format!(
            "Invalid region: {}, choose one of the following: {}",
            region, region_list
        ))));
    }

    if let Err(e) = Cron::from_str(&create_opts.schedule) {
        return Err(ApiError::bad_request(Some(&format!(
            "Invalid cron schedule '{}': {e}",
            create_opts.schedule
        ))));
    }

    create_opts.request.verify()?;

    let mut txn = ctx.pool.begin().await?;
    let tenant = if let Some(tenant_id) = tenant_id.clone() {
        sqlx::query!("SELECT * FROM tenants WHERE id = $1", tenant_id)
            .fetch_optional(&mut *txn)
            .await?
    } else {
        None
    };

    if tenant_id.is_some() && tenant.is_none() {
        return Err(ApiError::bad_request(Some("Invalid tenant id")));
    }

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

    if let Some(body_text) = &create_opts.request.body
        && let Some(tenant) = &tenant
        && body_text.len() as i32 > tenant.max_request_bytes
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your request body of {} bytes is higher than your limit of {}",
            body_text.len(),
            tenant.max_request_bytes
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

    let job_id = id::generate("cron");
    let max_retries = create_opts
        .max_retries
        .or(tenant.map(|t| t.default_retries))
        .unwrap_or(3);

    sqlx::query!(r#"
      INSERT INTO cron_jobs (id, region, tenant_id, request_id, schedule, timeout_ms, max_retries, max_response_bytes)
      VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
      "#,
        job_id,
        region,
        tenant_id,
        request_id,
        create_opts.schedule,
        create_opts.timeout_ms,
        max_retries,
        create_opts.max_response_bytes
    )
    .execute(&mut *txn)
    .await?;

    txn.commit().await?;

    let job = CronJob {
        id: job_id,
        region,
        schedule: create_opts.schedule,
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

#[utoipa::path(
  get,
  path = "/api/cron",
  params(QueryParams),
  responses((status = 200, body = ApiListResponse<CronJob>),
    (status = "4XX", body = ApiError),
    (status = "5XX", body = ApiError)),
  tag = "cron jobs"
)]
async fn list_cron_jobs(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    Query(params): Query<QueryParams>,
) -> Result<ApiListResponse<CronJob>, ApiError> {
    let limit = params.limit.unwrap_or(15).min(250);

    let jobs = sqlx::query_as!(
        IntermediateCronJob,
        r#"
      SELECT
        job.id,
        job.region,
        job.tenant_id,
        req.id as req_id,
        req.method,
        req.url,
        req.headers,
        req.body,
        job.schedule,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.created_at,
        job.error
      FROM cron_jobs as job
      INNER JOIN http_requests as req
        ON req.id = job.request_id
      WHERE
        job.deleted_at IS NULL
        AND ($2::text IS NULL OR job.tenant_id = $2)
        AND ($3::text IS NULL OR job.id < $3)
      ORDER BY job.id DESC
      LIMIT $1;
      "#,
        limit,
        tenant_id.clone(),
        params.cursor
    )
    .fetch_all(&ctx.pool)
    .await?;

    let job_ids: Vec<String> = jobs.iter().map(|j| j.id.clone()).collect();

    let completed_executions =
        executions::get_executions(job_ids.clone(), tenant_id.clone(), true, 5, &ctx.pool).await?;
    let not_yet_executed =
        executions::get_executions(job_ids.clone(), tenant_id.clone(), false, 2, &ctx.pool).await?;

    let executions = completed_executions
        .into_iter()
        .chain(not_yet_executed.into_iter())
        .collect::<Vec<_>>();

    let jobs: Vec<CronJob> = jobs.iter().map(|j| j.to_cron_job(&executions)).collect();

    let last_job = jobs.last().map(|j| j.id.clone());

    Ok(ApiListResponse {
        count: jobs.len(),
        data: jobs,
        cursor: last_job,
    })
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct UpdateCronJob {
    region: Option<String>,
    schedule: Option<String>,
    request: Option<HttpRequest>,
    timeout_ms: Option<i32>,
    max_retries: Option<i32>,
    max_response_bytes: Option<i32>,
}

#[utoipa::path(
  post,
  path = "/api/cron/{job_id}",
  params(("job_id", description = "Id of the cron job")),
  request_body = UpdateCronJob,
  responses(
    (status = 200, description = "Cron job updated", body = CronJob),
    (status = "4XX", description = "Job not found", body = ApiError),
    (status = "5XX", description = "Invalid request", body = ApiError)),
  tag = "cron jobs"
)]
async fn update_cron_job(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
    TenantId(tenant_id): TenantId,
    JsonBody(update_opts): JsonBody<UpdateCronJob>,
) -> Result<CronJob, ApiError> {
    if let Some(region) = update_opts.region.clone()
        && !ctx.valid_regions.contains(&region)
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Invalid region: {region}, choose one of the following: {}",
            ctx.valid_regions.join(", ")
        ))));
    }

    if let Some(schedule) = &update_opts.schedule
        && let Err(e) = Cron::from_str(schedule)
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Invalid cron schedule '{}': {e}",
            schedule
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

    if tenant_id.is_some() && tenant.is_none() {
        return Err(ApiError::bad_request(Some("Invalid tenant id")));
    }

    if let Some(input_timeout) = update_opts.timeout_ms
        && let Some(tenant) = &tenant
        && input_timeout > tenant.max_timeout
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your timeout of {input_timeout}ms is higher than your limit of {}ms",
            tenant.max_timeout
        ))));
    }

    if let Some(input_max_response_bytes) = update_opts.max_response_bytes
        && let Some(tenant) = &tenant
        && input_max_response_bytes > tenant.max_max_response_bytes
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your max response bytes of {input_max_response_bytes} is higher than your limit of {}",
            tenant.max_max_response_bytes
        ))));
    }

    if let Some(body_text) = update_opts.request.as_ref().and_then(|r| r.body.clone())
        && let Some(tenant) = &tenant
        && body_text.len() as i32 > tenant.max_request_bytes
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your request body of {} bytes is higher than your limit of {}",
            body_text.len(),
            tenant.max_request_bytes
        ))));
    }

    let existing = sqlx::query!(
        r#"
      SELECT
        job.id,
        job.region,
        job.schedule,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.request_id as req_id
      FROM cron_jobs as job
      WHERE job.deleted_at IS NULL AND job.id = $1 AND ($2::text IS NULL OR job.tenant_id = $2)
      FOR UPDATE
      "#,
        job_id.clone(),
        tenant_id
    )
    .fetch_optional(&mut *txn)
    .await?;

    if existing.is_none() {
        return Err(ApiError::not_found());
    }

    let existing_data = existing.unwrap();

    sqlx::query!(
        r#"DELETE FROM scheduled_jobs WHERE cron_job_id = $1 AND execution_id IS NULL;"#,
        job_id.clone()
    )
    .execute(&mut *txn)
    .await?;

    if let Some(updated_request) = update_opts.request.clone() {
        updated_request.verify()?;

        let headers: Vec<String> = updated_request
            .headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        sqlx::query!(
            r#"
        UPDATE http_requests
        SET
          method = $1,
          url = $2,
          headers = $3,
          body = $4
        WHERE id = $5;
        "#,
            updated_request.method,
            updated_request.url,
            &headers,
            updated_request.body,
            existing_data.req_id
        )
        .execute(&mut *txn)
        .await?;
    }

    let new_region = update_opts.region.unwrap_or(existing_data.region);
    let new_schedule = update_opts.schedule.unwrap_or(existing_data.schedule);
    let new_timeout_ms = update_opts.timeout_ms.or(existing_data.timeout_ms);
    let new_max_retries = update_opts.max_retries.unwrap_or(existing_data.max_retries);
    let new_max_response_bytes = update_opts
        .max_response_bytes
        .or(existing_data.max_response_bytes);

    let new_job = sqlx::query_as!(
        IntermediateCronJob,
        r#"
      UPDATE cron_jobs
      SET
        region = $2,
        schedule = $3,
        timeout_ms = $4,
        max_retries = $5,
        max_response_bytes = $6,
        error = NULL
      FROM http_requests AS req
      WHERE cron_jobs.id = $1 AND req.id = cron_jobs.request_id
      RETURNING
        cron_jobs.id,
        cron_jobs.region,
        cron_jobs.tenant_id,
        req.id as req_id,
        req.method,
        req.url,
        req.headers,
        req.body,
        cron_jobs.schedule,
        cron_jobs.timeout_ms,
        cron_jobs.max_retries,
        cron_jobs.max_response_bytes,
        cron_jobs.created_at,
        cron_jobs.error
      "#,
        job_id.clone(),
        new_region,
        new_schedule,
        new_timeout_ms,
        new_max_retries,
        new_max_response_bytes
    )
    .fetch_one(&mut *txn)
    .await?;

    let completed_executions =
        executions::get_executions(vec![job_id.clone()], tenant_id.clone(), true, 5, &mut *txn)
            .await?;

    let not_yet_executed =
        executions::get_executions(vec![job_id.clone()], tenant_id.clone(), false, 2, &mut *txn)
            .await?;

    let executions = completed_executions
        .into_iter()
        .chain(not_yet_executed.into_iter())
        .collect::<Vec<_>>();

    txn.commit().await?;

    let job = new_job.to_cron_job(&executions);

    Ok(job)
}

#[utoipa::path(
  delete,
  path = "/api/cron/{job_id}",
  params(("job_id", description = "Id of the cron job")),
  responses(
    (status = 200, description = "Cron job found", body = CronJob),
    (status = "4XX", description = "Job not found", body = ApiError),
    (status = "5XX", description = "Invalid request", body = ApiError)),
  tag = "cron jobs"
)]
async fn delete_cron_job(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
    TenantId(tenant_id): TenantId,
) -> Result<CronJob, ApiError> {
    let mut txn = ctx.pool.begin().await?;
    let tenant = if let Some(tenant_id) = tenant_id.clone() {
        sqlx::query!("SELECT * FROM tenants WHERE id = $1", tenant_id)
            .fetch_optional(&mut *txn)
            .await?
    } else {
        None
    };

    if tenant_id.is_some() && tenant.is_none() {
        return Err(ApiError::bad_request(Some("Invalid tenant id")));
    }

    let existing = sqlx::query_as!(
        IntermediateCronJob,
        r#"
  SELECT
    job.id,
    job.region,
    job.tenant_id,
    req.id as req_id,
    req.method,
    req.url,
    req.headers,
    req.body,
    job.schedule,
    job.timeout_ms,
    job.max_retries,
    job.max_response_bytes,
    job.created_at,
    job.error
  FROM cron_jobs as job
  INNER JOIN http_requests as req
    ON req.id = job.request_id
  WHERE
    job.deleted_at IS NULL
    AND job.id = $1 AND
    ($2::text IS NULL OR job.tenant_id = $2)
  FOR UPDATE
  "#,
        job_id.clone(),
        tenant_id
    )
    .fetch_optional(&ctx.pool)
    .await?;

    if existing.is_none() {
        return Err(ApiError::not_found());
    }

    let executions =
        executions::get_executions(vec![job_id.clone()], tenant_id, true, 7, &mut *txn).await?;

    let existing = existing.unwrap();

    let pre_delete_job = existing.to_cron_job(&executions);

    sqlx::query!(
        r#"
      UPDATE cron_jobs
      SET deleted_at = NOW()
      WHERE id = $1
      "#,
        job_id.clone()
    )
    .execute(&mut *txn)
    .await?;

    sqlx::query!(
        r#"
      DELETE FROM scheduled_jobs
      WHERE cron_job_id = $1
        AND lock_nonce IS NULL
        AND execution_id IS NULL
      "#,
        job_id.clone()
    )
    .execute(&mut *txn)
    .await?;

    sqlx::query!(
        r#"
      UPDATE scheduled_jobs
      SET deleted_at = NOW()
      WHERE cron_job_id = $1
      "#,
        job_id.clone()
    )
    .execute(&mut *txn)
    .await?;

    sqlx::query!(
        r#"
      UPDATE http_requests
      SET
        body = '<deleted>',
        headers = '{}'
      WHERE id = $1
      "#,
        existing.req_id
    )
    .execute(&mut *txn)
    .await?;

    txn.commit().await?;

    Ok(pre_delete_job)
}

#[utoipa::path(
  get,
  path = "/api/cron/{job_id}",
  params(("job_id", description = "Id of the cron job")),
  responses(
    (status = 200, description = "Cron job found", body = CronJob),
    (status = "4XX", description = "Job not found", body = ApiError),
    (status = "5XX", description = "Invalid request", body = ApiError)),
  tag = "cron jobs"
)]
async fn get_cron_job(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
    TenantId(tenant_id): TenantId,
) -> Result<CronJob, ApiError> {
    let job = sqlx::query_as!(
        IntermediateCronJob,
        r#"
    SELECT
      job.id,
      job.region,
      job.tenant_id,
      req.id as req_id,
      req.method,
      req.url,
      req.headers,
      req.body,
      job.schedule,
      job.timeout_ms,
      job.max_retries,
      job.max_response_bytes,
      job.created_at,
      job.error
    FROM cron_jobs as job
    INNER JOIN http_requests as req
      ON req.id = job.request_id
    WHERE
      job.deleted_at IS NULL
      AND job.id = $1 AND
      ($2::text IS NULL OR job.tenant_id = $2);
    "#,
        job_id.clone(),
        tenant_id
    )
    .fetch_optional(&ctx.pool)
    .await?;

    if job.is_none() {
        return Err(ApiError::not_found());
    }

    let completed_executions =
        executions::get_executions(vec![job_id.clone()], tenant_id.clone(), true, 5, &ctx.pool)
            .await?;
    let not_yet_executed =
        executions::get_executions(vec![job_id.clone()], tenant_id.clone(), false, 2, &ctx.pool)
            .await?;

    let executions = completed_executions
        .into_iter()
        .chain(not_yet_executed.into_iter())
        .collect::<Vec<_>>();

    let job = job.unwrap().to_cron_job(&executions);

    Ok(job)
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new()
        .routes(routes!(create_cron_job, list_cron_jobs))
        .routes(routes!(update_cron_job, get_cron_job, delete_cron_job))
}
