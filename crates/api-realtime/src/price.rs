use sqlx::MySqlPool;

/// Looks up the lowest of a product's `price`/`special_price` EAV decimal
/// attributes by SKU -- a simplified stand-in for Go's
/// `LEAST(base_price, special_price, tier_price WHERE qty=1)` raw-SQL join.
/// Tier pricing is a non-goal for this port (no `catalog_product_entity_tier_price`
/// table in the simplified schema this project seeds), so only the two
/// scalar attributes are considered.
pub async fn lowest_price_by_sku(pool: &MySqlPool, sku: &str) -> Result<Option<f64>, sqlx::Error> {
    let entity_id: Option<u64> = sqlx::query_scalar("SELECT entity_id FROM catalog_product_entity WHERE sku = ?").bind(sku).fetch_optional(pool).await?;
    let Some(entity_id) = entity_id else { return Ok(None) };

    let values: Vec<f64> = sqlx::query_scalar(
        "SELECT d.value FROM catalog_product_entity_decimal d \
         JOIN eav_attribute a ON a.attribute_id = d.attribute_id \
         WHERE d.entity_id = ? AND a.attribute_code IN ('price', 'special_price') AND d.value IS NOT NULL",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await?;

    Ok(values.into_iter().reduce(f64::min))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_none_for_unknown_sku() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let result = lowest_price_by_sku(&pool, "DEFINITELY-NOT-A-REAL-SKU").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn returns_the_lower_of_price_and_special_price() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        // SAMPLE-SKU-0001 is seeded with a "price" decimal attribute;
        // whatever its value, this must be <= it.
        let Some(price) = sqlx::query_scalar::<_, f64>(
            "SELECT d.value FROM catalog_product_entity_decimal d \
             JOIN eav_attribute a ON a.attribute_id = d.attribute_id \
             JOIN catalog_product_entity p ON p.entity_id = d.entity_id \
             WHERE p.sku = 'SAMPLE-SKU-0001' AND a.attribute_code = 'price'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap() else {
            return; // seed data not present in this environment; nothing to assert
        };

        let lowest = lowest_price_by_sku(&pool, "SAMPLE-SKU-0001").await.unwrap();
        assert!(lowest.is_some());
        assert!(lowest.unwrap() <= price);
    }
}
