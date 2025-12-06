mod tenants;

use axum::{Json, extract::rejection::JsonRejection, response::IntoResponse};

use axum_macros::FromRequest;
use http::StatusCode;
use serde::Serialize;
use sqlx::{Pool, Postgres};
use utoipa::{
    OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::ApiOptions;

#[derive(Debug, Clone)]
pub struct Config {
    port: usize,
    pool: Pool<Postgres>,
    valid_regions: Vec<String>,
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct Context {
    pub pool: Pool<Postgres>,
    pub valid_regions: Vec<String>,
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
    };

    let (router, api) = init_router().split_for_parts();

    let scalar = Scalar::with_url("/docs", api);

    let app = router.merge(scalar).with_state(context);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;
    println!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new().merge(tenants::init_router())
}
