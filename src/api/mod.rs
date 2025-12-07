mod cron;
mod publish;
mod tenants;

use axum::{
    Json,
    extract::{Request, State, rejection::JsonRejection},
    middleware::Next,
    response::{IntoResponse, Response},
};

use axum_macros::FromRequest;
use http::StatusCode;
use serde::Serialize;
use sqlx::{Pool, Postgres};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::ApiOptions;

#[derive(Debug, Clone)]
pub struct Config {
    port: usize,
    pool: Pool<Postgres>,
    valid_regions: Vec<String>,
    auth_key: Option<String>,
}

impl Config {
    pub async fn from_cli(options: ApiOptions, pool: Pool<Postgres>) -> Self {
        let valid_regions = options
            .valid_regions
            .split(",")
            .map(|s| s.trim().to_string())
            .collect();

        Self {
            port: options.port,
            valid_regions,
            pool,
            auth_key: options.auth_key,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Context {
    pub pool: Pool<Postgres>,
    pub valid_regions: Vec<String>,
    auth_key: Option<String>,
}

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct JsonBody<T>(T);

impl From<JsonRejection> for ApiError {
    fn from(value: JsonRejection) -> Self {
        match value {
            JsonRejection::JsonDataError(json_data_error) => {
                ApiError::bad_request(Some(&json_data_error.body_text()))
            }
            JsonRejection::JsonSyntaxError(json_syntax_error) => {
                ApiError::bad_request(Some(&json_syntax_error.body_text()))
            }
            JsonRejection::MissingJsonContentType(missing_json_content_type) => {
                ApiError::bad_request(Some(&missing_json_content_type.body_text()))
            }
            JsonRejection::BytesRejection(bytes_rejection) => {
                ApiError::bad_request(Some(&bytes_rejection.body_text()))
            }
            _ => ApiError::bad_request(None),
        }
    }
}

impl From<String> for ApiError {
    fn from(value: String) -> Self {
        ApiError::internal_server_error(Some(&value))
    }
}

impl From<&str> for ApiError {
    fn from(value: &str) -> Self {
        ApiError::internal_server_error(Some(value))
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiError {
    #[serde(skip_serializing)]
    #[schema(ignore)]
    code: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.code, Json(self)).into_response()
    }
}

impl ApiError {
    pub fn internal_server_error(message: Option<&str>) -> Self {
        ApiError {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.unwrap_or("Internal server error").to_string(),
        }
    }

    pub fn not_found() -> Self {
        ApiError {
            code: StatusCode::NOT_FOUND,
            message: "Not Found".to_string(),
        }
    }

    pub fn bad_request(message: Option<&str>) -> Self {
        ApiError {
            code: StatusCode::BAD_REQUEST,
            message: message.unwrap_or("Bad request").to_string(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiResponse<T: Serialize + ToSchema> {
    data: T,
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Serialize + ToSchema,
{
    fn into_response(self) -> axum::response::Response {
        (http::StatusCode::OK, Json(self)).into_response()
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    println!("Valid Regions: {:?}", &config.valid_regions);

    let context = Context {
        pool: config.pool,
        valid_regions: config.valid_regions,
        auth_key: config.auth_key,
    };

    let (router, api) = init_router().split_for_parts();

    let scalar = Scalar::with_url("/docs", api);

    let app = router
        .merge(scalar)
        .layer(axum::middleware::from_fn_with_state(
            context.clone(),
            auth_middleware,
        ))
        .with_state(context);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;
    println!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new()
        .merge(tenants::init_router())
        .merge(publish::init_router())
        .merge(cron::init_router())
}

async fn auth_middleware(State(ctx): State<Context>, req: Request, next: Next) -> Response {
    if ctx.auth_key.is_none() {
        return next.run(req).await;
    }

    let expected_token = ctx.auth_key.unwrap();

    let header_token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    if let Some(token) = header_token
        && token == expected_token
    {
        next.run(req).await
    } else {
        ApiError {
            code: StatusCode::UNAUTHORIZED,
            message: "UNAUTHORIZED".to_string(),
        }
        .into_response()
    }
}
