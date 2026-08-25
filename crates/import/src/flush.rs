use crate::eav_bucket::EavValue;
use chrono::NaiveDateTime;
use entity::{ProductIndexPrice, StockItem};
use sqlx::{MySql, MySqlPool, QueryBuilder};

/// Generates one `flush_<name>` function per EAV value table. All five
/// tables share an identical shape (`entity_id, attribute_id, store_id,
/// value`) and upsert rule (`value = VALUES(value)`); only the table name and
/// value type differ, so a macro removes the repetition without fighting
/// `sqlx::query_builder::Separated`'s lifetime parameters across a generic
/// function boundary (the two realistic alternatives: five hand-written
/// near-duplicates, or a generic fn whose closure-parameter type can't be
/// named cleanly -- the macro is the least awkward of the three).
macro_rules! impl_eav_flush {
    ($fn_name:ident, $table:literal, $value_ty:ty) => {
        #[doc = concat!("Batched upsert into `", $table, "`.")]
        pub async fn $fn_name(
            pool: &MySqlPool,
            rows: &[EavValue<$value_ty>],
            batch_size: usize,
        ) -> Result<(), sqlx::Error> {
            if rows.is_empty() {
                return Ok(());
            }
            for chunk in rows.chunks(batch_size.max(1)) {
                let mut qb: QueryBuilder<MySql> = QueryBuilder::new(concat!(
                    "INSERT INTO ",
                    $table,
                    " (entity_id, attribute_id, store_id, value) "
                ));
                qb.push_values(chunk, |mut b, row: &EavValue<$value_ty>| {
                    b.push_bind(row.entity_id)
                        .push_bind(row.attribute_id)
                        .push_bind(row.store_id)
                        .push_bind(row.value.clone());
                });
                qb.push(" ON DUPLICATE KEY UPDATE value = VALUES(value)");
                qb.build().execute(pool).await?;
            }
            Ok(())
        }
    };
}

impl_eav_flush!(flush_varchar, "catalog_product_entity_varchar", String);
impl_eav_flush!(flush_int, "catalog_product_entity_int", i32);
impl_eav_flush!(flush_decimal, "catalog_product_entity_decimal", f64);
impl_eav_flush!(flush_text, "catalog_product_entity_text", String);
impl_eav_flush!(flush_datetime, "catalog_product_entity_datetime", NaiveDateTime);

/// Batched upsert into `cataloginventory_stock_item`, keyed on
/// `(product_id, stock_id)` -- matches Go's `flushStock`.
pub async fn flush_stock(pool: &MySqlPool, rows: &[StockItem], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new(
            "INSERT INTO cataloginventory_stock_item \
             (product_id, stock_id, qty, is_in_stock, manage_stock, min_qty, min_sale_qty, max_sale_qty) ",
        );
        qb.push_values(chunk, |mut b, row: &StockItem| {
            b.push_bind(row.product_id)
                .push_bind(row.stock_id)
                .push_bind(row.qty)
                .push_bind(row.is_in_stock)
                .push_bind(row.manage_stock)
                .push_bind(row.min_qty)
                .push_bind(row.min_sale_qty)
                .push_bind(row.max_sale_qty);
        });
        qb.push(
            " ON DUPLICATE KEY UPDATE qty = VALUES(qty), is_in_stock = VALUES(is_in_stock), \
              manage_stock = VALUES(manage_stock), min_qty = VALUES(min_qty), \
              min_sale_qty = VALUES(min_sale_qty), max_sale_qty = VALUES(max_sale_qty)",
        );
        qb.build().execute(pool).await?;
    }
    Ok(())
}

