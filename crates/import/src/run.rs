use crate::attributes::AttributesByCode;
use crate::csv_parse::parse_csv;
use crate::eav_bucket::{bucket_rows, BucketedEav};
use crate::entities::insert_new_products;
use crate::error::ImportError;
use crate::flush::{flush_datetime, flush_decimal, flush_int, flush_price, flush_stock, flush_text, flush_varchar};
use crate::price_bucket::collect_price;
use crate::sku_lookup::lookup_existing_skus;
use crate::stock_bucket::collect_stock;
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
/// rows -> bucket attribute/stock/price values -> flush all seven target
/// tables concurrently. Mirrors Go's `ImportProducts`
/// (`service/product/import_service.go`), raw-SQL mode (sqlx *is* the raw-SQL
/// path here -- there's no separate ORM mode to toggle, see the project plan).
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

    let mut skus: Vec<String> = csv.rows.iter().filter_map(|row| csv.sku(row).map(str::to_string)).collect();
    skus.sort_unstable();
    skus.dedup();

    let lookup_insert_start = Instant::now();
    let mut sku_to_id = lookup_existing_skus(pool, &skus, opts.batch_size).await?;
    let updated_count = sku_to_id.len();

    let new_skus: Vec<String> = skus.into_iter().filter(|s| !sku_to_id.contains_key(s)).collect();
    let created_count = new_skus.len();
    if !new_skus.is_empty() {
        let inserted = insert_new_products(pool, &new_skus, "simple", opts.attribute_set_id, opts.batch_size).await?;
        sku_to_id.extend(inserted);
    }
    db_time += lookup_insert_start.elapsed();

    let process_start = Instant::now();
    let (eav, mut warnings) = bucket_rows(&csv, &sku_to_id, &attrs_by_code, opts.store_id);
    let (stock_rows, stock_warnings) = collect_stock(&csv, &sku_to_id);
    let (price_rows, price_warnings) = collect_price(&csv, &sku_to_id);
    warnings.extend(stock_warnings);
    warnings.extend(price_warnings);
    let process_time = process_start.elapsed();

    let mut eav_counts = HashMap::with_capacity(5);
    eav_counts.insert("varchar", eav.varchar.len());
    eav_counts.insert("int", eav.int.len());
    eav_counts.insert("decimal", eav.decimal.len());
    eav_counts.insert("text", eav.text.len());
    eav_counts.insert("datetime", eav.datetime.len());
    let stock_count = stock_rows.len();
    let price_count = price_rows.len();

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
}
