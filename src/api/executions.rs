use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{ApiError, ApiListResponse, Context, TenantId};

#[derive(Debug, Serialize, ToSchema)]
pub struct Execution {
    id: String,
    region: String,
    scheduled_at: i64,
    executed_at: Option<i64>,
    success: Option<bool>,
    request: Request,
    response: Option<Response>,
    response_error: Option<String>,
    timeout_ms: i32,
    max_retries: i32,
    max_response_bytes: i32,
    tenant_id: Option<String>,
    one_off_job_id: Option<String>,
    cron_job_id: Option<String>,
    workflow_id: Option<String>,
    retry_for: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Request {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Response {
    status: i32,
    headers: HashMap<String, String>,
    body: String,
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
struct QueryParams {
    cursor: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    completed: Option<bool>,
    limit: Option<i64>,
    one_off_job_id: Option<String>,
    cron_id: Option<String>,
    workflow_id: Option<String>,
}

struct IntermediateExecution {
    id: String,
    region: String,
    scheduled_at: DateTime<Utc>,
    success: Option<bool>,
    executed_at: Option<DateTime<Utc>>,
    response_error: Option<String>,
    method: String,
    url: String,
    req_headers: Vec<String>,
    req_body: Option<String>,
    status: Option<i32>,
    res_headers: Option<Vec<String>>,
    res_body: Option<String>,
    timeout_ms: i32,
    max_retries: i32,
    max_response_bytes: i32,
    tenant_id: Option<String>,
    one_off_job_id: Option<String>,
    cron_job_id: Option<String>,
    workflow_id: Option<String>,
    retry_for_id: Option<String>,
}

impl IntermediateExecution {
    pub fn to_execution(&self) -> Execution {
        Execution {
            id: self.id.clone(),
            region: self.region.clone(),
            scheduled_at: self.scheduled_at.timestamp(),
            executed_at: self.executed_at.map(|time| time.timestamp()),
            success: self.success,
            request: Request {
                method: self.method.clone(),
                url: self.url.clone(),
                headers: self
                    .req_headers
                    .iter()
                    .filter_map(|entry| {
                        let mut parts = entry.splitn(2, ":");
                        let key = parts.next()?.trim().to_string();
                        let value = parts.next()?.trim().to_string();
                        Some((key, value))
                    })
                    .collect(),
                body: self.req_body.clone(),
            },
            response: match (self.status, self.res_headers.clone(), self.res_body.clone()) {
                (Some(status), Some(headers), Some(body)) => Some(Response {
                    status,
                    headers: headers
                        .iter()
                        .filter_map(|entry| {
                            let mut parts = entry.splitn(2, ":");
                            let key = parts.next()?.trim().to_string();
                            let value = parts.next()?.trim().to_string();
                            Some((key, value))
                        })
                        .collect(),
                    body,
                }),
                _ => None,
            },
            response_error: self.response_error.clone(),
            timeout_ms: self.timeout_ms,
            max_retries: self.max_retries,
            max_response_bytes: self.max_response_bytes,
            tenant_id: self.tenant_id.clone(),
            one_off_job_id: self.one_off_job_id.clone(),
            cron_job_id: self.cron_job_id.clone(),
            workflow_id: self.workflow_id.clone(),
            retry_for: self.retry_for_id.clone(),
        }
    }
}

/// List executions
#[utoipa::path(
  get,
  path = "/api/executions",
  params(QueryParams),
  responses((status = 200, body= ApiListResponse<Execution>),
    (status = "4XX", body = ApiError),
    (status = "5XX", body = ApiError)),
  tag = "executions"
)]
async fn list_executions(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    Query(params): Query<QueryParams>,
) -> Result<ApiListResponse<Execution>, ApiError> {
    let limit = params.limit.unwrap_or(15).min(1000);

    let results = sqlx::query_as!(
        IntermediateExecution,
        r#"
      SELECT
        job.id,
        job.region,
        job.scheduled_at,
        exe.success as "success?",
        exe.executed_at as "executed_at?",
        exe.response_error as "response_error?",
        req.method,
        req.url,
        req.headers as req_headers,
        req.body as req_body,
        res.status as "status?",
        res.headers as "res_headers?",
        res.body as "res_body?",
        job.timeout_ms,
        job.max_retries,
        job.max_response_bytes,
        job.tenant_id,
        job.one_off_job_id,
        job.cron_job_id,
        job.workflow_id,
        job.retry_for_id
      FROM scheduled_jobs as job
      INNER JOIN http_requests as req
        ON req.id = job.request_id
      LEFT JOIN job_executions exe
        ON job.execution_id = exe.id
      LEFT JOIN http_responses res
        ON exe.response_id = res.id
      WHERE
        ($2::text IS NULL OR job.tenant_id = $2)
        AND ($3::bool IS NULL OR exe.id IS NULL != $3)
        AND ($4::text IS NULL OR job.id > $4)
        AND ($5::bigint IS NULL OR job.scheduled_at >= to_timestamp($5))
        AND ($6::bigint IS NULL OR job.scheduled_at <= to_timestamp($6))
        AND ($7::text IS NULL OR job.one_off_job_id = $7)
        AND ($8::text IS NULL OR job.cron_job_id = $8)
        AND ($9::text IS NULL OR job.workflow_id = $9)
      ORDER BY job.id ASC
      LIMIT $1;
      "#,
        limit,
        tenant_id,
        params.completed,
        params.cursor,
        params.from,
        params.to,
        params.one_off_job_id,
        params.cron_id,
        params.workflow_id,
    )
    .fetch_all(&ctx.pool)
    .await
    .map_err(|err| {
        eprintln!("Error fetching executions: {err:?}");
        ApiError::internal_server_error(None)
    })?;

    let executions: Vec<Execution> = results
        .iter()
        .map(IntermediateExecution::to_execution)
        .collect();

    let last_execution = executions.last().map(|exe| exe.id.clone());

    Ok(ApiListResponse {
        count: executions.len(),
        data: executions,
        cursor: last_execution,
    })
}

impl IntoResponse for Execution {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

/// Get Execution
#[utoipa::path(
    get,
    path = "/api/executions/{execution_id}",
    responses(
        (status = 200, description = "Get Execution", body = Execution),
        (status = "4XX", description = "Execution not found", body = ApiError),
        (status = "5XX", description = "Internal Server Error", body = ApiError),
    ),
    params(
        ("execution_id" = String, Path, description = "Execution ID"),
    ),
    tag = "executions"
)]
async fn get_execution(
    State(ctx): State<Context>,
    Path(execution_id): Path<String>,
    TenantId(tenant_id): TenantId,
) -> Result<Execution, ApiError> {
    let execution = sqlx::query_as!(
        IntermediateExecution,
        r#"
    SELECT
      job.id,
      job.region,
      job.scheduled_at,
      exe.success as "success?",
      exe.executed_at as "executed_at?",
      exe.response_error as "response_error?",
      req.method,
      req.url,
      req.headers as req_headers,
      req.body as req_body,
      res.status as "status?",
      res.headers as "res_headers?",
      res.body as "res_body?",
      job.timeout_ms,
      job.max_retries,
      job.max_response_bytes,
      job.tenant_id,
      job.one_off_job_id,
      job.cron_job_id,
      job.workflow_id,
      job.retry_for_id
    FROM scheduled_jobs as job
    INNER JOIN http_requests as req
      ON req.id = job.request_id
    LEFT JOIN job_executions exe
      ON job.execution_id = exe.id
    LEFT JOIN http_responses res
      ON exe.response_id = res.id
    WHERE
      job.id = $1 AND
      ($2::text IS NULL OR job.tenant_id = $2)
    "#,
        execution_id,
        tenant_id
    )
    .fetch_optional(&ctx.pool)
    .await
    .map_err(|err| {
        eprintln!("Error fetching execution {execution_id}: {err:?}");
        ApiError::internal_server_error(None)
    })?;

    if execution.is_none() {
        return Err(ApiError::not_found());
    }

    let execution = execution.unwrap();

    Ok(execution.to_execution())
}

