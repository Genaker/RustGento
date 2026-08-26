use crate::attribute_cache::AttributeCodeMap;
use crate::flat_cache::FlatCache;
use crate::product_repo::{flatten_product, ProductEavRows};
use entity::{
    EavAttribute, Product, ProductDatetime, ProductDecimal, ProductIndexPrice, ProductInt,
    ProductText, ProductVarchar, StockItem, PRODUCT_ENTITY_TYPE_ID,
};
use serde_json::Value;
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::hash::Hash;

/// Chunk size for `WHERE x IN (...)` batch queries -- keeps well under
/// MySQL's bound-parameter ceiling, same role as Go's 1000-row batching.
const BATCH_SIZE: usize = 500;

pub async fn load_attribute_code_map(pool: &MySqlPool) -> Result<AttributeCodeMap, sqlx::Error> {
    let attrs: Vec<EavAttribute> = sqlx::query_as("SELECT * FROM eav_attribute WHERE entity_type_id = ?")
        .bind(PRODUCT_ENTITY_TYPE_ID)
        .fetch_all(pool)
        .await?;
    Ok(AttributeCodeMap::build(&attrs))
}

pub async fn find_all(pool: &MySqlPool, limit: usize) -> Result<Vec<Product>, sqlx::Error> {
    if limit > 0 {
        sqlx::query_as("SELECT * FROM catalog_product_entity ORDER BY entity_id LIMIT ?")
            .bind(limit as i64)
            .fetch_all(pool)
            .await
    } else {
        sqlx::query_as("SELECT * FROM catalog_product_entity ORDER BY entity_id").fetch_all(pool).await
    }
}

pub async fn find_by_id(pool: &MySqlPool, id: u64) -> Result<Option<Product>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM catalog_product_entity WHERE entity_id = ?").bind(id).fetch_optional(pool).await
}

/// Full-catalog search: matches SKU (a plain column) or the "name" EAV
/// attribute (passed in as `name_attribute_id` -- the caller resolves it
/// once rather than this function re-querying `eav_attribute` on every
/// search) against a case-insensitive substring, ordered by entity_id.
pub async fn search_ids(pool: &MySqlPool, name_attribute_id: u16, query: &str) -> Result<Vec<u64>, sqlx::Error> {
    let like = format!("%{query}%");
    sqlx::query_scalar(
        "SELECT DISTINCT p.entity_id FROM catalog_product_entity p \
         LEFT JOIN catalog_product_entity_varchar v ON v.entity_id = p.entity_id AND v.attribute_id = ? \
         WHERE p.sku LIKE ? OR v.value LIKE ? \
         ORDER BY p.entity_id",
    )
    .bind(name_attribute_id)
    .bind(&like)
    .bind(&like)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Clone)]
pub struct ProductInput {
    pub attribute_set_id: u16,
    pub type_id: String,
    pub sku: String,
    pub has_options: i16,
    pub required_options: u16,
}

pub async fn create(pool: &MySqlPool, input: &ProductInput) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku, has_options, required_options) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(input.attribute_set_id)
    .bind(&input.type_id)
    .bind(&input.sku)
    .bind(input.has_options)
    .bind(input.required_options)
    .execute(pool)
    .await?;
    Ok(result.last_insert_id())
}

pub async fn update(pool: &MySqlPool, id: u64, input: &ProductInput) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE catalog_product_entity SET attribute_set_id = ?, type_id = ?, sku = ?, \
         has_options = ?, required_options = ? WHERE entity_id = ?",
    )
    .bind(input.attribute_set_id)
    .bind(&input.type_id)
    .bind(&input.sku)
    .bind(input.has_options)
    .bind(input.required_options)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete(pool: &MySqlPool, id: u64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM catalog_product_entity WHERE entity_id = ?").bind(id).execute(pool).await?;
    Ok(result.rows_affected() > 0)
}

