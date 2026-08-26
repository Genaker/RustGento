use crate::csv_parse::ParsedCsv;
use entity::{Category, CATEGORY_ENTITY_TYPE_ID};
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};
use std::collections::HashMap;

/// One raw (unresolved) product -> category-path pair collected from a CSV
/// row, before the path is turned into a category_id.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryAssignment {
    pub product_id: u64,
    pub path: Vec<String>,
}

/// Collects the "categories" column: a comma-separated list of
/// "/"-delimited category paths (e.g.
/// "Default Category/Shoes,Default Category/Sale"), matching Magento/
/// Magmi's own CSV convention for on-the-fly category assignment.
pub fn collect_categories(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> (Vec<CategoryAssignment>, Vec<String>) {
    let mut assignments = Vec::new();
    let mut warnings = Vec::new();

    let Some(col) = csv.col_index("categories") else { return (assignments, warnings) };

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&product_id) = sku_to_id.get(sku) else { continue };
        let Some(val) = csv.field(row, col) else { continue };

        for raw_path in val.split(',') {
            let raw_path = raw_path.trim();
            if raw_path.is_empty() {
                continue;
            }
            let segments: Vec<String> = raw_path.split('/').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            if segments.is_empty() {
                warnings.push(format!("sku={sku}: empty category path {raw_path:?}, skipping"));
                continue;
            }
            assignments.push(CategoryAssignment { product_id, path: segments });
        }
    }

    (assignments, warnings)
}

/// Resolves each unique category path to a category_id -- creating any
/// missing category in the path along the way (Magmi's "on the fly category
/// creator/importer") -- then upserts the resulting product/category
/// assignments into `catalog_category_product`.
pub async fn flush_categories(pool: &MySqlPool, assignments: &[CategoryAssignment], batch_size: usize) -> Result<(), sqlx::Error> {
    if assignments.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    let name_attr_id = find_or_create_category_name_attribute(&mut tx).await?;
    let root = find_or_create_root_category(&mut tx).await?;

    // Memoize path -> category_id for this run: many rows commonly share
    // the same category path, and each resolution costs a few queries, so
    // re-walking it per row would multiply that cost by the number of
    // assigned products instead of the number of distinct paths.
    let mut path_cache: HashMap<String, u64> = HashMap::new();
    let mut links = Vec::with_capacity(assignments.len());

    for a in assignments {
        let key = a.path.join("\u{0}");
        let category_id = match path_cache.get(&key) {
            Some(&id) => id,
            None => {
                let id = resolve_or_create_category_path(&mut tx, &root, &a.path, name_attr_id).await?;
                path_cache.insert(key, id);
                id
            }
        };
        links.push((category_id, a.product_id));
    }

    for chunk in links.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO catalog_category_product (category_id, product_id) ");
        qb.push_values(chunk, |mut b, (category_id, product_id): &(u64, u64)| {
            b.push_bind(*category_id).push_bind(*product_id);
        });
        qb.push(" ON DUPLICATE KEY UPDATE category_id = category_id");
        qb.build().execute(&mut *tx).await?;
    }

    tx.commit().await
}

async fn find_or_create_category_name_attribute(tx: &mut sqlx::Transaction<'_, MySql>) -> Result<u16, sqlx::Error> {
    if let Some(id) = sqlx::query_scalar::<_, u16>(
        "SELECT attribute_id FROM eav_attribute WHERE entity_type_id = ? AND attribute_code = 'name'",
    )
    .bind(CATEGORY_ENTITY_TYPE_ID)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }

    let result = sqlx::query(
        "INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type) VALUES (?, 'name', 'varchar')",
    )
    .bind(CATEGORY_ENTITY_TYPE_ID)
    .execute(&mut **tx)
    .await?;
    Ok(result.last_insert_id() as u16)
}

async fn find_or_create_root_category(tx: &mut sqlx::Transaction<'_, MySql>) -> Result<Category, sqlx::Error> {
    if let Some(root) = sqlx::query_as::<_, Category>(
        "SELECT * FROM catalog_category_entity WHERE parent_id = 0 ORDER BY entity_id LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(root);
    }

    let result = sqlx::query(
        "INSERT INTO catalog_category_entity (attribute_set_id, parent_id, path, position, level, children_count) \
         VALUES (3, 0, '1', 0, 0, 0)",
    )
    .execute(&mut **tx)
    .await?;
    let entity_id = result.last_insert_id();
    sqlx::query_as::<_, Category>("SELECT * FROM catalog_category_entity WHERE entity_id = ?")
        .bind(entity_id)
        .fetch_one(&mut **tx)
        .await
}

