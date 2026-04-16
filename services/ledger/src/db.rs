use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Config;

pub async fn connect(config: &Config) -> Result<PgPool, sqlx::Error> {
    // Postgres inside docker-compose can take several seconds to finish its
    // first-boot cluster init. Retry connection a handful of times before giving up.
    let mut last_err: Option<sqlx::Error> = None;
    for attempt in 1..=15 {
        match PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(Duration::from_secs(2))
            .connect(&config.database_url)
            .await
        {
            Ok(pool) => {
                sqlx::migrate!("./migrations").run(&pool).await?;
                tracing::info!("database connected and migrated");
                return Ok(pool);
            }
            Err(e) => {
                tracing::warn!(attempt, error = %e, "db connect failed, retrying");
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
    Err(last_err.unwrap())
}