/// Runs `{table_and_where} (?,?,...)` for `ids`, chunked, and groups the
/// resulting rows by `key_of(row)`. Generic over the key type `K` because
/// Magento's own schema isn't uniform here: most `entity_id`/`value_id`
/// columns are `bigint unsigned` (`u64`), but e.g. `catalog_category_product.product_id`
/// is `int unsigned` (`u32`) -- both need to key a batch group.
pub(crate) async fn batch_by_ids<T, K, F>(pool: &MySqlPool, table_and_where: &str, ids: &[K], key_of: F) -> Result<HashMap<K, Vec<T>>, sqlx::Error>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::mysql::MySqlRow> + Send + Unpin,
    K: Eq + Hash + Copy + Send + Sync + for<'q> sqlx::Encode<'q, sqlx::MySql> + sqlx::Type<sqlx::MySql>,
    F: Fn(&T) -> K,
{
    let mut grouped: HashMap<K, Vec<T>> = HashMap::new();
    if ids.is_empty() {
        return Ok(grouped);
    }
    for chunk in ids.chunks(BATCH_SIZE) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!("{table_and_where} ({placeholders})");
        let mut query = sqlx::query_as::<_, T>(&sql);
        for id in chunk {
            query = query.bind(*id);
        }
        let rows = query.fetch_all(pool).await?;
        for row in rows {
            grouped.entry(key_of(&row)).or_default().push(row);
        }
    }
    Ok(grouped)
}

