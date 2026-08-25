use serde::Serialize;
use sqlx::MySqlPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StockInfo {
    pub qty: Option<f64>,
    pub is_in_stock: u16,
}

/// Looks up stock (qty/in-stock status) by SKU from
/// `cataloginventory_stock_item` -- this port's realtime inventory lookup
/// uses the single-source stock table rather than Magento's MSI
/// `inventory_source_item`, since MSI isn't part of the simplified schema
/// this project seeds (see the top-level README's known limitations). The observable
/// contract (qty + in-stock boolean by SKU) is preserved either way.
pub async fn stock_by_sku(pool: &MySqlPool, sku: &str) -> Result<Option<StockInfo>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.qty, s.is_in_stock FROM cataloginventory_stock_item s \
         JOIN catalog_product_entity p ON p.entity_id = s.product_id \
         WHERE p.sku = ? AND s.stock_id = 1",
    )
    .bind(sku)
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_none_for_unknown_sku() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let result = stock_by_sku(&pool, "DEFINITELY-NOT-A-REAL-SKU").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn returns_stock_info_for_seeded_sku() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let result = stock_by_sku(&pool, "SAMPLE-SKU-0000").await.unwrap();
        assert!(result.is_some(), "seed data must include stock for SAMPLE-SKU-0000");
    }
}
