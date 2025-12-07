use std::collections::HashMap;

use axum::extract::{Query, State};
use http::HeaderMap;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{ApiError, ApiListResponse, Context};

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
    headers: HeaderMap,
    Query(params): Query<QueryParams>,
) -> Result<ApiListResponse<Execution>, ApiError> {
    let tenant_id = headers
        .get("tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    let results = sqlx::query!(
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
        ($1::text IS NULL OR job.tenant_id = $1)
        AND ($2::bool IS NULL OR exe.id IS NOT NULL)
        AND ($3::text IS NULL OR job.id > $3)
        AND ($4::bigint IS NULL OR job.scheduled_at >= to_timestamp($4))
        AND ($5::bigint IS NULL OR job.scheduled_at <= to_timestamp($5))
      ORDER BY job.id ASC
      LIMIT 100;
      "#,
        tenant_id,
        params.completed,
        params.cursor,
        params.from,
        params.to
    )
    .fetch_all(&ctx.pool)
    .await
    .map_err(|err| {
        eprintln!("Error fetching executions: {err:?}");
        ApiError::internal_server_error(None)
    })?;

    let executions: Vec<Execution> = results
        .iter()
        .map(|row| Execution {
            id: row.id.clone(),
            region: row.region.clone(),
            scheduled_at: row.scheduled_at.timestamp(),
            executed_at: row.executed_at.map(|time| time.timestamp()),
            success: row.success,
            request: Request {
                method: row.method.clone(),
                url: row.url.clone(),
                headers: row
                    .req_headers
                    .iter()
                    .filter_map(|entry| {
                        let mut parts = entry.splitn(2, ":");
                        let key = parts.next()?.trim().to_string();
                        let value = parts.next()?.trim().to_string();
                        Some((key, value))
                    })
                    .collect(),
                body: row.req_body.clone(),
            },
            response: match (row.status, row.res_headers.clone(), row.res_body.clone()) {
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
            response_error: row.response_error.clone(),
            timeout_ms: row.timeout_ms,
            max_retries: row.max_retries,
            max_response_bytes: row.max_response_bytes,
            tenant_id: row.tenant_id.clone(),
            one_off_job_id: row.one_off_job_id.clone(),
            cron_job_id: row.cron_job_id.clone(),
            workflow_id: row.workflow_id.clone(),
            retry_for: row.retry_for_id.clone(),
        })
        .collect();

    let last_execution = executions.last().map(|exe| exe.id.clone());

    Ok(ApiListResponse {
        data: executions,
        cursor: last_execution,
    })
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new().routes(routes!(list_executions))
}