pub async fn get_executions(
    jobs: Vec<String>,
    tenant_id: Option<String>,
    count_per: i64,
    pool: &Pool<Postgres>,
) -> Result<Vec<Execution>, sqlx::Error> {
    let execution = sqlx::query_as!(
        IntermediateExecution,
        r#"
  SELECT
    job.id,
    job.region,
    job.scheduled_at,
    exe.success as "success?",
    exe.executed_at as "executed_at?",
    exe.response_error as "response_error?",
    req.method,
    req.url,
    req.headers as req_headers,
    req.body as req_body,
    res.status as "status?",
    res.headers as "res_headers?",
    res.body as "res_body?",
    job.timeout_ms,
    job.max_retries,
    job.max_response_bytes,
    job.tenant_id,
    job.one_off_job_id,
    job.cron_job_id,
    job.workflow_id,
    job.retry_for_id
  FROM (
    SELECT
      *,
      ROW_NUMBER() OVER (PARTITION BY
        tenant_id,
        one_off_job_id,
        cron_job_id,
        workflow_id
          ORDER BY
            scheduled_at DESC
        ) AS row_num
    FROM scheduled_jobs
    WHERE
      ($1::text[] IS NULL OR
        one_off_job_id = ANY($1) OR
        cron_job_id = ANY($1) OR
        workflow_id = ANY($1))
      AND ($2::text IS NULL OR tenant_id = $2)
  ) job
  INNER JOIN http_requests as req
    ON req.id = job.request_id
  LEFT JOIN job_executions exe
    ON job.execution_id = exe.id
  LEFT JOIN http_responses res
    ON exe.response_id = res.id
  WHERE
    job.row_num <= $3
  "#,
        &jobs,
        tenant_id,
        count_per
    )
    .fetch_optional(pool)
    .await?;

    Ok(execution
        .iter()
        .map(IntermediateExecution::to_execution)
        .collect())
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new()
        .routes(routes!(list_executions))
        .routes(routes!(get_execution))
}