/// Batch-fetches every EAV/stock/price/category-link row for `products` and
/// flattens each one, keyed by `entity_id`. Used by both the single-item and
/// list fetch paths so a page of N products costs a fixed ~8 queries total,
/// not 8*N (the naive per-product-N+1 shape).
async fn batch_flatten(
    pool: &MySqlPool,
    code_map: &AttributeCodeMap,
    products: &[Product],
) -> Result<HashMap<u64, Value>, sqlx::Error> {
    let ids: Vec<u64> = products.iter().map(|p| p.entity_id).collect();

    let varchar = batch_by_ids::<ProductVarchar, _, _>(pool, "SELECT * FROM catalog_product_entity_varchar WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let int = batch_by_ids::<ProductInt, _, _>(pool, "SELECT * FROM catalog_product_entity_int WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let decimal = batch_by_ids::<ProductDecimal, _, _>(pool, "SELECT * FROM catalog_product_entity_decimal WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let text = batch_by_ids::<ProductText, _, _>(pool, "SELECT * FROM catalog_product_entity_text WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let datetime = batch_by_ids::<ProductDatetime, _, _>(pool, "SELECT * FROM catalog_product_entity_datetime WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let stock = batch_by_ids::<StockItem, _, _>(pool, "SELECT * FROM cataloginventory_stock_item WHERE product_id IN", &ids, |r| r.product_id).await?;
    let prices = batch_by_ids::<ProductIndexPrice, _, _>(pool, "SELECT * FROM catalog_product_index_price WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let category_links = batch_by_ids::<entity::category::CategoryProduct, _, _>(
        pool,
        "SELECT * FROM catalog_category_product WHERE product_id IN",
        &ids,
        |r| r.product_id as u64,
    )
    .await?;

    let mut out = HashMap::with_capacity(products.len());
    for product in products {
        let id = product.entity_id;
        let rows = ProductEavRows {
            varchar: varchar.get(&id).cloned().unwrap_or_default(),
            int: int.get(&id).cloned().unwrap_or_default(),
            decimal: decimal.get(&id).cloned().unwrap_or_default(),
            text: text.get(&id).cloned().unwrap_or_default(),
            datetime: datetime.get(&id).cloned().unwrap_or_default(),
        };
        let category_ids: Vec<u64> = category_links.get(&id).map(|links| links.iter().map(|l| l.category_id).collect()).unwrap_or_default();
        let stock_item = stock.get(&id).and_then(|v| v.first());
        let index_prices = prices.get(&id).cloned().unwrap_or_default();

        let flat = flatten_product(product, &rows, code_map, &category_ids, stock_item, &index_prices);
        out.insert(id, Value::Object(flat));
    }
    Ok(out)
}

/// Fetches and flattens a single product, read-through the cache keyed by
/// `(store_id, entity_id)`. A miss is filled in on success.
pub async fn fetch_flat_by_id(
    pool: &MySqlPool,
    cache: &FlatCache,
    code_map: &AttributeCodeMap,
    store_id: u16,
    entity_id: u64,
    bypass_cache: bool,
) -> Result<Option<Value>, sqlx::Error> {
    if let Some(cached) = cache.get(bypass_cache, store_id, entity_id) {
        return Ok(Some(cached));
    }
    let Some(product) = find_by_id(pool, entity_id).await? else { return Ok(None) };
    let flattened = batch_flatten(pool, code_map, std::slice::from_ref(&product)).await?;
    let Some(flat) = flattened.into_values().next() else { return Ok(None) };
    cache.put(bypass_cache, store_id, entity_id, flat.clone());
    Ok(Some(flat))
}

/// Fetches and flattens up to `limit` products (`0` = unbounded), ordered by
/// `entity_id`. Matches Go's documented behavior: a limited fetch bypasses
/// (and does not populate) the cache, since a partial page shouldn't poison
/// the full-set cache. `force_bypass` additionally bypasses regardless of
/// `limit` -- the caller passes `!config::product_flat_cache_enabled()`
/// (`PRODUCT_FLAT_CACHE=off`), keeping this crate decoupled from `config`.
pub async fn fetch_flat_list(
    pool: &MySqlPool,
    cache: &FlatCache,
    code_map: &AttributeCodeMap,
    store_id: u16,
    limit: usize,
    force_bypass: bool,
) -> Result<Vec<Value>, sqlx::Error> {
    let bypass_cache = force_bypass || limit > 0;
    let products = find_all(pool, limit).await?;
    if products.is_empty() {
        return Ok(Vec::new());
    }

    let mut results: HashMap<u64, Value> = HashMap::new();
    let mut missing = Vec::new();
    for p in &products {
        match cache.get(bypass_cache, store_id, p.entity_id) {
            Some(cached) => {
                results.insert(p.entity_id, cached);
            }
            None => missing.push(p.clone()),
        }
    }

    if !missing.is_empty() {
        let fetched = batch_flatten(pool, code_map, &missing).await?;
        for (id, flat) in fetched {
            cache.put(bypass_cache, store_id, id, flat.clone());
            results.insert(id, flat);
        }
    }

    Ok(products.iter().filter_map(|p| results.get(&p.entity_id).cloned()).collect())
}

/// Fetches and flattens a specific set of products by ID, preserving the
/// order and duplicates of `ids` (unknown IDs are simply absent from the
/// result) -- matches the REST `/flat/:ids` and GraphQL `skus`-filter shape.
pub async fn fetch_flat_by_ids(
    pool: &MySqlPool,
    cache: &FlatCache,
    code_map: &AttributeCodeMap,
    store_id: u16,
    ids: &[u64],
) -> Result<Vec<Value>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut results: HashMap<u64, Value> = HashMap::new();
    let mut missing_ids = Vec::new();
    for &id in ids {
        if let Some(cached) = cache.get(false, store_id, id) {
            results.insert(id, cached);
        } else if !missing_ids.contains(&id) {
            missing_ids.push(id);
        }
    }

    if !missing_ids.is_empty() {
        let products = batch_by_ids::<Product, _, _>(pool, "SELECT * FROM catalog_product_entity WHERE entity_id IN", &missing_ids, |p| p.entity_id)
            .await?
            .into_values()
            .flatten()
            .collect::<Vec<_>>();
        let fetched = batch_flatten(pool, code_map, &products).await?;
        for (id, flat) in fetched {
            cache.put(false, store_id, id, flat.clone());
            results.insert(id, flat);
        }
    }

    Ok(ids.iter().filter_map(|id| results.get(id).cloned()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn crud_round_trip_against_live_db() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-REPO-CRUD-TEST'").execute(&pool).await.unwrap();

        let input = ProductInput {
            attribute_set_id: 4,
            type_id: "simple".into(),
            sku: "RUST-REPO-CRUD-TEST".into(),
            has_options: 0,
            required_options: 0,
        };
        let id = create(&pool, &input).await.unwrap();

        let found = find_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(found.sku, "RUST-REPO-CRUD-TEST");

        let updated_input = ProductInput { sku: "RUST-REPO-CRUD-TEST-RENAMED".into(), ..input };
        assert!(update(&pool, id, &updated_input).await.unwrap());
        let found = find_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(found.sku, "RUST-REPO-CRUD-TEST-RENAMED");

        assert!(delete(&pool, id).await.unwrap());
        assert!(find_by_id(&pool, id).await.unwrap().is_none());
        assert!(!delete(&pool, id).await.unwrap(), "deleting an already-gone id reports not-found");
    }

    #[tokio::test]
    async fn search_ids_matches_sku_and_name() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-REPO-SEARCH-TEST'").execute(&pool).await.unwrap();
        let id = create(
            &pool,
            &ProductInput { attribute_set_id: 4, type_id: "simple".into(), sku: "RUST-REPO-SEARCH-TEST".into(), has_options: 0, required_options: 0 },
        )
        .await
        .unwrap();

        let name_attr_id: u16 = sqlx::query_scalar("SELECT attribute_id FROM eav_attribute WHERE entity_type_id = 4 AND attribute_code = 'name'").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO catalog_product_entity_varchar (attribute_id, store_id, entity_id, value) VALUES (?, 0, ?, 'Repository Search Widget')")
            .bind(name_attr_id)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        let by_sku = search_ids(&pool, name_attr_id, "REPO-SEARCH-TEST").await.unwrap();
        assert!(by_sku.contains(&id), "should match by SKU substring");

        let by_name = search_ids(&pool, name_attr_id, "Search Widget").await.unwrap();
        assert!(by_name.contains(&id), "should match by name substring");

        let no_match = search_ids(&pool, name_attr_id, "definitely-not-a-real-query-xyz").await.unwrap();
        assert!(!no_match.contains(&id));

        sqlx::query("DELETE FROM catalog_product_entity_varchar WHERE entity_id = ?").bind(id).execute(&pool).await.unwrap();
        delete(&pool, id).await.unwrap();
    }

    #[tokio::test]
    async fn fetch_flat_by_id_returns_none_for_unknown_id() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let cache = FlatCache::new();
        let code_map = load_attribute_code_map(&pool).await.unwrap();
        let result = fetch_flat_by_id(&pool, &cache, &code_map, 0, 999_999_999, true).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn fetch_flat_list_and_by_ids_agree_and_populate_cache() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let cache = FlatCache::new();
        let code_map = load_attribute_code_map(&pool).await.unwrap();

        let list = fetch_flat_list(&pool, &cache, &code_map, 0, 5, false).await.unwrap();
        assert!(!list.is_empty(), "seed data must have at least a few products");
        // limit > 0 must bypass the cache per Go's documented behavior.
        assert_eq!(cache.len_for_store(0), 0);

        let ids: Vec<u64> = list.iter().map(|v| v["entity_id"].as_u64().unwrap()).collect();
        let by_ids = fetch_flat_by_ids(&pool, &cache, &code_map, 0, &ids).await.unwrap();
        assert_eq!(by_ids.len(), ids.len());
        assert_eq!(cache.len_for_store(0), ids.len(), "by-id fetch populates the cache");

        for (a, b) in list.iter().zip(by_ids.iter()) {
            assert_eq!(a["sku"], b["sku"]);
        }
    }

    #[tokio::test]
    async fn force_bypass_prevents_cache_population_even_with_unbounded_limit() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let cache = FlatCache::new();
        let code_map = load_attribute_code_map(&pool).await.unwrap();

        // limit=0 (unbounded) would normally populate the cache; force_bypass=true
        // (PRODUCT_FLAT_CACHE=off) must override that.
        let list = fetch_flat_list(&pool, &cache, &code_map, 0, 0, true).await.unwrap();
        assert!(!list.is_empty());
        assert_eq!(cache.len_for_store(0), 0);
    }
}