/// Walks `path` under `root`, creating any missing category segment, and
/// returns the leaf category's entity_id. An existing category is matched
/// by (parent_id, name) via the category name attribute -- there's no
/// uniqueness constraint enforcing distinct sibling names, so the first
/// match wins, same as Magento's own CSV importer.
async fn resolve_or_create_category_path(
    tx: &mut sqlx::Transaction<'_, MySql>,
    root: &Category,
    path: &[String],
    name_attr_id: u16,
) -> Result<u64, sqlx::Error> {
    let mut parent = root.clone();

    for name in path {
        let existing_id: Option<u64> = sqlx::query(
            "SELECT c.entity_id AS entity_id FROM catalog_category_entity c \
             JOIN catalog_category_entity_varchar v ON v.entity_id = c.entity_id \
             WHERE c.parent_id = ? AND v.attribute_id = ? AND v.store_id = 0 AND v.value = ? \
             LIMIT 1",
        )
        .bind(parent.entity_id)
        .bind(name_attr_id)
        .bind(name)
        .fetch_optional(&mut **tx)
        .await?
        .map(|row| row.get::<u64, _>("entity_id"));

        if let Some(id) = existing_id {
            parent = sqlx::query_as::<_, Category>("SELECT * FROM catalog_category_entity WHERE entity_id = ?")
                .bind(id)
                .fetch_one(&mut **tx)
                .await?;
            continue;
        }

        let result = sqlx::query(
            "INSERT INTO catalog_category_entity (attribute_set_id, parent_id, path, position, level, children_count) \
             VALUES (3, ?, '', 0, ?, 0)",
        )
        .bind(parent.entity_id)
        .bind(parent.level + 1)
        .execute(&mut **tx)
        .await?;
        let new_id = result.last_insert_id();
        let new_path = format!("{}/{}", parent.path, new_id);
        sqlx::query("UPDATE catalog_category_entity SET path = ? WHERE entity_id = ?")
            .bind(&new_path)
            .bind(new_id)
            .execute(&mut **tx)
            .await?;
        sqlx::query("UPDATE catalog_category_entity SET children_count = children_count + 1 WHERE entity_id = ?")
            .bind(parent.entity_id)
            .execute(&mut **tx)
            .await?;
        sqlx::query("INSERT INTO catalog_category_entity_varchar (attribute_id, store_id, entity_id, value) VALUES (?, 0, ?, ?)")
            .bind(name_attr_id)
            .bind(new_id)
            .bind(name)
            .execute(&mut **tx)
            .await?;

        parent = sqlx::query_as::<_, Category>("SELECT * FROM catalog_category_entity WHERE entity_id = ?")
            .bind(new_id)
            .fetch_one(&mut **tx)
            .await?;
    }

    Ok(parent.entity_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn no_categories_column_is_a_no_op() {
        let csv = parse("sku,name\nSKU-1,Widget\n");
        let (assignments, warnings) = collect_categories(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(assignments.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn single_path_is_split_into_segments() {
        let csv = parse("sku,categories\nSKU-1,Default Category/Shoes/Running\n");
        let (assignments, warnings) = collect_categories(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].path, vec!["Default Category", "Shoes", "Running"]);
        assert_eq!(assignments[0].product_id, 1);
    }

    #[test]
    fn multiple_paths_are_split_by_comma() {
        let csv = parse("sku,categories\nSKU-1,\"Default Category/Shoes,Default Category/Sale\"\n");
        let (assignments, _) = collect_categories(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].path, vec!["Default Category", "Shoes"]);
        assert_eq!(assignments[1].path, vec!["Default Category", "Sale"]);
    }

    #[test]
    fn blank_cell_is_skipped() {
        let csv = parse("sku,categories\nSKU-1,\n");
        let (assignments, warnings) = collect_categories(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(assignments.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,categories\nSKU-1,Default Category/Shoes\n");
        let (assignments, warnings) = collect_categories(&csv, &HashMap::new());
        assert!(assignments.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn empty_path_segments_warn_and_are_skipped() {
        let csv = parse("sku,categories\nSKU-1,\"//\"\n");
        let (assignments, warnings) = collect_categories(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(assignments.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("empty category path"));
    }

    #[tokio::test]
    async fn flush_categories_creates_hierarchy_and_assigns_product() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-CAT-TEST-%'").execute(&pool).await.unwrap();
        let result = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-CAT-TEST-1')")
            .execute(&pool)
            .await
            .unwrap();
        let product_id = result.last_insert_id();

        let unique_root = format!("Rust Test Root {product_id}");
        let unique_child = format!("Rust Test Child {product_id}");
        let assignments = vec![CategoryAssignment {
            product_id,
            path: vec![unique_root.clone(), unique_child.clone()],
        }];

        flush_categories(&pool, &assignments, 500).await.unwrap();

        let leaf_id: u64 = sqlx::query_scalar(
            "SELECT c.entity_id FROM catalog_category_entity c \
             JOIN catalog_category_entity_varchar v ON v.entity_id = c.entity_id \
             WHERE v.value = ?",
        )
        .bind(&unique_child)
        .fetch_one(&pool)
        .await
        .unwrap();
        let root_id: u64 = sqlx::query_scalar(
            "SELECT c.entity_id FROM catalog_category_entity c \
             JOIN catalog_category_entity_varchar v ON v.entity_id = c.entity_id \
             WHERE v.value = ?",
        )
        .bind(&unique_root)
        .fetch_one(&pool)
        .await
        .unwrap();

        let linked_product: u64 =
            sqlx::query_scalar("SELECT product_id FROM catalog_category_product WHERE category_id = ?")
                .bind(leaf_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked_product, product_id);

        // Reimport must not duplicate the category or the link.
        flush_categories(&pool, &assignments, 500).await.unwrap();
        let link_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_category_product WHERE category_id = ?")
            .bind(leaf_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(link_count, 1, "upsert must not duplicate the link on reimport");
        let category_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM catalog_category_entity_varchar WHERE value IN (?, ?)",
        )
        .bind(&unique_root)
        .bind(&unique_child)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(category_count, 2, "reimport must not recreate the path");

        sqlx::query("DELETE FROM catalog_category_product WHERE category_id = ?").bind(leaf_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_category_entity_varchar WHERE entity_id IN (?, ?)")
            .bind(leaf_id)
            .bind(root_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM catalog_category_entity WHERE entity_id IN (?, ?)")
            .bind(leaf_id)
            .bind(root_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-CAT-TEST-%'").execute(&pool).await.unwrap();
    }
}
