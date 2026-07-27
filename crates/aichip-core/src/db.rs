use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

impl Db {
    /// Connect and run pending migrations. `database_url` comes either from
    /// the embedded Postgres the CLI boots, or the user's own DATABASE_URL.
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}
