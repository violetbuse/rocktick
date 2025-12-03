use std::time::Duration;

use poem::{Route, Server, handler, listener::TcpListener, web::Path};
use sqlx::{Pool, Postgres};

use crate::ApiOptions;

#[derive(Debug, Clone)]
pub struct Config {
    port: usize,
    pool: Pool<Postgres>,
}

impl Config {
    pub async fn from_cli(options: ApiOptions, pool: Pool<Postgres>) -> Self {
        Self {
            port: options.port,
            pool,
        }
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_secs(rand::random_range(0..10))).await;

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
