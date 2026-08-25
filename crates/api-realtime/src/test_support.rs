//! Shared test-only helper for connecting to a live MySQL instance --
//! mirrors `import::test_support` (see that module's docs for rationale).

use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;
use std::time::Duration;

const DEFAULT_TEST_DATABASE_URL: &str = "mysql://magento:magento@127.0.0.1:3309/magento";
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn test_pool() -> Option<MySqlPool> {
    let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    match MySqlPoolOptions::new().acquire_timeout(PROBE_TIMEOUT).connect(&url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            eprintln!("skipping: cannot connect to test database at {url}: {e}");
            None
        }
    }
}
