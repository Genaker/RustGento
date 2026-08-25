use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

/// Bulk-inserts new `catalog_product_entity` rows for SKUs that don't
/// already exist, returning the newly assigned `sku -> entity_id` mapping.
///
/// Relies on MySQL/InnoDB's guarantee that a single multi-row `INSERT` into
/// an `AUTO_INCREMENT` primary key produces *consecutive* IDs starting from
/// the one `LAST_INSERT_ID()` reports for that statement (the default
/// `innodb_autoinc_lock_type=1` "consecutive" lock mode) -- the same trick
/// Go's CE-path bulk insert relies on, just via GORM's batch `Create` instead
/// of a hand-built statement.
pub async fn insert_new_products(
    pool: &MySqlPool,
    skus: &[String],
    type_id: &str,
    attribute_set_id: u16,
    batch_size: usize,
) -> Result<HashMap<String, u32>, sqlx::Error> {
    let mut map = HashMap::with_capacity(skus.len());
    if skus.is_empty() {
        return Ok(map);
    }

    for chunk in skus.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) ");
        qb.push_values(chunk, |mut b, sku: &String| {
            b.push_bind(attribute_set_id).push_bind(type_id).push_bind(sku);
        });

        let result = qb.build().execute(pool).await?;
        let first_id = result.last_insert_id() as u32;
        for (i, sku) in chunk.iter().enumerate() {
            map.insert(sku.clone(), first_id + i as u32);
        }
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inserts_new_products_and_assigns_consecutive_ids() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        // Unique, timestamp-free SKUs so repeated test runs don't collide --
        // random/time-based values are avoided per workflow constraints, so
        // use a fixed, clearly-scoped test prefix and clean up afterward.
        let skus = vec![
            "RUST-IMPORT-TEST-ENTITIES-1".to_string(),
            "RUST-IMPORT-TEST-ENTITIES-2".to_string(),
            "RUST-IMPORT-TEST-ENTITIES-3".to_string(),
        ];

        // Clean up any leftovers from a prior failed run before asserting.
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TEST-ENTITIES-%'")
            .execute(&pool)
            .await
            .unwrap();

        let inserted = insert_new_products(&pool, &skus, "simple", 4, 500).await.unwrap();

        assert_eq!(inserted.len(), 3);
        let id1 = inserted["RUST-IMPORT-TEST-ENTITIES-1"];
        let id2 = inserted["RUST-IMPORT-TEST-ENTITIES-2"];
        let id3 = inserted["RUST-IMPORT-TEST-ENTITIES-3"];
        assert_eq!(id2, id1 + 1, "consecutive IDs within one multi-row insert");
        assert_eq!(id3, id1 + 2);

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TEST-ENTITIES-%'")
            .execute(&pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn empty_input_inserts_nothing() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let result = insert_new_products(&pool, &[], "simple", 4, 500).await.unwrap();
        assert!(result.is_empty());
    }
}
