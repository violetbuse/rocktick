use axum::extract::{Path, Query, State};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    api::{
        ApiError, ApiListResponse, Context, JsonBody, TenantId, executions,
        models::{Execution, HttpRequest, OneOffJob},
    },
    id, util,
};

struct IntermediateOneOffJob {
    id: String,
    region: String,
    tenant_id: Option<String>,
    req_id: String,
    method: String,
    url: String,
    headers: Vec<String>,
    body: Option<String>,
    execute_at: i64,
    timeout_ms: Option<i32>,
    max_retries: i32,
    max_response_bytes: Option<i32>,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
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
            deleted_at: self.deleted_at.as_ref().map(DateTime::timestamp),
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct CreateJob {
    region: Option<String>,
    execute_at: i64,
    request: HttpRequest,
    timeout_ms: Option<i32>,
    max_retries: Option<i32>,
    max_response_bytes: Option<i32>,
}

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
#[tracing::instrument(name = "api_create_job")]
async fn create_job(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    JsonBody(create_opts): JsonBody<CreateJob>,
) -> Result<OneOffJob, ApiError> {
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

    if let Some(input_max_retries) = create_opts.max_retries
        && let Some(tenant) = &tenant
        && input_max_retries > tenant.max_retries
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your max retries of {input_max_retries} is higher than your limit of {}",
            tenant.max_retries
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

    let scheduled_for = DateTime::from_timestamp_secs(create_opts.execute_at).ok_or(
        ApiError::bad_request(Some(&format!("Invalid time {}", create_opts.execute_at))),
    )?;
    let time_until = scheduled_for - Utc::now();

    if let Some(tenant) = &tenant
        && time_until > Duration::days(tenant.max_delay_days as i64)
    {
        let over_by = time_until - Duration::days(tenant.max_delay_days as i64);
        return Err(ApiError::bad_request(Some(&format!(
            "Your request is scheduled {} days in the future, which is higher than your limit of {} days by {} seconds",
            time_until.num_days(),
            tenant.max_delay_days,
            over_by.num_seconds()
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
        region,
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
        region,
        execute_at: create_opts.execute_at,
        request: create_opts.request,
        executions: Vec::new(),
        timeout_ms: create_opts.timeout_ms,
        max_retries,
        max_response_bytes: create_opts.max_response_bytes,
        tenant_id,
        deleted_at: None,
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
  path = "/api/jobs",
  params(QueryParams),
  responses((status = 200, body = ApiListResponse<OneOffJob>),
    (status = "4XX", body = ApiError),
    (status = "5XX", body = ApiError)),
  tag = "one off jobs"
)]
#[tracing::instrument(name = "api_list_jobs")]
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
        req.id as req_id,
        req.method,
        req.url,
        req.headers,
        req.body,
        job.execute_at,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.created_at,
        job.deleted_at
      FROM one_off_jobs as job
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

    // dbg!(&job_ids);

    let completed_executions =
        executions::get_executions(job_ids.clone(), tenant_id.clone(), true, 3, &ctx.pool).await?;

    let not_yet_executed =
        executions::get_executions(job_ids.clone(), tenant_id.clone(), false, 2, &ctx.pool).await?;

    let executions = completed_executions
        .into_iter()
        .chain(not_yet_executed.into_iter())
        .collect::<Vec<_>>();

    // dbg!(&executions);

    let jobs: Vec<OneOffJob> = jobs.iter().map(|j| j.to_one_off_job(&executions)).collect();

    let last_job = jobs.last().map(|j| j.id.clone());

    Ok(ApiListResponse {
        count: jobs.len(),
        data: jobs,
        cursor: last_job,
    })
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct UpdateJob {
    region: Option<String>,
    execute_at: Option<i64>,
    request: Option<HttpRequest>,
    timeout_ms: Option<i32>,
    max_retries: Option<i32>,
    max_response_bytes: Option<i32>,
}

#[utoipa::path(
  post,
  path = "/api/jobs/{job_id}",
  params(("job_id", description = "Id of the job")),
  request_body = UpdateJob,
  responses(
    (status = 200, description = "Job updated", body = OneOffJob),
    (status = "4XX", description = "Job not found", body = ApiError),
    (status = "5XX", description = "Invalid request", body = ApiError)),
  tag = "one off jobs"
)]
#[tracing::instrument(name = "api_update_job")]
async fn update_job(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
    TenantId(tenant_id): TenantId,
    JsonBody(update_opts): JsonBody<UpdateJob>,
) -> Result<OneOffJob, ApiError> {
    if let Some(region) = update_opts.region.clone()
        && !ctx.valid_regions.contains(&region)
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Invalid region: {region}, choose one of the following: {}",
            ctx.valid_regions.join(", ")
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

    if let Some(input_max_retries) = update_opts.max_retries
        && let Some(tenant) = &tenant
        && input_max_retries > tenant.max_retries
    {
        return Err(ApiError::bad_request(Some(&format!(
            "Your max retries of {input_max_retries} is higher than your limit of {}",
            tenant.max_retries
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

    if let Some(input_scheduled_for) = update_opts.execute_at {
        let scheduled_for = DateTime::from_timestamp_secs(input_scheduled_for).ok_or(
            ApiError::bad_request(Some(&format!("Invalid time {}", input_scheduled_for))),
        )?;
        let time_until = scheduled_for - Utc::now();

        if let Some(tenant) = &tenant
            && time_until > Duration::days(tenant.max_delay_days as i64)
        {
            let over_by = time_until - Duration::days(tenant.max_delay_days as i64);
            return Err(ApiError::bad_request(Some(&format!(
                "Your request is scheduled {} days in the future, which is higher than your limit of {} days by {} seconds",
                time_until.num_days(),
                tenant.max_delay_days,
                over_by.num_seconds()
            ))));
        }
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
      WITH
        job AS (
          SELECT *
          FROM one_off_jobs
          WHERE deleted_at IS NULL AND id = $1
            AND ($2::text IS NULL OR tenant_id = $2)
          FOR UPDATE
        ),
        req AS (
          SELECT *
          FROM http_requests
          WHERE id = (SELECT request_id FROM job)
          FOR UPDATE
        )
      SELECT
        job.id,
        job.region,
        job.execute_at,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        req.id as req_id,
        req.method,
        req.url,
        req.headers,
        req.body
      FROM job
      JOIN req ON req.id = job.request_id
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
        r#"DELETE FROM scheduled_jobs WHERE one_off_job_id = $1 AND execution_id IS NULL;"#,
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
    let new_execute_at = update_opts.execute_at.unwrap_or(existing_data.execute_at);
    let new_timeout_ms = update_opts.timeout_ms.or(existing_data.timeout_ms);
    let new_max_retries = update_opts.max_retries.unwrap_or(existing_data.max_retries);
    let new_max_response_bytes = update_opts
        .max_response_bytes
        .or(existing_data.max_response_bytes);

    let new_job = sqlx::query_as!(
        IntermediateOneOffJob,
        r#"
      UPDATE one_off_jobs
      SET
        region = $2,
        execute_at = $3,
        timeout_ms = $4,
        max_retries = $5,
        max_response_bytes = $6
      FROM one_off_jobs as job
      JOIN http_requests AS req
        ON req.id = job.request_id
      WHERE job.id = $1
      RETURNING
        job.id,
        job.region,
        job.tenant_id,
        req.id as req_id,
        req.method,
        req.url,
        req.headers,
        req.body,
        job.execute_at,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.created_at,
        job.deleted_at
      "#,
        job_id.clone(),
        new_region,
        new_execute_at,
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

    let job = new_job.to_one_off_job(&executions);

    Ok(job)
}

#[utoipa::path(
  get,
  path = "/api/jobs/{job_id}",
  params(("job_id", description = "Id of the job")),
  responses(
    (status = 200, description = "Job found", body = OneOffJob),
    (status = "4XX", description = "Job not found", body = ApiError),
    (status = "5XX", description = "Invalid request", body = ApiError)),
  tag = "one off jobs"
)]
#[tracing::instrument(name = "api_get_job")]
async fn get_job(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
    TenantId(tenant_id): TenantId,
) -> Result<OneOffJob, ApiError> {
    let job = sqlx::query_as!(
        IntermediateOneOffJob,
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
      job.execute_at,
      job.timeout_ms,
      job.max_retries,
      job.max_response_bytes,
      job.created_at,
      job.deleted_at
    FROM one_off_jobs as job
    INNER JOIN http_requests as req
      ON req.id = job.request_id
    WHERE
      job.id = $1 AND
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

    let job = job.unwrap().to_one_off_job(&executions);

    Ok(job)
}

#[utoipa::path(
  delete,
  path = "/api/jobs/{job_id}",
  params(("job_id", description = "Id of the job")),
  responses(
    (status = 200, description = "Job deleted", body = OneOffJob),
    (status = "4XX", description = "Job not found", body = ApiError),
    (status = "5XX", description = "Invalid request", body = ApiError)),
  tag = "one off jobs"
)]
#[tracing::instrument(name = "api_delete_job")]
async fn delete_job(
    State(ctx): State<Context>,
    Path(job_id): Path<String>,
    TenantId(tenant_id): TenantId,
) -> Result<OneOffJob, ApiError> {
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
        IntermediateOneOffJob,
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
        job.execute_at,
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.created_at,
        job.deleted_at
      FROM one_off_jobs as job
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
    .fetch_optional(&mut *txn)
    .await?;

    if existing.is_none() {
        return Err(ApiError::not_found());
    }

    let executions =
        executions::get_executions(vec![job_id.clone()], tenant_id, true, 7, &mut *txn).await?;

    let existing = existing.unwrap();

    let pre_delete_job = existing.to_one_off_job(&executions);

    sqlx::query!(
        r#"
      UPDATE one_off_jobs
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
      WHERE one_off_job_id = $1
        AND lock_nonce IS NULL
        AND execution_id IS NULL
      "#,
        job_id.clone()
    )
    .execute(&mut *txn)
    .await?;

    util::http_requests::delete_http_req(existing.req_id, &mut txn).await?;

    txn.commit().await?;

    Ok(pre_delete_job)
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new()
        .routes(routes!(create_job, list_jobs))
        .routes(routes!(update_job, get_job, delete_job))
}
