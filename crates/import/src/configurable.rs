use crate::attributes::AttributesByCode;
use crate::csv_parse::ParsedCsv;
use entity::{ProductSuperAttribute, ProductSuperLink};
use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

pub const CONFIGURABLE_ATTRIBUTES_COLUMN: &str = "configurable_attributes";
pub const CONFIGURABLE_VARIATIONS_COLUMN: &str = "configurable_variations";

/// Returns every distinct child SKU referenced by "configurable_variations"
/// across every row, so the caller can fold them into the same batch SKU
/// lookup used for product links and bundle selections -- a configurable's
/// child must already exist as its own row in this (or an earlier) import,
/// it isn't created from this column.
pub fn configurable_child_skus(csv: &ParsedCsv) -> Vec<String> {
    let Some(col) = csv.col_index(CONFIGURABLE_VARIATIONS_COLUMN) else { return Vec::new() };
    let mut skus = Vec::new();
    for row in &csv.rows {
        let Some(val) = csv.field(row, col) else { continue };
        for sku in val.split(',') {
            let sku = sku.trim();
            if !sku.is_empty() {
                skus.push(sku.to_string());
            }
        }
    }
    skus
}

/// Collects "configurable_attributes" (a comma-separated list of attribute
/// codes that vary across this configurable's children, e.g. "color,size")
/// and "configurable_variations" (a comma-separated list of child SKUs) on
/// any row that has at least one of the two columns populated -- that
/// row's own SKU becomes the configurable parent.
///
/// Unlike Magento's own CSV import, variations here are just child SKUs,
/// not "sku=X,color=Y,size=Z" pairs: each child is expected to already be
/// its own CSV row (or pre-existing product) carrying its own EAV
/// attribute values normally, so there's nothing to duplicate here.
pub fn collect_configurable(
    csv: &ParsedCsv,
    sku_to_id: &HashMap<String, u64>,
    attrs_by_code: &AttributesByCode,
) -> (Vec<ProductSuperAttribute>, Vec<ProductSuperLink>, Vec<String>) {
    let mut attributes = Vec::new();
    let mut links = Vec::new();
    let mut warnings = Vec::new();

    let attr_col = csv.col_index(CONFIGURABLE_ATTRIBUTES_COLUMN);
    let var_col = csv.col_index(CONFIGURABLE_VARIATIONS_COLUMN);
    if attr_col.is_none() && var_col.is_none() {
        return (attributes, links, warnings);
    }

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&parent_id) = sku_to_id.get(sku) else { continue };

        let attrs_val = attr_col.and_then(|c| csv.field(row, c));
        let vars_val = var_col.and_then(|c| csv.field(row, c));
        if attrs_val.is_none() && vars_val.is_none() {
            continue;
        }

        if let Some(val) = attrs_val {
            for (pos, code) in val.split(',').enumerate() {
                let code = code.trim();
                if code.is_empty() {
                    continue;
                }
                let Some(meta) = attrs_by_code.get(code) else {
                    warnings.push(format!("sku={sku}: configurable_attributes references unknown attribute {code:?}, skipping"));
                    continue;
                };
                attributes.push(ProductSuperAttribute { product_super_attribute_id: 0, product_id: parent_id as u32, attribute_id: meta.id, position: pos as i64 });
            }
        }

        if let Some(val) = vars_val {
            for child_sku in val.split(',') {
                let child_sku = child_sku.trim();
                if child_sku.is_empty() {
                    continue;
                }
                if child_sku == sku {
                    warnings.push(format!("sku={sku}: configurable_variations references itself, skipping"));
                    continue;
                }
                let Some(&child_id) = sku_to_id.get(child_sku) else {
                    warnings.push(format!("sku={sku}: configurable_variations references unknown SKU {child_sku:?}, skipping"));
                    continue;
                };
                links.push(ProductSuperLink { link_id: 0, product_id: child_id as u32, parent_id: parent_id as u32 });
            }
        }
    }

    (attributes, links, warnings)
}

