use anyhow::anyhow;
use sqlx::sqlite::SqlitePoolOptions;
use std::{path::PathBuf, time::Duration};
use tokio::fs;

use sqlx::{
    Pool, Sqlite, migrate,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};

pub mod executions;

#[derive(Debug, Clone)]
pub struct DroneStore {
    pool: Pool<Sqlite>,
}

impl DroneStore {
    async fn run_migrations(&self) -> anyhow::Result<()> {
        migrate!("migrations/sqlite")
            .run(&self.pool)
            .await
            .map_err(|err| anyhow!("Error running drone store migrations: {:?}", err))?;

        Ok(())
    }

    pub async fn in_memory(name: &str) -> anyhow::Result<Self> {
        let connection_options = SqliteConnectOptions::new()
            .create_if_missing(true)
            .foreign_keys(true)
            .filename(format!("file:in_memory_{}", name))
            .in_memory(true)
            .shared_cache(true);

        let conn = SqlitePoolOptions::new()
            .min_connections(3)
            .max_connections(10)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(connection_options)
            .await?;

        let store = Self { pool: conn };
        store.run_migrations().await?;
        Ok(store)
    }

    pub fn default_store_location() -> anyhow::Result<PathBuf> {
        let mut datastore_location = std::env::current_dir()?;
        datastore_location.push(".rocktick");
        datastore_location.push("drone.db");

        Ok(datastore_location)
    }

    fn connect_options() -> SqliteConnectOptions {
        SqliteConnectOptions::new()
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10))
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
    }

    pub async fn from_filename(store_location: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = store_location.parent() {
            fs::create_dir_all(parent).await?;
        }

        let connection_options = Self::connect_options().filename(store_location);
        let conn = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connection_options)
            .await?;

        let store = Self { pool: conn };
        store.run_migrations().await?;
        Ok(store)
    }
}
