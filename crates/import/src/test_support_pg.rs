//! Shared test-only helper for connecting to a live Postgres instance.
//!
//! Mirrors `test_support.rs`'s MySQL helper: try to connect, and if that
//! fails, skip the test rather than failing -- so `cargo test` still passes
//! without Docker/Postgres available, while giving full coverage against
//! `gogento-postgres` (the container `sql/postgres_schema.sql` is applied
//! to) whenever it's up.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

const DEFAULT_TEST_DATABASE_URL: &str = "postgres://magento:magento@127.0.0.1:5435/magento";

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn test_pool() -> Option<PgPool> {
    let url = std::env::var("GOGENTO_TEST_POSTGRES_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    match PgPoolOptions::new().acquire_timeout(PROBE_TIMEOUT).connect(&url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            eprintln!("skipping: cannot connect to test database at {url}: {e}");
            None
        }
    }
}
