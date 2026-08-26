use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

/// A new product entity to insert: its SKU and its `type_id` (read from the
/// CSV's own `type_id` column, defaulting to "simple" -- see
/// `bucket_new_products` in `run.rs`).
pub struct NewProduct {
    pub sku: String,
    pub type_id: String,
}

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
    entries: &[NewProduct],
    attribute_set_id: u16,
    batch_size: usize,
) -> Result<HashMap<String, u64>, sqlx::Error> {
    let mut map = HashMap::with_capacity(entries.len());
    if entries.is_empty() {
        return Ok(map);
    }

    for chunk in entries.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) ");
        qb.push_values(chunk, |mut b, entry: &NewProduct| {
            b.push_bind(attribute_set_id).push_bind(&entry.type_id).push_bind(&entry.sku);
        });

        let result = qb.build().execute(pool).await?;
        let first_id = result.last_insert_id();
        for (i, entry) in chunk.iter().enumerate() {
            map.insert(entry.sku.clone(), first_id + i as u64);
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
        let entries = vec![
            NewProduct { sku: "RUST-IMPORT-TEST-ENTITIES-1".to_string(), type_id: "simple".to_string() },
            NewProduct { sku: "RUST-IMPORT-TEST-ENTITIES-2".to_string(), type_id: "simple".to_string() },
            NewProduct { sku: "RUST-IMPORT-TEST-ENTITIES-3".to_string(), type_id: "simple".to_string() },
        ];

        // Clean up any leftovers from a prior failed run before asserting.
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TEST-ENTITIES-%'")
            .execute(&pool)
            .await
            .unwrap();

        let inserted = insert_new_products(&pool, &entries, 4, 500).await.unwrap();

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
        let result = insert_new_products(&pool, &[], 4, 500).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn each_entry_gets_its_own_type_id() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        let entries = vec![
            NewProduct { sku: "RUST-IMPORT-TEST-TYPES-SIMPLE".to_string(), type_id: "simple".to_string() },
            NewProduct { sku: "RUST-IMPORT-TEST-TYPES-CONFIG".to_string(), type_id: "configurable".to_string() },
        ];
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TEST-TYPES-%'")
            .execute(&pool)
            .await
            .unwrap();

        insert_new_products(&pool, &entries, 4, 500).await.unwrap();

        let simple_type: String =
            sqlx::query_scalar("SELECT type_id FROM catalog_product_entity WHERE sku = ?")
                .bind("RUST-IMPORT-TEST-TYPES-SIMPLE")
                .fetch_one(&pool)
                .await
                .unwrap();
        let config_type: String =
            sqlx::query_scalar("SELECT type_id FROM catalog_product_entity WHERE sku = ?")
                .bind("RUST-IMPORT-TEST-TYPES-CONFIG")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(simple_type, "simple");
        assert_eq!(config_type, "configurable");

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TEST-TYPES-%'")
            .execute(&pool)
            .await
            .unwrap();
    }
}
