use std::str::FromStr;

use indoc::indoc;
use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use sqlx::{
    ConnectOptions, Pool, Postgres,
    postgres::{PgConnectOptions, PgPoolOptions},
};

include!(concat!(env!("OUT_DIR"), "/embedded_postgres_version.rs"));

pub async fn create_pool(postgres_url: String, count: u32) -> anyhow::Result<Pool<Postgres>> {
    let pool = PgPoolOptions::new()
        .min_connections(count)
        .connect(&postgres_url)
        .await?;

    let cleanup_pool = pool.clone();

    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        println!("Closing postgres connection.");
        cleanup_pool.close().await;
    });

    Ok(pool)
}

pub async fn migrate_pg(pool: &Pool<Postgres>) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations/postgres").run(pool).await?;

    Ok(())
}

pub async fn run_embedded(temporary: bool) -> anyhow::Result<String> {
    let mut data_dir = std::env::current_dir()?;
    data_dir.push(".rocktick");
    data_dir.push("pg");

    let settings = Settings {
        version: VersionReq::parse(EMBEDDED_POSTGRES_VERSION)
            .expect("Failed to parse embedded postgres version?"),
        password: "postgres".to_string(),
        data_dir,
        temporary,
        ..Default::default()
    };

    let mut postgresql = PostgreSQL::new(settings);

    let text = indoc! {r#"
      Starting Embedded Postgres...
      If you want to connect to an existing db, use one of:
        --postgres-url "...postgres url"
        DATABASE_URL="...postgres url"
    "#};
    println!("{}", text);
    postgresql.setup().await?;
    postgresql.start().await?;

    let db_name = "rocktick";
    if !postgresql.database_exists(db_name).await? {
        postgresql.create_database(db_name).await?;
    }

    let settings = postgresql.settings();
    let url = settings.url(db_name);

    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        println!("Stopping embedded postgres instance.");
        postgresql
            .stop()
            .await
            .expect("Failed to gracefully stop postgres.")
    });

    Ok(url)
}
