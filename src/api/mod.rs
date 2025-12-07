mod cron;
mod executions;
mod publish;
mod tenants;
mod verify;

use axum::{
    Json, Router,
    extract::{FromRequest, FromRequestParts, Request, State, rejection::JsonRejection},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};

use futures::never::Never;
use http::StatusCode;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Pool, Postgres};
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::{
        OpenApi as OpenApiSpec,
        security::{HttpBuilder, SecurityScheme},
    },
};
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

#[derive(OpenApi)]
#[openapi(
  info(title = "Rocktick",),
  components(),
  security(("bearer_auth" = [])),
  modifiers(&BearerAuth)
)]
struct MyOpenApiSpec;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                    .build(),
            ),
        );
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
pub struct ApiListResponse<T: Serialize + ToSchema> {
    data: Vec<T>,
    count: usize,
    cursor: Option<String>,
}

impl<T> IntoResponse for ApiListResponse<T>
where
    T: Serialize + ToSchema,
{
    fn into_response(self) -> axum::response::Response {
        (http::StatusCode::OK, Json(self)).into_response()
    }
}

#[derive(Debug, Clone)]
pub struct TenantId(Option<String>);

impl<S> FromRequestParts<S> for TenantId
where
    S: Send + Sync,
{
    type Rejection = Never;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(TenantId(
            parts
                .headers
                .get("tenant-id")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string()),
        ))
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    println!("Valid Regions: {:?}", &config.valid_regions);

    let context = Context {
        pool: config.pool,
        valid_regions: config.valid_regions,
        auth_key: config.auth_key,
    };

    let router = create_router();
    let spec = create_spec();

    let scalar = Scalar::with_url("/docs", spec);

    let app = router
        .layer(axum::middleware::from_fn_with_state(
            context.clone(),
            auth_middleware,
        ))
        .route("/docs/openapi.json", get(openapi_json))
        .merge(scalar)
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
        .merge(verify::init_router())
        .merge(executions::init_router())
}

fn create_router() -> Router<Context> {
    let (router, _) = init_router().split_for_parts();

    router
}

fn create_spec() -> OpenApiSpec {
    let (_, spec) = init_router().split_for_parts();

    MyOpenApiSpec::openapi().merge_from(spec)
}

async fn openapi_json() -> Json<Value> {
    let spec = create_spec();
    Json(serde_json::to_value(spec).unwrap())
}

async fn auth_middleware(State(ctx): State<Context>, req: Request, next: Next) -> Response {
    if ctx.auth_key.is_none() {
        return next.run(req).await;
    }

    if req.uri().path().starts_with("/docs") {
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
