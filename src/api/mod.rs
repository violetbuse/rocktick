mod cron;
mod one_off;
mod public;
mod tenant;

use poem::{Route, Server, handler, listener::TcpListener, web::Path};
use poem_openapi::OpenApiService;
use sqlx::{Pool, Postgres};

use crate::{ApiOptions, api::public::PublicApi};

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

pub async fn start(config: Config) -> anyhow::Result<()> {
    println!("Valid Regions: {:?}", &config.valid_regions);

    let public_api = OpenApiService::new(
        PublicApi { pool: config.pool },
        "Rocktick Api",
        "http://localhost:3000",
    );
    let ui = public_api.scalar();

    let app = Route::new().nest("/", public_api).nest("/docs", ui);

    println!("Api   http://localhost:{}/api", config.port);
    println!("Docs  http://localhost:{}/docs", config.port);

    Server::new(TcpListener::bind(format!("0.0.0.0:{}", config.port)))
        .run(app)
        .await?;

    Ok(())
}

#[handler]
fn hello(Path(name): Path<String>) -> String {
    format!("hello: {name}")
}