/// Batched upsert into `catalog_product_index_price`, keyed on
/// `(entity_id, customer_group_id, website_id)` -- matches Go's `flushPrice`.
pub async fn flush_price(pool: &MySqlPool, rows: &[ProductIndexPrice], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new(
            "INSERT INTO catalog_product_index_price \
             (entity_id, customer_group_id, website_id, tax_class_id, price, final_price, min_price, max_price, tier_price) ",
        );
        qb.push_values(chunk, |mut b, row: &ProductIndexPrice| {
            b.push_bind(row.entity_id)
                .push_bind(row.customer_group_id)
                .push_bind(row.website_id)
                .push_bind(row.tax_class_id)
                .push_bind(row.price)
                .push_bind(row.final_price)
                .push_bind(row.min_price)
                .push_bind(row.max_price)
                .push_bind(row.tier_price);
        });
        qb.push(
            " ON DUPLICATE KEY UPDATE price = VALUES(price), final_price = VALUES(final_price), \
              min_price = VALUES(min_price), max_price = VALUES(max_price), tier_price = VALUES(tier_price)",
        );
        qb.build().execute(pool).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use entity::DEFAULT_STOCK_ID;

    /// Creates a private scratch product row (deleting any leftover from a
    /// previous crashed run of the same test first) and returns its
    /// `entity_id`.
    ///
    /// Each test that flushes into a shared table (varchar/stock/price all
    /// key off `entity_id`/`product_id`) gets its **own** product row rather
    /// than sharing one seeded entity: `cargo test` runs tests in parallel
    /// threads by default, and a shared entity_id + a blanket
    /// delete-everything-for-this-entity cleanup at the start of each test
    /// is a race -- one test's cleanup can wipe another test's in-flight
    /// data mid-run. Distinct SKUs per test sidesteps that entirely instead
    /// of serializing tests with a lock.
    async fn scratch_product(pool: &MySqlPool, sku: &str) -> u64 {
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = ?")
            .bind(sku)
            .execute(pool)
            .await
            .expect("pre-test cleanup delete must succeed");

        sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', ?)")
            .bind(sku)
            .execute(pool)
            .await
            .expect("scratch product insert must succeed")
            .last_insert_id()
    }

    async fn delete_scratch_product(pool: &MySqlPool, sku: &str) {
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = ?")
            .bind(sku)
            .execute(pool)
            .await
            .expect("scratch product cleanup delete must succeed");
    }

    #[tokio::test]
    async fn flush_varchar_upserts_a_row() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let entity_id = scratch_product(&pool, "RUST-FLUSH-TEST-VARCHAR").await;

        let rows = vec![EavValue { entity_id, attribute_id: 65000, store_id: 0, value: "rust test value".to_string() }];
        flush_varchar(&pool, &rows, 500).await.unwrap();

        let value: String = sqlx::query_scalar(
            "SELECT value FROM catalog_product_entity_varchar WHERE entity_id = ? AND attribute_id = ?",
        )
        .bind(entity_id)
        .bind(65000u16)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(value, "rust test value");

        // Upsert again with a new value -- should update, not duplicate.
        let rows = vec![EavValue { entity_id, attribute_id: 65000, store_id: 0, value: "updated value".to_string() }];
        flush_varchar(&pool, &rows, 500).await.unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM catalog_product_entity_varchar WHERE entity_id = ? AND attribute_id = ?",
        )
        .bind(entity_id)
        .bind(65000u16)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "upsert must update in place, not insert a duplicate row");

        delete_scratch_product(&pool, "RUST-FLUSH-TEST-VARCHAR").await;
    }

    #[tokio::test]
    async fn flush_stock_upserts_a_row() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let entity_id = scratch_product(&pool, "RUST-FLUSH-TEST-STOCK").await;

        let rows = vec![StockItem {
            item_id: 0,
            product_id: entity_id,
            stock_id: DEFAULT_STOCK_ID,
            qty: Some(42.0),
            min_qty: 0.0,
            is_qty_decimal: 0,
            backorders: 0,
            min_sale_qty: 1.0,
            max_sale_qty: 0.0,
            is_in_stock: 1,
            manage_stock: 1,
            website_id: 0,
        }];
        flush_stock(&pool, &rows, 500).await.unwrap();

        let qty: f64 = sqlx::query_scalar(
            "SELECT qty FROM cataloginventory_stock_item WHERE product_id = ? AND stock_id = ?",
        )
        .bind(entity_id)
        .bind(DEFAULT_STOCK_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(qty, 42.0);

        delete_scratch_product(&pool, "RUST-FLUSH-TEST-STOCK").await;
    }

    #[tokio::test]
    async fn flush_price_upserts_a_row() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let entity_id = scratch_product(&pool, "RUST-FLUSH-TEST-PRICE").await;

        let rows = vec![ProductIndexPrice {
            entity_id,
            customer_group_id: 0,
            website_id: 1,
            tax_class_id: Some(0),
            price: Some(12.34),
            final_price: Some(10.0),
            min_price: Some(10.0),
            max_price: Some(12.34),
            tier_price: Some(0.0),
        }];
        flush_price(&pool, &rows, 500).await.unwrap();

        let final_price: f64 = sqlx::query_scalar(
            "SELECT final_price FROM catalog_product_index_price WHERE entity_id = ? AND customer_group_id = 0 AND website_id = 1",
        )
        .bind(entity_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(final_price, 10.0);

        delete_scratch_product(&pool, "RUST-FLUSH-TEST-PRICE").await;
    }

    #[tokio::test]
    async fn empty_rows_are_a_no_op_for_every_flush_fn() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        flush_varchar(&pool, &[], 500).await.unwrap();
        flush_int(&pool, &[], 500).await.unwrap();
        flush_decimal(&pool, &[], 500).await.unwrap();
        flush_text(&pool, &[], 500).await.unwrap();
        flush_datetime(&pool, &[], 500).await.unwrap();
        flush_stock(&pool, &[], 500).await.unwrap();
        flush_price(&pool, &[], 500).await.unwrap();
    }
}
