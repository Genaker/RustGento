use repository::{AttributeCodeMap, CategoryAttributeMeta, FlatCache};
use sqlx::MySqlPool;
use std::sync::Arc;

/// Data injected into every GraphQL resolver's context -- the GraphQL-layer
/// analog of `api_rest::state::AppState`.
#[derive(Clone)]
pub struct GraphQLContext {
    pub pool: MySqlPool,
    pub product_cache: Arc<FlatCache>,
    pub category_cache: Arc<FlatCache>,
    pub product_code_map: Arc<AttributeCodeMap>,
    pub category_meta: Arc<CategoryAttributeMeta>,
    pub product_flat_cache_enabled: bool,
}

/// Per-request store ID, resolved by [`crate::store::resolve_store_id`]
/// before GraphQL execution and injected via `Request::data` -- distinct
/// from `GraphQLContext`, which is shared schema-level (`Schema::data`)
/// state, not per-request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreId(pub u16);
