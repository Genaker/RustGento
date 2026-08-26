use crate::csv_parse::ParsedCsv;
use crate::price_bucket::PRICE_WEBSITE_ID;
use entity::TierPrice;
use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

/// Collects the "tier_prices" column: a "|"-separated list of
/// "group:qty:price" entries, e.g. "all:5:8.99|1:10:7.50|2:1:9.99". "group"
/// is either the literal "all" (applies to every customer group) or a
/// numeric customer_group_id. A qty=1 entry for one specific group is what
/// Magento calls a "group price" -- there is no separate table for it,
/// tier pricing and group pricing are the same mechanism at different qty
/// breaks.
pub fn collect_tier_prices(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> (Vec<TierPrice>, Vec<String>) {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    let Some(col) = csv.col_index("tier_prices") else { return (rows, warnings) };

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&entity_id) = sku_to_id.get(sku) else { continue };
        let Some(val) = csv.field(row, col) else { continue };

        for entry in val.split('|') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let parts: Vec<&str> = entry.split(':').collect();
            if parts.len() != 3 {
                warnings.push(format!("sku={sku}: malformed tier_prices entry {entry:?}, want group:qty:price"));
                continue;
            }
            let (group_raw, qty_raw, price_raw) = (parts[0].trim(), parts[1].trim(), parts[2].trim());

            let (all_groups, customer_group_id) = if group_raw.eq_ignore_ascii_case("all") {
                (1u8, 0u16)
            } else {
                match group_raw.parse::<u16>() {
                    Ok(gid) => (0u8, gid),
                    Err(_) => {
                        warnings.push(format!("sku={sku}: invalid customer group {group_raw:?} in tier_prices entry {entry:?}"));
                        continue;
                    }
                }
            };
            let Ok(qty) = qty_raw.parse::<f64>() else {
                warnings.push(format!("sku={sku}: invalid qty {qty_raw:?} in tier_prices entry {entry:?}"));
                continue;
            };
            let Ok(price) = price_raw.parse::<f64>() else {
                warnings.push(format!("sku={sku}: invalid price {price_raw:?} in tier_prices entry {entry:?}"));
                continue;
            };

            rows.push(TierPrice {
                value_id: 0,
                entity_id: Some(entity_id),
                row_id: None,
                all_groups,
                customer_group_id,
                qty,
                value: price,
                website_id: PRICE_WEBSITE_ID,
                percentage_value: None,
            });
        }
    }

    (rows, warnings)
}

/// Upserts buffered tier/group price rows, keyed on the same (entity_id,
/// all_groups, customer_group_id, qty, website_id) tuple Magento itself
/// uses to distinguish price breaks.
pub async fn flush_tier_prices(pool: &MySqlPool, rows: &[TierPrice], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new(
            "INSERT INTO catalog_product_entity_tier_price \
             (entity_id, all_groups, customer_group_id, qty, value, website_id) ",
        );
        qb.push_values(chunk, |mut b, row: &TierPrice| {
            b.push_bind(row.entity_id)
                .push_bind(row.all_groups)
                .push_bind(row.customer_group_id)
                .push_bind(row.qty)
                .push_bind(row.value)
                .push_bind(row.website_id);
        });
        qb.push(" ON DUPLICATE KEY UPDATE value = VALUES(value)");
        qb.build().execute(&mut *tx).await?;
    }
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn no_column_is_a_no_op() {
        let csv = parse("sku,name\nSKU-1,Widget\n");
        let (rows, warnings) = collect_tier_prices(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn all_groups_and_per_group_entries() {
        let csv = parse("sku,tier_prices\nSKU-1,\"all:5:8.99|1:10:7.50|2:1:9.99\"\n");
        let (rows, warnings) = collect_tier_prices(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].all_groups, 1);
        assert_eq!(rows[0].customer_group_id, 0);
        assert_eq!(rows[0].qty, 5.0);
        assert_eq!(rows[0].value, 8.99);
        assert_eq!(rows[1].all_groups, 0);
        assert_eq!(rows[1].customer_group_id, 1);
        assert_eq!(rows[2].customer_group_id, 2);
        assert_eq!(rows[2].qty, 1.0);
    }

    #[test]
    fn malformed_entry_warns_and_is_skipped() {
        let csv = parse("sku,tier_prices\nSKU-1,\"all:5:8.99|not-enough-parts|all:abc:1.00|all:5:not-a-number\"\n");
        let (rows, warnings) = collect_tier_prices(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(rows.len(), 1, "only the valid entry survives");
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn blank_cell_is_skipped() {
        let csv = parse("sku,tier_prices\nSKU-1,\n");
        let (rows, warnings) = collect_tier_prices(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,tier_prices\nSKU-1,all:5:8.99\n");
        let (rows, warnings) = collect_tier_prices(&csv, &HashMap::new());
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn flush_upserts_on_reimport() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-TIER-TEST-1'").execute(&pool).await.unwrap();
        let result = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-TIER-TEST-1')")
            .execute(&pool)
            .await
            .unwrap();
        let entity_id = result.last_insert_id();

        let rows = vec![TierPrice {
            value_id: 0,
            entity_id: Some(entity_id),
            row_id: None,
            all_groups: 1,
            customer_group_id: 0,
            qty: 5.0,
            value: 8.99,
            website_id: PRICE_WEBSITE_ID,
            percentage_value: None,
        }];
        flush_tier_prices(&pool, &rows, 500).await.unwrap();

        let mut updated = rows.clone();
        updated[0].value = 6.99;
        flush_tier_prices(&pool, &updated, 500).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_entity_tier_price WHERE entity_id = ?")
            .bind(entity_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "upsert must update in place, not insert a duplicate");
        // CAST needed because sqlx's MySQL driver won't decode a DECIMAL
        // column straight into f64 -- binding f64 into an INSERT tolerates
        // it fine, only strict decode on the way back does not.
        let value: f64 = sqlx::query_scalar("SELECT CAST(value AS DOUBLE) FROM catalog_product_entity_tier_price WHERE entity_id = ?")
            .bind(entity_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(value, 6.99);

        sqlx::query("DELETE FROM catalog_product_entity_tier_price WHERE entity_id = ?").bind(entity_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-TIER-TEST-1'").execute(&pool).await.unwrap();
    }
}
