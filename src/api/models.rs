use std::collections::HashMap;

use axum::{Json, response::IntoResponse};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OneOffJob {
    pub id: String,
    pub region: String,
    pub execute_at: i64,
    pub request: Request,
    pub executions: Vec<Execution>,
    pub timeout_ms: Option<i32>,
    pub max_retries: i32,
    pub max_response_bytes: Option<i32>,
    pub tenant_id: Option<String>,
}

impl IntoResponse for OneOffJob {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Execution {
    pub id: String,
    pub region: String,
    pub scheduled_at: i64,
    pub executed_at: Option<i64>,
    pub success: Option<bool>,
    pub request: Request,
    pub response: Option<Response>,
    pub response_error: Option<String>,
    pub timeout_ms: Option<i32>,
    pub max_retries: i32,
    pub max_response_bytes: Option<i32>,
    pub tenant_id: Option<String>,
    pub one_off_job_id: Option<String>,
    pub cron_job_id: Option<String>,
    pub retry_for: Option<String>,
}

impl IntoResponse for Execution {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Response {
    pub status: i32,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Tenant {
    pub id: String,
    pub tokens: i32,
    pub max_tokens: i32,
    pub tok_per_day: i32,
    pub max_timeout: i32,
    pub default_retries: i32,
    pub max_max_response_bytes: i32,
}

impl IntoResponse for Tenant {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}
