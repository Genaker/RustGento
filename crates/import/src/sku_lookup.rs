use sqlx::MySqlPool;
use std::collections::HashMap;

/// Batch-resolves `sku -> entity_id` for already-existing products, chunking
/// the `IN (...)` lookup at `batch_size` to stay well under MySQL's bound
/// parameter ceiling -- same role as Go's batched `SELECT entity_id, sku ...
/// WHERE sku IN (...)` loop in `import_service.go`.
pub async fn lookup_existing_skus(
    pool: &MySqlPool,
    skus: &[String],
    batch_size: usize,
) -> Result<HashMap<String, u32>, sqlx::Error> {
    let mut map = HashMap::with_capacity(skus.len());
    if skus.is_empty() {
        return Ok(map);
    }

    for chunk in skus.chunks(batch_size.max(1)) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let sql = format!("SELECT entity_id, sku FROM catalog_product_entity WHERE sku IN ({placeholders})");

        let mut query = sqlx::query_as::<_, (u32, String)>(&sql);
        for sku in chunk {
            query = query.bind(sku);
        }
        let rows = query.fetch_all(pool).await?;
        for (entity_id, sku) in rows {
            map.insert(sku, entity_id);
        }
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_input_short_circuits_without_a_query() {
        // An empty `skus` slice must not run a `WHERE sku IN ()` query,
        // which is invalid SQL. `connect_lazy` builds a pool without
        // actually connecting, so this proves the early return happens
        // before any real `sqlx` call reaches the network -- no live DB
        // needed for this specific case.
        let pool = MySqlPool::connect_lazy("mysql://user:pass@127.0.0.1:1/db").unwrap();
        let result = lookup_existing_skus(&pool, &[], 500).await.unwrap();
        assert!(result.is_empty());
    }

    /// Exercises the real batched lookup against `gogento-mysql`. Skips
    /// gracefully (matching Go's `t.Skip` pattern in `doc/testing.md`) when
    /// `GOGENTO_TEST_DATABASE_URL` isn't set, so `cargo test` still passes
    /// with no DB available.
    #[tokio::test]
    async fn looks_up_existing_skus_against_live_db() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        // The seed data (~/GoGento/cmd/seed) always creates SAMPLE-SKU-0000.
        let skus = vec!["SAMPLE-SKU-0000".to_string(), "DEFINITELY-NOT-A-REAL-SKU".to_string()];
        let found = lookup_existing_skus(&pool, &skus, 500).await.unwrap();

        assert!(found.contains_key("SAMPLE-SKU-0000"));
        assert!(!found.contains_key("DEFINITELY-NOT-A-REAL-SKU"));
    }
}
