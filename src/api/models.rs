use std::{collections::HashMap, str::FromStr};

use axum::{Json, response::IntoResponse};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::ApiError;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CronJob {
    pub id: String,
    pub region: String,
    pub schedule: String,
    pub request: HttpRequest,
    pub executions: Vec<Execution>,
    pub timeout_ms: Option<i32>,
    pub max_retries: i32,
    pub max_response_bytes: Option<i32>,
    pub tenant_id: Option<String>,
    pub deleted_at: Option<i64>,
}

impl IntoResponse for CronJob {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OneOffJob {
    pub id: String,
    pub region: String,
    pub execute_at: i64,
    pub request: HttpRequest,
    pub executions: Vec<Execution>,
    pub timeout_ms: Option<i32>,
    pub max_retries: i32,
    pub max_response_bytes: Option<i32>,
    pub tenant_id: Option<String>,
    pub deleted_at: Option<i64>,
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
    pub request: HttpRequest,
    pub response: Option<HttpResponse>,
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
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn verify(&self) -> Result<(), ApiError> {
        if self.method.len() > 10 {
            return Err(ApiError::bad_request(Some(&format!(
                "{} is not a valid http method (max-length: 10). Do you really need a custom http method?",
                self.method
            ))));
        }

        let method = http::Method::from_str(&self.method);

        if method.is_err() {
            return Err(ApiError::bad_request(Some(&format!(
                "{} is not a valid http method.",
                self.method
            ))));
        }

        let url = url::Url::parse(&self.url);

        if let Err(error) = url {
            return Err(ApiError::bad_request(Some(&format!(
                "{} is not a valid url: {error}",
                self.url,
            ))));
        };

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HttpResponse {
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
    pub max_retries: i32,
    pub max_max_response_bytes: i32,
    pub max_request_bytes: i32,
    pub retain_for_days: i32,
    pub max_delay_days: i32,
    pub max_cron_jobs: i32,
}

impl IntoResponse for Tenant {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}
