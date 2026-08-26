use crate::attributes::AttributesByCode;
use crate::bundle::{bundle_selection_skus, collect_bundle_options, flush_bundle_options, option_count as bundle_option_count, selection_count as bundle_selection_count};
use crate::categories::{collect_categories, flush_categories};
use crate::configurable::{collect_configurable, configurable_child_skus, flush_configurable};
use crate::csv_parse::parse_csv;
use crate::custom_options::{collect_custom_options, flush_custom_options, total_option_count as custom_option_count};
use crate::downloadable::{collect_downloadable, flush_downloadable};
use crate::eav_bucket::{bucket_rows, BucketedEav};
use crate::entities::{insert_new_products, NewProduct};
use crate::error::ImportError;
use crate::flush::{flush_datetime, flush_decimal, flush_int, flush_price, flush_stock, flush_text, flush_varchar};
use crate::gallery::collect_gallery;
use crate::gallery::flush_gallery as flush_gallery_rows;
use crate::links::{collect_product_links, flush_product_links, link_sku_columns};
use crate::price_bucket::collect_price;
use crate::sku_lookup::lookup_existing_skus;
use crate::stock_bucket::collect_stock;
use crate::tier_prices::{collect_tier_prices, flush_tier_prices};
use entity::{EavAttribute, PRODUCT_ENTITY_TYPE_ID};
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::io::Read;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub store_id: u16,
    pub batch_size: usize,
    pub attribute_set_id: u16,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions { store_id: 0, batch_size: 500, attribute_set_id: 4 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImportResult {
    pub total_rows: usize,
    pub created: usize,
    pub updated: usize,
    pub eav_counts: HashMap<&'static str, usize>,
    pub stock_count: usize,
    pub price_count: usize,
    pub category_link_count: usize,
    pub tier_price_count: usize,
    pub product_link_count: usize,
    pub custom_option_count: usize,
    pub downloadable_link_count: usize,
    pub downloadable_sample_count: usize,
    pub bundle_option_count: usize,
    pub bundle_selection_count: usize,
    pub configurable_attribute_count: usize,
    pub configurable_link_count: usize,
    pub warnings: Vec<String>,
    /// CSV parse + in-memory bucketing/validation time.
    pub process_time: Duration,
    /// SKU lookup + new-entity insert + all table flushes.
    pub db_time: Duration,
    pub total_time: Duration,
}

impl ImportResult {
    pub fn total_eav_rows(&self) -> usize {
        self.eav_counts.values().sum()
    }
}

/// Runs a full product import: parse CSV -> resolve/insert `catalog_product_entity`
/// rows -> bucket attribute/stock/price/category/tier-price/link/option/
/// downloadable/bundle/configurable values -> flush every target table
/// concurrently (sqlx's parameterized queries are the raw-SQL path here --
/// there's no separate ORM layer to toggle).
pub async fn import_products<R: Read>(
    pool: &MySqlPool,
    reader: R,
    opts: ImportOptions,
) -> Result<ImportResult, ImportError> {
    let total_start = Instant::now();
    let mut db_time = Duration::ZERO;

    let csv = parse_csv(reader)?;
    let total_rows = csv.rows.len();

    let attrs_start = Instant::now();
    let attrs: Vec<EavAttribute> = sqlx::query_as("SELECT * FROM eav_attribute WHERE entity_type_id = ?")
        .bind(PRODUCT_ENTITY_TYPE_ID)
        .fetch_all(pool)
        .await?;
    db_time += attrs_start.elapsed();
    let attrs_by_code = AttributesByCode::build(&attrs);

    // sku -> type_id from the CSV's own "type_id" column (first occurrence
    // wins), used only for entities this import actually creates -- a
    // referenced-but-pre-existing SKU (link/bundle/configurable target)
    // keeps whatever type it already has.
    let type_col = csv.col_index("type_id");
    let mut sku_type: HashMap<&str, &str> = HashMap::new();
    let mut skus: Vec<String> = Vec::with_capacity(csv.rows.len());
    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        skus.push(sku.to_string());
        let type_id = type_col.and_then(|c| csv.field(row, c)).unwrap_or("simple");
        sku_type.entry(sku).or_insert(type_id);
    }
    skus.sort_unstable();
    skus.dedup();

    // SKUs referenced by a related/upsell/crosssell/grouped, bundle
    // selection, or configurable variation column also need resolving,
    // even though they won't be created if missing -- those columns may
    // point at a product that already exists but isn't itself part of
    // this CSV's primary "sku" column.
    for col in link_sku_columns(&csv) {
        skus.push(col);
    }
    skus.extend(bundle_selection_skus(&csv));
    skus.extend(configurable_child_skus(&csv));

    let lookup_insert_start = Instant::now();
    let mut sku_to_id = lookup_existing_skus(pool, &skus, opts.batch_size).await?;
    // Counted only over primary "sku" column values (sku_type's keys),
    // not the extra link/bundle/configurable-referenced SKUs also folded
    // into `skus` above -- those aren't part of this import's own product
    // set and must not inflate the updated count.
    let updated_count = sku_type.keys().filter(|sku| sku_to_id.contains_key(**sku)).count();

    let new_entries: Vec<NewProduct> = sku_type
        .iter()
        .filter(|(sku, _)| !sku_to_id.contains_key(**sku))
        .map(|(sku, type_id)| NewProduct { sku: sku.to_string(), type_id: type_id.to_string() })
        .collect();
    let created_count = new_entries.len();
    if !new_entries.is_empty() {
        let inserted = insert_new_products(pool, &new_entries, opts.attribute_set_id, opts.batch_size).await?;
        sku_to_id.extend(inserted);
    }
    db_time += lookup_insert_start.elapsed();

    let process_start = Instant::now();
    let (eav, mut warnings) = bucket_rows(&csv, &sku_to_id, &attrs_by_code, opts.store_id);
    let (stock_rows, stock_warnings) = collect_stock(&csv, &sku_to_id);
    let (price_rows, price_warnings) = collect_price(&csv, &sku_to_id);
    let (category_assignments, category_warnings) = collect_categories(&csv, &sku_to_id);
    let (tier_price_rows, tier_price_warnings) = collect_tier_prices(&csv, &sku_to_id);
    let (link_rows, link_warnings) = collect_product_links(&csv, &sku_to_id);
    let (custom_option_products, custom_option_warnings) = collect_custom_options(&csv, &sku_to_id);
    let (downloadable_links, downloadable_samples, downloadable_touched, downloadable_warnings) = collect_downloadable(&csv, &sku_to_id);
    let (bundle_products, bundle_warnings) = collect_bundle_options(&csv, &sku_to_id);
    let (configurable_attrs, configurable_links, configurable_warnings) = collect_configurable(&csv, &sku_to_id, &attrs_by_code);
    let gallery_rows = collect_gallery(&csv, &sku_to_id);

    warnings.extend(stock_warnings);
    warnings.extend(price_warnings);
    warnings.extend(category_warnings);
    warnings.extend(tier_price_warnings);
    warnings.extend(link_warnings);
    warnings.extend(custom_option_warnings);
    warnings.extend(downloadable_warnings);
    warnings.extend(bundle_warnings);
    warnings.extend(configurable_warnings);
    let process_time = process_start.elapsed();

    let mut eav_counts = HashMap::with_capacity(5);
    eav_counts.insert("varchar", eav.varchar.len());
    eav_counts.insert("int", eav.int.len());
    eav_counts.insert("decimal", eav.decimal.len());
    eav_counts.insert("text", eav.text.len());
    eav_counts.insert("datetime", eav.datetime.len());
    let stock_count = stock_rows.len();
    let price_count = price_rows.len();
    let category_link_count = category_assignments.len();
    let tier_price_count = tier_price_rows.len();
    let product_link_count = link_rows.len();
    let custom_opt_count = custom_option_count(&custom_option_products);
    let downloadable_link_count = downloadable_links.len();
    let downloadable_sample_count = downloadable_samples.len();
    let bundle_opt_count = bundle_option_count(&bundle_products);
    let bundle_sel_count = bundle_selection_count(&bundle_products);
    let configurable_attribute_count = configurable_attrs.len();
    let configurable_link_count = configurable_links.len();

    let flush_start = Instant::now();
    let BucketedEav { varchar, int, decimal, text, datetime } = eav;
    let batch_size = opts.batch_size;

    let mut tasks = tokio::task::JoinSet::new();
    macro_rules! spawn_flush {
        ($flush_fn:path, $rows:expr) => {{
            let pool = pool.clone();
            let rows = $rows;
            tasks.spawn(async move { $flush_fn(&pool, &rows, batch_size).await });
        }};
    }
    spawn_flush!(flush_varchar, varchar);
    spawn_flush!(flush_int, int);
    spawn_flush!(flush_decimal, decimal);
    spawn_flush!(flush_text, text);
    spawn_flush!(flush_datetime, datetime);
    spawn_flush!(flush_stock, stock_rows);
    spawn_flush!(flush_price, price_rows);
    spawn_flush!(flush_categories, category_assignments);
    spawn_flush!(flush_tier_prices, tier_price_rows);
    spawn_flush!(flush_product_links, link_rows);
    spawn_flush!(flush_gallery_rows, gallery_rows);

    {
        let pool = pool.clone();
        tasks.spawn(async move { flush_custom_options(&pool, &custom_option_products, batch_size).await });
    }
    {
        let pool = pool.clone();
        tasks.spawn(async move { flush_downloadable(&pool, &downloadable_links, &downloadable_samples, &downloadable_touched, batch_size).await });
    }
    {
        let pool = pool.clone();
        tasks.spawn(async move { flush_bundle_options(&pool, &bundle_products, batch_size).await });
    }
    {
        let pool = pool.clone();
        tasks.spawn(async move { flush_configurable(&pool, &configurable_attrs, &configurable_links, batch_size).await });
    }

    while let Some(res) = tasks.join_next().await {
        res.expect("a flush task panicked")?;
    }
    db_time += flush_start.elapsed();

    Ok(ImportResult {
        total_rows,
        created: created_count,
        updated: updated_count,
        eav_counts,
        stock_count,
        price_count,
        category_link_count,
        tier_price_count,
        product_link_count,
        custom_option_count: custom_opt_count,
        downloadable_link_count,
        downloadable_sample_count,
        bundle_option_count: bundle_opt_count,
        bundle_selection_count: bundle_sel_count,
        configurable_attribute_count,
        configurable_link_count,
        warnings,
        process_time,
        db_time,
        total_time: total_start.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn default_options_match_go_defaults() {
        let opts = ImportOptions::default();
        assert_eq!(opts.batch_size, 500);
        assert_eq!(opts.attribute_set_id, 4);
        assert_eq!(opts.store_id, 0);
    }

    #[test]
    fn total_eav_rows_sums_all_backend_types() {
        let mut result = ImportResult::default();
        result.eav_counts.insert("varchar", 3);
        result.eav_counts.insert("int", 2);
        assert_eq!(result.total_eav_rows(), 5);
    }

    #[tokio::test]
    async fn full_pipeline_creates_updates_and_flushes_against_live_db() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        // Clean slate for this test's SKUs.
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-RUN-TEST-%'")
            .execute(&pool)
            .await
            .unwrap();

        let csv_data = "sku,name,price,qty,price_index\n\
             RUST-IMPORT-RUN-TEST-1,Widget One,9.99,10,9.99\n\
             RUST-IMPORT-RUN-TEST-2,Widget Two,19.99,20,19.99\n";

        let result = import_products(&pool, Cursor::new(csv_data.as_bytes().to_vec()), ImportOptions::default())
            .await
            .unwrap();

        assert_eq!(result.total_rows, 2);
        assert_eq!(result.created, 2, "both SKUs are new");
        assert_eq!(result.updated, 0);
        assert!(result.warnings.is_empty(), "warnings: {:?}", result.warnings);
        assert_eq!(result.stock_count, 2);
        assert_eq!(result.price_count, 2);
        assert!(result.total_eav_rows() >= 2, "at least the varchar/decimal attributes used above");

        // Re-running against the same SKUs should now report updates, not creates.
        let result2 = import_products(&pool, Cursor::new(csv_data.as_bytes().to_vec()), ImportOptions::default())
            .await
            .unwrap();
        assert_eq!(result2.created, 0);
        assert_eq!(result2.updated, 2);

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-RUN-TEST-%'")
            .execute(&pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn type_id_from_csv_is_respected_per_row() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TYPE-TEST-%'").execute(&pool).await.unwrap();

        let csv_data = "sku,type_id\nRUST-IMPORT-TYPE-TEST-1,simple\nRUST-IMPORT-TYPE-TEST-2,configurable\n";
        import_products(&pool, Cursor::new(csv_data.as_bytes().to_vec()), ImportOptions::default()).await.unwrap();

        let simple_type: String = sqlx::query_scalar("SELECT type_id FROM catalog_product_entity WHERE sku = ?")
            .bind("RUST-IMPORT-TYPE-TEST-1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let config_type: String = sqlx::query_scalar("SELECT type_id FROM catalog_product_entity WHERE sku = ?")
            .bind("RUST-IMPORT-TYPE-TEST-2")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(simple_type, "simple");
        assert_eq!(config_type, "configurable");

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-IMPORT-TYPE-TEST-%'").execute(&pool).await.unwrap();
    }
}
