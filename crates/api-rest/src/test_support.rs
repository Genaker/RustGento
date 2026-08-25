//! Shared test-only helper for connecting to a live MySQL instance and
//! building a real [`AppState`] -- mirrors `import::test_support` (see that
//! module's docs for rationale).

use crate::state::AppState;
use sqlx::mysql::MySqlPoolOptions;
use std::time::Duration;

const DEFAULT_TEST_DATABASE_URL: &str = "mysql://magento:magento@127.0.0.1:3309/magento";
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn test_state() -> Option<AppState> {
    let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    let pool = MySqlPoolOptions::new().acquire_timeout(PROBE_TIMEOUT).connect(&url).await.ok()?;
    AppState::new(pool).await.ok()
}
