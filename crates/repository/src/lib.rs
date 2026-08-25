//! Repository layer — mirrors GoGento's `model/repository`: flattening EAV
//! rows into `attribute_code -> value` maps, plus the in-process flat cache.
//!
//! DB-touching fetch functions (querying `gogento-mysql`) are intentionally
//! thin and live alongside the pure logic below; they're exercised by
//! integration testing against a live database (Phase B), not unit tests,
//! since mocking sqlx's wire protocol wouldn't meaningfully test anything
//! beyond what the pure flatten/cache/batching logic already covers.

pub mod attribute_cache;
pub mod batching;
pub mod category_repo;
pub mod flat_cache;
pub mod product_repo;

pub use attribute_cache::AttributeCodeMap;
pub use batching::chunk_ids;
pub use category_repo::{flatten_category, CategoryAttributeMeta, CategoryEavRows};
pub use flat_cache::FlatCache;
pub use product_repo::{flatten_product, ProductEavRows};
