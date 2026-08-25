use repository::{AttributeCodeMap, CategoryAttributeMeta, FlatCache};
use sqlx::MySqlPool;
use std::sync::Arc;

/// Shared application state, cloned cheaply (everything inside is an `Arc`
/// or a `sqlx` pool, which is itself an `Arc` internally) into every handler
/// via axum's `State` extractor.
#[derive(Clone)]
pub struct AppState {
    pub pool: MySqlPool,
    pub product_cache: Arc<FlatCache>,
    pub category_cache: Arc<FlatCache>,
    pub product_code_map: Arc<AttributeCodeMap>,
    pub category_meta: Arc<CategoryAttributeMeta>,
    pub product_flat_cache_enabled: bool,
}

impl AppState {
    pub async fn new(pool: MySqlPool) -> Result<Self, sqlx::Error> {
        let product_code_map = repository::product_db::load_attribute_code_map(&pool).await?;
        let category_meta = repository::category_db::load_attribute_meta(&pool).await?;
        Ok(AppState {
            pool,
            product_cache: Arc::new(FlatCache::new()),
            category_cache: Arc::new(FlatCache::new()),
            product_code_map: Arc::new(product_code_map),
            category_meta: Arc::new(category_meta),
            product_flat_cache_enabled: config::product_flat_cache_enabled(),
        })
    }
}
