use crate::csv_parse::ParsedCsv;
use entity::{ProductLink, LINK_TYPE_CROSSSELL, LINK_TYPE_GROUPED, LINK_TYPE_RELATED, LINK_TYPE_UPSELL};
use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

/// Maps each CSV column to the link type it produces. Each column is a
/// comma-separated list of SKUs. "grouped_skus" backs grouped products
/// (Magento's `LINK_TYPE_GROUPED`) -- the "grouped" product type itself
/// needs no special handling here since `type_id` already comes straight
/// from the CSV's own `type_id` column.
pub const PRODUCT_LINK_COLUMNS: [(&str, u16); 4] = [
    ("related_skus", LINK_TYPE_RELATED),
    ("upsell_skus", LINK_TYPE_UPSELL),
    ("crosssell_skus", LINK_TYPE_CROSSSELL),
    ("grouped_skus", LINK_TYPE_GROUPED),
];

/// Returns the SKUs referenced by a related/upsell/crosssell/grouped
/// column across every row, so the caller can fold them into the same
/// batch SKU lookup used for the primary "sku" column -- a linked SKU must
/// already resolve to an existing product_id; this project's import
/// doesn't create products from a link column, only from "sku" itself.
pub fn link_sku_columns(csv: &ParsedCsv) -> Vec<String> {
    let mut skus = Vec::new();
    for (col_name, _) in PRODUCT_LINK_COLUMNS {
        let Some(col) = csv.col_index(col_name) else { continue };
        for row in &csv.rows {
            let Some(val) = csv.field(row, col) else { continue };
            for sku in val.split(',') {
                let sku = sku.trim();
                if !sku.is_empty() {
                    skus.push(sku.to_string());
                }
            }
        }
    }
    skus
}

/// Collects the related_skus/upsell_skus/crosssell_skus/grouped_skus
/// columns into `ProductLink` rows. `sku_to_id` must already contain any
/// SKU referenced by these columns (see [`link_sku_columns`]).
pub fn collect_product_links(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> (Vec<ProductLink>, Vec<String>) {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    let active: Vec<(usize, u16)> =
        PRODUCT_LINK_COLUMNS.iter().filter_map(|(name, link_type)| csv.col_index(name).map(|ci| (ci, *link_type))).collect();
    if active.is_empty() {
        return (rows, warnings);
    }

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&product_id) = sku_to_id.get(sku) else { continue };

        for &(col, link_type) in &active {
            let Some(val) = csv.field(row, col) else { continue };
            for (pos, linked_sku) in val.split(',').enumerate() {
                let linked_sku = linked_sku.trim();
                if linked_sku.is_empty() {
                    continue;
                }
                if linked_sku == sku {
                    warnings.push(format!("sku={sku}: link column references itself, skipping"));
                    continue;
                }
                let Some(&linked_id) = sku_to_id.get(linked_sku) else {
                    warnings.push(format!("sku={sku}: link column references unknown SKU {linked_sku:?}, skipping"));
                    continue;
                };
                rows.push(ProductLink {
                    link_id: 0,
                    product_id: product_id as u32,
                    linked_product_id: linked_id as u32,
                    link_type_id: link_type,
                    position: pos as u32,
                });
            }
        }
    }

    (rows, warnings)
}

/// Upserts buffered product link rows, keyed on Magento's own
/// (product_id, linked_product_id, link_type_id) uniqueness rule.
pub async fn flush_product_links(pool: &MySqlPool, rows: &[ProductLink], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new("INSERT INTO catalog_product_link (product_id, linked_product_id, link_type_id, position) ");
        qb.push_values(chunk, |mut b, row: &ProductLink| {
            b.push_bind(row.product_id).push_bind(row.linked_product_id).push_bind(row.link_type_id).push_bind(row.position);
        });
        qb.push(" ON DUPLICATE KEY UPDATE position = VALUES(position)");
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
    fn no_columns_is_a_no_op() {
        let csv = parse("sku,name\nSKU-A,Widget\n");
        let (rows, warnings) = collect_product_links(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn all_four_link_types_from_one_row() {
        let sku_to_id = HashMap::from([("SKU-A".to_string(), 1u64), ("SKU-B".to_string(), 2u64), ("SKU-C".to_string(), 3u64)]);
        let csv = parse(
            "sku,related_skus,upsell_skus,crosssell_skus,grouped_skus\n\
             SKU-A,\"SKU-B,SKU-C\",SKU-B,SKU-C,SKU-B\n",
        );
        let (rows, warnings) = collect_product_links(&csv, &sku_to_id);
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 5, "related=2 + upsell=1 + crosssell=1 + grouped=1");
        assert!(rows.iter().all(|r| r.product_id == 1));
    }

    #[test]
    fn unknown_sku_warns_and_is_skipped() {
        let csv = parse("sku,related_skus\nSKU-A,DOES-NOT-EXIST\n");
        let (rows, warnings) = collect_product_links(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(rows.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown SKU"));
    }

    #[test]
    fn self_reference_warns_and_is_skipped() {
        let csv = parse("sku,related_skus\nSKU-A,SKU-A\n");
        let (rows, warnings) = collect_product_links(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(rows.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("references itself"));
    }

    #[test]
    fn link_sku_columns_collects_every_referenced_sku() {
        let csv = parse("sku,related_skus,upsell_skus\nSKU-A,\"SKU-B,SKU-C\",SKU-D\n");
        let mut skus = link_sku_columns(&csv);
        skus.sort();
        assert_eq!(skus, vec!["SKU-B", "SKU-C", "SKU-D"]);
    }

    #[tokio::test]
    async fn flush_upserts_on_reimport() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-LINK-A', 'RUST-LINK-B')").execute(&pool).await.unwrap();
        let a = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-LINK-A')")
            .execute(&pool).await.unwrap().last_insert_id();
        let b = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-LINK-B')")
            .execute(&pool).await.unwrap().last_insert_id();

        let rows = vec![ProductLink { link_id: 0, product_id: a as u32, linked_product_id: b as u32, link_type_id: LINK_TYPE_RELATED, position: 0 }];
        flush_product_links(&pool, &rows, 500).await.unwrap();
        flush_product_links(&pool, &rows, 500).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_link WHERE product_id = ?")
            .bind(a as u32)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "upsert must not duplicate on reimport");

        sqlx::query("DELETE FROM catalog_product_link WHERE product_id = ?").bind(a as u32).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-LINK-A', 'RUST-LINK-B')").execute(&pool).await.unwrap();
    }
}
