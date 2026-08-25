//! Shared test-only helper for connecting to a live MySQL instance.
//!
//! Mirrors Go's `magentoTestDB` in `tests/integration/magento_db_test.go`:
//! try to connect, and if that fails, skip the test rather than failing --
//! so `cargo test` still passes in a sandbox with no Docker/MySQL available,
//! while giving full real-database coverage whenever `gogento-mysql` (the
//! container this project's benchmarks run against) is reachable.

use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;
use std::time::Duration;

const DEFAULT_TEST_DATABASE_URL: &str = "mysql://magento:magento@127.0.0.1:3309/magento";

/// Short acquire timeout for the "is the dev DB up at all" probe -- the
/// default `sqlx` pool timeout (30s) would make every skipped test in this
/// crate take 30 seconds if `gogento-mysql` ever isn't running, multiplying
/// badly across dozens of call sites. A few seconds is plenty to distinguish
/// "not running" from "just slow to accept connections".
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn test_pool() -> Option<MySqlPool> {
    let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    connect_or_none(&url).await
}

/// Split out from [`test_pool`] so the "connection failed -> skip" path can
/// be exercised directly with a deliberately unreachable URL, without
/// touching the shared `GOGENTO_TEST_DATABASE_URL` env var (which every
/// other test in this crate also reads).
async fn connect_or_none(url: &str) -> Option<MySqlPool> {
    match MySqlPoolOptions::new().acquire_timeout(PROBE_TIMEOUT).connect(url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            eprintln!("skipping: cannot connect to test database at {url}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_or_none_returns_none_on_connection_failure() {
        // Port 1 is a reserved, effectively always-unbound port, so this
        // fails fast (connection refused) rather than hanging.
        let result = connect_or_none("mysql://user:pass@127.0.0.1:1/nonexistent").await;
        assert!(result.is_none());
    }
}
