use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

pub async fn create_pool(postgres_url: String) -> anyhow::Result<Pool<Postgres>> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .min_connections(2)
        .connect(&postgres_url)
        .await?)
}

pub async fn migrate_pg(pool: &Pool<Postgres>) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;

    Ok(())
}

pub async fn run_embedded(temporary: bool) -> anyhow::Result<String> {
    // let mut settings = Settings::default();

    let mut data_dir = std::env::current_dir()?;
    data_dir.push(".rocktick");
    data_dir.push("pg");
    // settings.data_dir = data_dir;
    // settings.temporary = temporary;
    // settings.password = "postgres".to_string();

    let settings = Settings {
        version: VersionReq::parse(env!("POSTGRESQL_VERSION"))?,
        password: "postgres".to_string(),
        data_dir,
        temporary,
        ..Default::default()
    };

    let mut postgresql = PostgreSQL::new(settings);

    println!("Starting Postgres...");
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