/// Upserts buffered super-attribute/super-link rows. Links upsert on
/// product_id alone (a child belongs to at most one configurable parent,
/// matching real Magento); attributes upsert on (product_id, attribute_id).
pub async fn flush_configurable(pool: &MySqlPool, attributes: &[ProductSuperAttribute], links: &[ProductSuperLink], batch_size: usize) -> Result<(), sqlx::Error> {
    if attributes.is_empty() && links.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;

    for chunk in attributes.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO catalog_product_super_attribute (product_id, attribute_id, position) ");
        qb.push_values(chunk, |mut b, a: &ProductSuperAttribute| {
            b.push_bind(a.product_id).push_bind(a.attribute_id).push_bind(a.position);
        });
        qb.push(" ON DUPLICATE KEY UPDATE position = VALUES(position)");
        qb.build().execute(&mut *tx).await?;
    }
    for chunk in links.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO catalog_product_super_link (product_id, parent_id) ");
        qb.push_values(chunk, |mut b, l: &ProductSuperLink| {
            b.push_bind(l.product_id).push_bind(l.parent_id);
        });
        qb.push(" ON DUPLICATE KEY UPDATE parent_id = VALUES(parent_id)");
        qb.build().execute(&mut *tx).await?;
    }

    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use entity::EavAttribute;

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    fn attr(id: u16, code: &str) -> EavAttribute {
        EavAttribute {
            attribute_id: id, entity_type_id: 4, attribute_code: code.to_string(), attribute_model: None,
            backend_model: None, backend_type: "int".to_string(), backend_table: None, frontend_model: None,
            frontend_input: None, frontend_label: None, frontend_class: None, source_model: None,
            is_required: 0, is_user_defined: 1, default_value: None, is_unique: 0, note: None,
        }
    }

    #[test]
    fn no_columns_is_a_no_op() {
        let csv = parse("sku,name\nSKU-A,Widget\n");
        let (attributes, links, warnings) = collect_configurable(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]), &AttributesByCode::default());
        assert!(attributes.is_empty());
        assert!(links.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn attributes_and_variations() {
        let sku_to_id = HashMap::from([("CONFIG-1".to_string(), 1u64), ("CHILD-1".to_string(), 2u64), ("CHILD-2".to_string(), 3u64)]);
        let attrs_by_code = AttributesByCode::build(&[attr(76, "status"), attr(77, "special_from_date")]);
        let csv = parse(
            "sku,configurable_attributes,configurable_variations\n\
             CONFIG-1,\"status,special_from_date\",\"CHILD-1,CHILD-2\"\n",
        );
        let (attributes, links, warnings) = collect_configurable(&csv, &sku_to_id, &attrs_by_code);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert_eq!(attributes.len(), 2);
        assert_eq!(attributes[0].attribute_id, 76);
        assert_eq!(attributes[1].attribute_id, 77);
        assert!(attributes.iter().all(|a| a.product_id == 1));
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.parent_id == 1));
        assert_eq!(links[0].product_id, 2);
        assert_eq!(links[1].product_id, 3);
    }

    #[test]
    fn unknown_attribute_code_warns_and_skips() {
        let csv = parse("sku,configurable_attributes\nCONFIG-1,not_a_real_attribute\n");
        let (attributes, _, warnings) =
            collect_configurable(&csv, &HashMap::from([("CONFIG-1".to_string(), 1u64)]), &AttributesByCode::default());
        assert!(attributes.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown attribute"));
    }

    #[test]
    fn unknown_child_sku_warns_and_skips() {
        let csv = parse("sku,configurable_variations\nCONFIG-1,DOES-NOT-EXIST\n");
        let (_, links, warnings) =
            collect_configurable(&csv, &HashMap::from([("CONFIG-1".to_string(), 1u64)]), &AttributesByCode::default());
        assert!(links.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown SKU"));
    }

    #[test]
    fn self_reference_warns_and_skips() {
        let csv = parse("sku,configurable_variations\nCONFIG-1,CONFIG-1\n");
        let (_, links, warnings) =
            collect_configurable(&csv, &HashMap::from([("CONFIG-1".to_string(), 1u64)]), &AttributesByCode::default());
        assert!(links.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("references itself"));
    }

    #[test]
    fn configurable_child_skus_collects_every_referenced_sku() {
        let csv = parse("sku,configurable_variations\nCONFIG-1,\"CHILD-1,CHILD-2\"\n");
        let mut skus = configurable_child_skus(&csv);
        skus.sort();
        assert_eq!(skus, vec!["CHILD-1", "CHILD-2"]);
    }

    #[tokio::test]
    async fn flush_reassigns_child_to_new_parent_on_reimport() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-CONF-CHILD', 'RUST-CONF-A', 'RUST-CONF-B')").execute(&pool).await.unwrap();
        let child = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-CONF-CHILD')")
            .execute(&pool).await.unwrap().last_insert_id();
        let config_a = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'configurable', 'RUST-CONF-A')")
            .execute(&pool).await.unwrap().last_insert_id();
        let config_b = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'configurable', 'RUST-CONF-B')")
            .execute(&pool).await.unwrap().last_insert_id();

        let link_to_a = vec![ProductSuperLink { link_id: 0, product_id: child as u32, parent_id: config_a as u32 }];
        flush_configurable(&pool, &[], &link_to_a, 500).await.unwrap();

        let link_to_b = vec![ProductSuperLink { link_id: 0, product_id: child as u32, parent_id: config_b as u32 }];
        flush_configurable(&pool, &[], &link_to_b, 500).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_super_link WHERE product_id = ?").bind(child as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "upsert on product_id, not a second row");
        let parent_id: u32 = sqlx::query_scalar("SELECT parent_id FROM catalog_product_super_link WHERE product_id = ?").bind(child as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(parent_id, config_b as u32, "reassigned to the new parent");

        sqlx::query("DELETE FROM catalog_product_super_link WHERE product_id = ?").bind(child as u32).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-CONF-CHILD', 'RUST-CONF-A', 'RUST-CONF-B')").execute(&pool).await.unwrap();
    }
}
