use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{Executor, Postgres};
use utoipa::IntoParams;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    ApiError, ApiListResponse, Context, TenantId,
    models::{Execution, HttpRequest, HttpResponse},
};

#[derive(Debug)]
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
    timeout_ms: Option<i32>,
    max_retries: i32,
    max_response_bytes: Option<i32>,
    tenant_id: Option<String>,
    one_off_job_id: Option<String>,
    cron_job_id: Option<String>,
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
            request: HttpRequest {
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
                (Some(status), Some(headers), Some(body)) => Some(HttpResponse {
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
            retry_for: self.retry_for_id.clone(),
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
struct QueryParams {
    cursor: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    completed: Option<bool>,
    limit: Option<i64>,
    one_off_job_id: Option<String>,
    cron_id: Option<String>,
}

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
    let limit = params.limit.unwrap_or(15).min(250);

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
        AND ($3::bool IS NULL OR
          ($3 = true AND exe.id IS NOT NULL) OR
          ($3 = false AND exe.id IS NULL))
        AND ($4::text IS NULL OR job.id < $4)
        AND ($5::bigint IS NULL OR job.scheduled_at >= to_timestamp($5))
        AND ($6::bigint IS NULL OR job.scheduled_at <= to_timestamp($6))
        AND ($7::text IS NULL OR job.one_off_job_id = $7)
        AND ($8::text IS NULL OR job.cron_job_id = $8)
      ORDER BY job.id DESC
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
    )
    .fetch_all(&ctx.pool)
    .await?;

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
    .await?;

    if execution.is_none() {
        return Err(ApiError::not_found());
    }

    let execution = execution.unwrap();

    Ok(execution.to_execution())
}

pub async fn get_executions<'a, E>(
    jobs: Vec<String>,
    tenant_id: Option<String>,
    count_per: i64,
    executor: E,
) -> Result<Vec<Execution>, sqlx::Error>
where
    E: Executor<'a, Database = Postgres>,
{
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
    job.retry_for_id
  FROM (
    SELECT
      *,
      ROW_NUMBER() OVER (PARTITION BY
        tenant_id,
        one_off_job_id,
        cron_job_id
          ORDER BY
            scheduled_at DESC
        ) AS row_num
    FROM scheduled_jobs
    WHERE
      (one_off_job_id = ANY($1)
      OR cron_job_id = ANY($1))
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
    .fetch_all(executor)
    .await?;

    // dbg!(&execution);

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
