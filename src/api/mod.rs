use poem::{Route, Server, handler, listener::TcpListener, web::Path};
use sqlx::{Pool, Postgres};

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

pub async fn start(config: Config) -> anyhow::Result<()> {
    println!("Valid Regions: {:?}", &config.valid_regions);

    let app = Route::new().at("/hello/:name", poem::get(hello));

    Server::new(TcpListener::bind(format!("0.0.0.0:{}", config.port)))
        .run(app)
        .await?;

    Ok(())
}

#[handler]
fn hello(Path(name): Path<String>) -> String {
    format!("hello: {name}")
}
