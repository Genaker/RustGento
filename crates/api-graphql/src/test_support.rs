//! Shared test-only helper for connecting to a live MySQL instance and
//! building a real [`GraphQLContext`] -- mirrors `import::test_support` (see
//! that module's docs for rationale).

use crate::context::GraphQLContext;
use crate::schema::{build_schema, GogentoSchema};
use repository::{category_db, product_db, FlatCache};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_TEST_DATABASE_URL: &str = "mysql://magento:magento@127.0.0.1:3309/magento";
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

async fn test_pool() -> Option<MySqlPool> {
    let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string());
    match MySqlPoolOptions::new().acquire_timeout(PROBE_TIMEOUT).connect(&url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            eprintln!("skipping: cannot connect to test database at {url}: {e}");
            None
        }
    }
}

/// Builds a real schema backed by a live DB connection, or `None` if
/// unreachable (tests should skip gracefully in that case).
pub async fn test_schema() -> Option<GogentoSchema> {
    let pool = test_pool().await?;
    let product_code_map = product_db::load_attribute_code_map(&pool).await.ok()?;
    let category_meta = category_db::load_attribute_meta(&pool).await.ok()?;
    let context = GraphQLContext {
        pool,
        product_cache: Arc::new(FlatCache::new()),
        category_cache: Arc::new(FlatCache::new()),
        product_code_map: Arc::new(product_code_map),
        category_meta: Arc::new(category_meta),
        product_flat_cache_enabled: true,
    };
    Some(build_schema(context))
}
