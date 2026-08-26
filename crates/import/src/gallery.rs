use crate::csv_parse::ParsedCsv;
use entity::{ProductMediaGallery, ProductMediaGalleryValueToEntity};
use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::{HashMap, HashSet};

pub const GALLERY_COLUMNS: [&str; 4] = ["image", "small_image", "thumbnail", "media_gallery"];

/// One gallery pool row paired with the product it belongs to. The pool
/// row (`catalog_product_entity_media_gallery`) has no entity_id column of
/// its own -- the link is only known once `flush_gallery` inserts it and
/// gets back a value_id to pair with `entity_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct GalleryRow {
    pub entity_id: u64,
    pub pool_row: ProductMediaGallery,
}

/// Collects the image/small_image/thumbnail/media_gallery columns:
/// "|"-separated image paths, deduplicated per (sku, image) pair so the
/// same image referenced by two different columns on the same row only
/// produces one pool entry.
///
/// Known simplification: there's no cross-product pool dedup (the same
/// image URL used by two different products gets two separate pool rows)
/// and no file download/attach, only the DB rows.
pub fn collect_gallery(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> Vec<GalleryRow> {
    let mut rows = Vec::new();

    let active: Vec<usize> = GALLERY_COLUMNS.iter().filter_map(|c| csv.col_index(c)).collect();
    if active.is_empty() {
        return rows;
    }

    let mut seen: HashSet<(String, String)> = HashSet::new();

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&entity_id) = sku_to_id.get(sku) else { continue };

        for &col in &active {
            let Some(val) = csv.field(row, col) else { continue };
            for img in val.split('|') {
                let img = img.trim();
                if img.is_empty() {
                    continue;
                }
                let key = (sku.to_string(), img.to_string());
                if !seen.insert(key) {
                    continue;
                }
                rows.push(GalleryRow {
                    entity_id,
                    pool_row: ProductMediaGallery { value_id: 0, attribute_id: 87, value: Some(img.to_string()), media_type: "image".into(), disabled: 0 },
                });
            }
        }
    }

    rows
}

/// Writes buffered gallery rows to the pool table, then links each one to
/// its owning product via
/// `catalog_product_entity_media_gallery_value_to_entity` -- computing
/// each chunk's contiguous `value_id`s from `LAST_INSERT_ID()`, the same
/// trick `insert_new_products` uses for `catalog_product_entity`.
pub async fn flush_gallery(pool: &MySqlPool, rows: &[GalleryRow], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    let mut links = Vec::with_capacity(rows.len());

    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new("INSERT INTO catalog_product_entity_media_gallery (attribute_id, value, media_type, disabled) ");
        qb.push_values(chunk, |mut b, row: &GalleryRow| {
            b.push_bind(row.pool_row.attribute_id).push_bind(row.pool_row.value.clone()).push_bind(&row.pool_row.media_type).push_bind(row.pool_row.disabled);
        });
        let result = qb.build().execute(&mut *tx).await?;
        let first_id = result.last_insert_id();
        for (i, row) in chunk.iter().enumerate() {
            links.push(ProductMediaGalleryValueToEntity { entity_id: row.entity_id, value_id: first_id + i as u64 });
        }
    }

    for chunk in links.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO catalog_product_entity_media_gallery_value_to_entity (entity_id, value_id) ");
        qb.push_values(chunk, |mut b, l: &ProductMediaGalleryValueToEntity| {
            b.push_bind(l.entity_id).push_bind(l.value_id);
        });
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
        let csv = parse("sku,name\nSKU-1,Widget\n");
        let rows = collect_gallery(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty());
    }

    #[test]
    fn pipe_separated_images_produce_multiple_rows() {
        let csv = parse("sku,image\nGAL-2,/m/y/image2.jpg|/m/y/image3.jpg\n");
        let rows = collect_gallery(&csv, &HashMap::from([("GAL-2".to_string(), 2u64)]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].pool_row.value.as_deref(), Some("/m/y/image2.jpg"));
        assert_eq!(rows[1].pool_row.value.as_deref(), Some("/m/y/image3.jpg"));
        assert!(rows.iter().all(|r| r.entity_id == 2));
    }

    #[test]
    fn same_image_across_columns_is_deduped() {
        let csv = parse("sku,image,small_image\nDEDUP-1,/m/y/same.jpg,/m/y/same.jpg\n");
        let rows = collect_gallery(&csv, &HashMap::from([("DEDUP-1".to_string(), 1u64)]));
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,image\nSKU-1,/m/y/x.jpg\n");
        let rows = collect_gallery(&csv, &HashMap::new());
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn flush_links_each_image_to_its_product() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-GAL-A', 'RUST-GAL-B')").execute(&pool).await.unwrap();
        let a = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-GAL-A')")
            .execute(&pool).await.unwrap().last_insert_id();
        let b = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-GAL-B')")
            .execute(&pool).await.unwrap().last_insert_id();

        let rows = vec![
            GalleryRow { entity_id: a, pool_row: ProductMediaGallery { value_id: 0, attribute_id: 87, value: Some("/m/y/a1.jpg".into()), media_type: "image".into(), disabled: 0 } },
            GalleryRow { entity_id: a, pool_row: ProductMediaGallery { value_id: 0, attribute_id: 87, value: Some("/m/y/a2.jpg".into()), media_type: "image".into(), disabled: 0 } },
            GalleryRow { entity_id: b, pool_row: ProductMediaGallery { value_id: 0, attribute_id: 87, value: Some("/m/y/b1.jpg".into()), media_type: "image".into(), disabled: 0 } },
        ];
        flush_gallery(&pool, &rows, 500).await.unwrap();

        let a_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_entity_media_gallery_value_to_entity WHERE entity_id = ?")
            .bind(a).fetch_one(&pool).await.unwrap();
        let b_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_entity_media_gallery_value_to_entity WHERE entity_id = ?")
            .bind(b).fetch_one(&pool).await.unwrap();
        assert_eq!(a_count, 2);
        assert_eq!(b_count, 1);

        // Every link must point at a value_id that actually exists in the pool.
        let orphaned: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM catalog_product_entity_media_gallery_value_to_entity vte \
             LEFT JOIN catalog_product_entity_media_gallery g ON g.value_id = vte.value_id \
             WHERE vte.entity_id IN (?, ?) AND g.value_id IS NULL",
        )
        .bind(a)
        .bind(b)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphaned, 0);

        sqlx::query(
            "DELETE g, vte FROM catalog_product_entity_media_gallery g \
             JOIN catalog_product_entity_media_gallery_value_to_entity vte ON vte.value_id = g.value_id \
             WHERE vte.entity_id IN (?, ?)",
        )
        .bind(a)
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-GAL-A', 'RUST-GAL-B')").execute(&pool).await.unwrap();
    }
}
