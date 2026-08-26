//! Postgres synthetic-import path.
//!
//! This is a **parallel** write path, not a generic-over-`sqlx::Database`
//! rewrite of `run.rs`: Postgres has no unsigned integer types (every
//! MySQL `... unsigned` column here becomes a signed `i32`/`i64` on bind)
//! and no `ON DUPLICATE KEY UPDATE`/`LAST_INSERT_ID()` (replaced by
//! `INSERT ... ON CONFLICT ... DO UPDATE` and `RETURNING`, respectively --
//! see `sql/postgres_schema.sql`). All the DB-free logic (CSV parsing, EAV
//! bucketing, stock/price collection) is reused unchanged from the MySQL
//! path; only the table-writing functions are Postgres-native.
//!
//! Deliberately scoped to the "core" tables this project's README
//! benchmarks: entity + the 5 EAV value tables + stock + price index.
//! Categories, tier prices, product links, custom options, downloadable,
//! bundle, and configurable products are not part of this Postgres mimic.

use crate::attributes::AttributesByCode;
use crate::csv_parse::parse_csv;
use crate::eav_bucket::{bucket_rows, BucketedEav, EavValue};
use crate::entities::NewProduct;
use crate::error::ImportError;
use crate::price_bucket::collect_price;
use crate::run::{ImportOptions, ImportResult};
use crate::stock_bucket::collect_stock;
use chrono::NaiveDateTime;
use entity::{EavAttribute, ProductIndexPrice, StockItem, PRODUCT_ENTITY_TYPE_ID};
use sqlx::{PgPool, Postgres, QueryBuilder};
use std::collections::HashMap;
use std::io::Read;
use std::time::{Duration, Instant};

/// Which strategy the 5 EAV value-table flushes use. `Insert` -- plain
/// batched `INSERT ... ON CONFLICT`, the same shape as the MySQL path --
/// is the default: it's the simpler, longer-exercised code path (no
/// temporary tables, no `COPY` protocol handshake), so it's the safer
/// choice when correctness matters more than the last ~30% of throughput.
/// `Copy` trades that simplicity for speed (see the top-level README's
/// Postgres performance section for the measured difference) via a
/// `COPY FROM STDIN` + temp-table + merge strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PgWriteMode {
    #[default]
    Insert,
    Copy,
}

/// `eav_attribute` decoded with Postgres-native (signed) column types, then
/// converted into the shared `entity::EavAttribute` so the rest of the
/// pipeline (`AttributesByCode`, `bucket_rows`) doesn't need a Postgres
/// variant of its own.
#[derive(sqlx::FromRow)]
struct PgEavAttributeRow {
    attribute_id: i32,
    entity_type_id: i32,
    attribute_code: String,
    attribute_model: Option<String>,
    backend_model: Option<String>,
    backend_type: String,
    backend_table: Option<String>,
    frontend_model: Option<String>,
    frontend_input: Option<String>,
    frontend_label: Option<String>,
    frontend_class: Option<String>,
    source_model: Option<String>,
    is_required: i32,
    is_user_defined: i32,
    default_value: Option<String>,
    is_unique: i32,
    note: Option<String>,
}

impl From<PgEavAttributeRow> for EavAttribute {
    fn from(r: PgEavAttributeRow) -> Self {
        EavAttribute {
            attribute_id: r.attribute_id as u16,
            entity_type_id: r.entity_type_id as u16,
            attribute_code: r.attribute_code,
            attribute_model: r.attribute_model,
            backend_model: r.backend_model,
            backend_type: r.backend_type,
            backend_table: r.backend_table,
            frontend_model: r.frontend_model,
            frontend_input: r.frontend_input,
            frontend_label: r.frontend_label,
            frontend_class: r.frontend_class,
            source_model: r.source_model,
            is_required: r.is_required as u16,
            is_user_defined: r.is_user_defined as u16,
            default_value: r.default_value,
            is_unique: r.is_unique as u16,
            note: r.note,
        }
    }
}

async fn fetch_attributes(pool: &PgPool) -> Result<Vec<EavAttribute>, sqlx::Error> {
    let rows: Vec<PgEavAttributeRow> = sqlx::query_as("SELECT * FROM eav_attribute WHERE entity_type_id = $1")
        .bind(PRODUCT_ENTITY_TYPE_ID as i32)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(EavAttribute::from).collect())
}

/// Postgres equivalent of `sku_lookup::lookup_existing_skus`.
async fn lookup_existing_skus(pool: &PgPool, skus: &[String], batch_size: usize) -> Result<HashMap<String, u64>, sqlx::Error> {
    let mut map = HashMap::with_capacity(skus.len());
    if skus.is_empty() {
        return Ok(map);
    }

    for chunk in skus.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT entity_id, sku FROM catalog_product_entity WHERE sku IN (");
        {
            let mut separated = qb.separated(", ");
            for sku in chunk {
                separated.push_bind(sku);
            }
        }
        qb.push(")");
        let rows: Vec<(i64, String)> = qb.build_query_as().fetch_all(pool).await?;
        for (entity_id, sku) in rows {
            map.insert(sku, entity_id as u64);
        }
    }

    Ok(map)
}

/// Postgres equivalent of `entities::insert_new_products`. Rather than lean
/// on MySQL's consecutive-auto-increment-lock trick, this binds each new
/// row's `sku` and reads it back via `RETURNING sku, entity_id` -- correct
/// regardless of what order Postgres happens to process a multi-row INSERT
/// in, since the mapping is built by SKU rather than by position.
async fn insert_new_products(pool: &PgPool, entries: &[NewProduct], attribute_set_id: u16, batch_size: usize) -> Result<HashMap<String, u64>, sqlx::Error> {
    let mut map = HashMap::with_capacity(entries.len());
    if entries.is_empty() {
        return Ok(map);
    }

    for chunk in entries.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) ");
        qb.push_values(chunk, |mut b, entry: &NewProduct| {
            b.push_bind(attribute_set_id as i32).push_bind(&entry.type_id).push_bind(&entry.sku);
        });
        qb.push(" RETURNING sku, entity_id");
        let rows: Vec<(String, i64)> = qb.build_query_as().fetch_all(pool).await?;
        for (sku, entity_id) in rows {
            map.insert(sku, entity_id as u64);
        }
    }

    Ok(map)
}

/// Per-value-type text rendering for the `COPY ... FORMAT csv` staging step
/// below -- every EAV value type stages through a `TEXT` column and gets
/// cast to its real column type in the merge `INSERT`, so this only needs
/// to produce a string Postgres's implicit `text -> $pg_type` cast accepts.
trait CopyText {
    fn copy_text(&self) -> String;
}
impl CopyText for String {
    fn copy_text(&self) -> String {
        self.clone()
    }
}
impl CopyText for i32 {
    fn copy_text(&self) -> String {
        self.to_string()
    }
}
impl CopyText for f64 {
    fn copy_text(&self) -> String {
        self.to_string()
    }
}
impl CopyText for NaiveDateTime {
    fn copy_text(&self) -> String {
        self.format("%Y-%m-%d %H:%M:%S%.f").to_string()
    }
}

/// Generates one Postgres `flush_<name>_pg_copy` function per EAV value
/// table -- the `PgWriteMode::Copy` strategy.
///
/// Measured ~38% faster than the equivalent batched
/// `INSERT ... ON CONFLICT` (see the top-level README's Postgres
/// performance section) for a fresh-insert workload: `COPY FROM STDIN`
/// streams every row into a per-transaction, `ON COMMIT DROP` temporary
/// table with no indexes/constraints to check and no chunking limit, and
/// only the single set-based merge `INSERT ... SELECT ... ON CONFLICT`
/// that follows pays the "conflict arbiter" cost `ON CONFLICT` always
/// carries -- once, as one statement, instead of once per chunked
/// round trip. No `EavValue` in this pipeline is ever null (`bucket_rows`
/// only emits a row when a cell resolves to a real value), so the CSV
/// staging step never needs to represent `NULL`.
macro_rules! impl_eav_flush_pg_copy {
    ($fn_name:ident, $table:literal, $pg_type:literal, $value_ty:ty) => {
        async fn $fn_name(pool: &PgPool, rows: &[EavValue<$value_ty>], _batch_size: usize) -> Result<(), sqlx::Error> {
            if rows.is_empty() {
                return Ok(());
            }
            let mut tx = pool.begin().await?;

            let staging = format!("pg_copy_stage_{}", $table);
            sqlx::query(&format!(
                "CREATE TEMPORARY TABLE {staging} (entity_id BIGINT, attribute_id INTEGER, store_id INTEGER, value TEXT) ON COMMIT DROP"
            ))
            .execute(&mut *tx)
            .await?;

            let mut writer = csv::WriterBuilder::new().has_headers(false).from_writer(Vec::new());
            for row in rows {
                writer
                    .write_record([row.entity_id.to_string(), row.attribute_id.to_string(), row.store_id.to_string(), row.value.copy_text()])
                    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            }
            let csv_bytes = writer.into_inner().map_err(|e| sqlx::Error::Protocol(e.to_string()))?;

            let mut copy_in = tx.copy_in_raw(&format!("COPY {staging} (entity_id, attribute_id, store_id, value) FROM STDIN WITH (FORMAT csv)")).await?;
            copy_in.send(csv_bytes.as_slice()).await?;
            copy_in.finish().await?;

            sqlx::query(&format!(
                "INSERT INTO {} (entity_id, attribute_id, store_id, value) \
                 SELECT entity_id, attribute_id, store_id, value::{} FROM {staging} \
                 ON CONFLICT (entity_id, attribute_id, store_id) DO UPDATE SET value = EXCLUDED.value",
                $table, $pg_type,
            ))
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            Ok(())
        }
    };
}

impl_eav_flush_pg_copy!(flush_varchar_pg_copy, "catalog_product_entity_varchar", "varchar", String);
impl_eav_flush_pg_copy!(flush_int_pg_copy, "catalog_product_entity_int", "integer", i32);
impl_eav_flush_pg_copy!(flush_decimal_pg_copy, "catalog_product_entity_decimal", "double precision", f64);
impl_eav_flush_pg_copy!(flush_text_pg_copy, "catalog_product_entity_text", "text", String);
impl_eav_flush_pg_copy!(flush_datetime_pg_copy, "catalog_product_entity_datetime", "timestamp", NaiveDateTime);

/// Generates one Postgres `flush_<name>_pg_insert` function per EAV value
/// table -- the `PgWriteMode::Insert` (default) strategy: plain batched
/// `INSERT ... ON CONFLICT DO UPDATE`, chunked at `batch_size`, the same
/// shape as the MySQL path's `flush::impl_eav_flush!`.
macro_rules! impl_eav_flush_pg_insert {
    ($fn_name:ident, $table:literal, $value_ty:ty) => {
        async fn $fn_name(pool: &PgPool, rows: &[EavValue<$value_ty>], batch_size: usize) -> Result<(), sqlx::Error> {
            if rows.is_empty() {
                return Ok(());
            }
            let mut tx = pool.begin().await?;
            for chunk in rows.chunks(batch_size.max(1)) {
                let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(concat!(
                    "INSERT INTO ",
                    $table,
                    " (entity_id, attribute_id, store_id, value) "
                ));
                qb.push_values(chunk, |mut b, row: &EavValue<$value_ty>| {
                    b.push_bind(row.entity_id as i64)
                        .push_bind(row.attribute_id as i32)
                        .push_bind(row.store_id as i32)
                        .push_bind(row.value.clone());
                });
                qb.push(" ON CONFLICT (entity_id, attribute_id, store_id) DO UPDATE SET value = EXCLUDED.value");
                qb.build().execute(&mut *tx).await?;
            }
            tx.commit().await?;
            Ok(())
        }
    };
}

impl_eav_flush_pg_insert!(flush_varchar_pg_insert, "catalog_product_entity_varchar", String);
impl_eav_flush_pg_insert!(flush_int_pg_insert, "catalog_product_entity_int", i32);
impl_eav_flush_pg_insert!(flush_decimal_pg_insert, "catalog_product_entity_decimal", f64);
impl_eav_flush_pg_insert!(flush_text_pg_insert, "catalog_product_entity_text", String);
impl_eav_flush_pg_insert!(flush_datetime_pg_insert, "catalog_product_entity_datetime", NaiveDateTime);

/// Postgres equivalent of `flush::flush_stock`.
async fn flush_stock_pg(pool: &PgPool, rows: &[StockItem], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO cataloginventory_stock_item \
             (product_id, stock_id, qty, is_in_stock, manage_stock, min_qty, min_sale_qty, max_sale_qty) ",
        );
        qb.push_values(chunk, |mut b, row: &StockItem| {
            b.push_bind(row.product_id as i64)
                .push_bind(row.stock_id as i32)
                .push_bind(row.qty)
                .push_bind(row.is_in_stock as i32)
                .push_bind(row.manage_stock as i32)
                .push_bind(row.min_qty)
                .push_bind(row.min_sale_qty)
                .push_bind(row.max_sale_qty);
        });
        qb.push(
            " ON CONFLICT (product_id, stock_id) DO UPDATE SET qty = EXCLUDED.qty, \
              is_in_stock = EXCLUDED.is_in_stock, manage_stock = EXCLUDED.manage_stock, \
              min_qty = EXCLUDED.min_qty, min_sale_qty = EXCLUDED.min_sale_qty, \
              max_sale_qty = EXCLUDED.max_sale_qty",
        );
        qb.build().execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Postgres equivalent of `flush::flush_price`.
async fn flush_price_pg(pool: &PgPool, rows: &[ProductIndexPrice], batch_size: usize) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for chunk in rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO catalog_product_index_price \
             (entity_id, customer_group_id, website_id, tax_class_id, price, final_price, min_price, max_price, tier_price) ",
        );
        qb.push_values(chunk, |mut b, row: &ProductIndexPrice| {
            b.push_bind(row.entity_id as i64)
                .push_bind(row.customer_group_id as i64)
                .push_bind(row.website_id as i32)
                .push_bind(row.tax_class_id.map(|v| v as i32))
                .push_bind(row.price)
                .push_bind(row.final_price)
                .push_bind(row.min_price)
                .push_bind(row.max_price)
                .push_bind(row.tier_price);
        });
        qb.push(
            " ON CONFLICT (entity_id, customer_group_id, website_id) DO UPDATE SET \
              price = EXCLUDED.price, final_price = EXCLUDED.final_price, \
              min_price = EXCLUDED.min_price, max_price = EXCLUDED.max_price, \
              tier_price = EXCLUDED.tier_price",
        );
        qb.build().execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Postgres equivalent of `run::import_products`, scoped to the core tables
/// (see module docs). `category_link_count` and every other
/// extended-feature count on the returned [`ImportResult`] are always zero
/// here -- this path doesn't touch those tables at all.
///
/// `write_mode` only affects the 5 EAV value-table flushes -- entity
/// creation (`RETURNING`) and stock/price (batched `INSERT ... ON
/// CONFLICT`) are unaffected by it either way.
pub async fn import_products_pg<R: Read>(pool: &PgPool, reader: R, opts: ImportOptions, write_mode: PgWriteMode) -> Result<ImportResult, ImportError> {
    let total_start = Instant::now();
    let mut db_time = Duration::ZERO;

    let csv = parse_csv(reader)?;
    let total_rows = csv.rows.len();

    let attrs_start = Instant::now();
    let attrs = fetch_attributes(pool).await?;
    db_time += attrs_start.elapsed();
    let attrs_by_code = AttributesByCode::build(&attrs);

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

    let lookup_insert_start = Instant::now();
    let mut sku_to_id = lookup_existing_skus(pool, &skus, opts.batch_size).await?;
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
    match write_mode {
        PgWriteMode::Copy => {
            spawn_flush!(flush_varchar_pg_copy, varchar);
            spawn_flush!(flush_int_pg_copy, int);
            spawn_flush!(flush_decimal_pg_copy, decimal);
            spawn_flush!(flush_text_pg_copy, text);
            spawn_flush!(flush_datetime_pg_copy, datetime);
        }
        PgWriteMode::Insert => {
            spawn_flush!(flush_varchar_pg_insert, varchar);
            spawn_flush!(flush_int_pg_insert, int);
            spawn_flush!(flush_decimal_pg_insert, decimal);
            spawn_flush!(flush_text_pg_insert, text);
            spawn_flush!(flush_datetime_pg_insert, datetime);
        }
    }
    spawn_flush!(flush_stock_pg, stock_rows);
    spawn_flush!(flush_price_pg, price_rows);

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
        category_link_count: 0,
        tier_price_count: 0,
        product_link_count: 0,
        custom_option_count: 0,
        downloadable_link_count: 0,
        downloadable_sample_count: 0,
        bundle_option_count: 0,
        bundle_selection_count: 0,
        configurable_attribute_count: 0,
        configurable_link_count: 0,
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

    const SYNTHETIC_CSV: &str = include_str!("../../../fixtures/synthetic_products.csv");

    /// Runs the same create-then-reimport-and-upsert assertions against a
    /// live Postgres instance for a given [`PgWriteMode`] and SKU prefix --
    /// shared by the `Insert` and `Copy` mode tests below so both strategies
    /// get identical correctness coverage, not just the default.
    async fn assert_full_pipeline_upserts_correctly(write_mode: PgWriteMode, sku_prefix: &str) {
        let Some(pool) = crate::test_support_pg::test_pool().await else { return };

        let csv = SYNTHETIC_CSV.replace("SYN-TEST-", sku_prefix);
        let delete_sql = format!("DELETE FROM catalog_product_entity WHERE sku LIKE '{sku_prefix}%'");
        sqlx::query(&delete_sql).execute(&pool).await.unwrap();

        let result = import_products_pg(&pool, Cursor::new(csv.as_bytes().to_vec()), ImportOptions::default(), write_mode).await.unwrap();

        assert_eq!(result.total_rows, 20);
        assert_eq!(result.created, 20, "all synthetic SKUs are new");
        assert_eq!(result.updated, 0);
        assert!(result.warnings.is_empty(), "warnings: {:?}", result.warnings);
        assert_eq!(result.stock_count, 0, "fixture has no stock columns");
        assert!(result.total_eav_rows() > 0);

        let name: String = sqlx::query_scalar(&format!(
            "SELECT v.value FROM catalog_product_entity_varchar v \
             JOIN catalog_product_entity p ON p.entity_id = v.entity_id \
             JOIN eav_attribute a ON a.attribute_id = v.attribute_id \
             WHERE p.sku = '{sku_prefix}0000' AND a.attribute_code = 'name'"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(name, "Synthetic Test Product 0");

        // Re-running should now report updates, not creates -- same
        // upsert-not-duplicate contract as the MySQL path.
        let result2 = import_products_pg(&pool, Cursor::new(csv.as_bytes().to_vec()), ImportOptions::default(), write_mode).await.unwrap();
        assert_eq!(result2.created, 0);
        assert_eq!(result2.updated, 20);

        let varchar_row_count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM catalog_product_entity_varchar v \
             JOIN catalog_product_entity p ON p.entity_id = v.entity_id \
             WHERE p.sku LIKE '{sku_prefix}%'"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(varchar_row_count, result.eav_counts["varchar"] as i64, "re-import must upsert in place, not duplicate rows");

        sqlx::query(&delete_sql).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn full_pipeline_creates_updates_and_flushes_against_live_postgres_insert_mode() {
        assert_full_pipeline_upserts_correctly(PgWriteMode::Insert, "SYN-INS-").await;
    }

    #[tokio::test]
    async fn full_pipeline_creates_updates_and_flushes_against_live_postgres_copy_mode() {
        assert_full_pipeline_upserts_correctly(PgWriteMode::Copy, "SYN-CPY-").await;
    }

    #[test]
    fn insert_is_the_default_write_mode() {
        assert_eq!(PgWriteMode::default(), PgWriteMode::Insert, "Insert must stay the default -- it's the simpler, safer strategy");
    }

    /// The literal ask this module exists for: the same synthetic CSV, run
    /// through this project's importer, lands correctly in both a MySQL
    /// and a Postgres database. Skips gracefully (not fail) unless both
    /// `GOGENTO_TEST_DATABASE_URL` (MySQL) and `GOGENTO_TEST_POSTGRES_URL`
    /// are reachable.
    ///
    /// Uses its own `SYN-DUAL-*` SKU prefix rather than the shared
    /// `SYN-TEST-*` fixture data -- `cargo test` runs tests in parallel
    /// threads by default, and this test's own DB instances are the same
    /// ones `full_pipeline_creates_updates_and_flushes_against_live_postgres`
    /// exercises, so reusing the same SKUs would race that test's
    /// insert/cleanup the same way `flush.rs`'s per-test scratch products do.
    #[tokio::test]
    async fn same_synthetic_csv_imports_into_both_mysql_and_postgres() {
        let Some(mysql_pool) = crate::test_support::test_pool().await else { return };
        let Some(pg_pool) = crate::test_support_pg::test_pool().await else { return };

        let dual_csv = SYNTHETIC_CSV.replace("SYN-TEST-", "SYN-DUAL-");

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'SYN-DUAL-%'").execute(&mysql_pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'SYN-DUAL-%'").execute(&pg_pool).await.unwrap();

        let mysql_result = crate::run::import_products(&mysql_pool, Cursor::new(dual_csv.as_bytes().to_vec()), ImportOptions::default()).await.unwrap();
        let pg_result =
            import_products_pg(&pg_pool, Cursor::new(dual_csv.as_bytes().to_vec()), ImportOptions::default(), PgWriteMode::default()).await.unwrap();

        assert_eq!(mysql_result.total_rows, pg_result.total_rows);
        assert_eq!(mysql_result.created, pg_result.created);
        assert_eq!(mysql_result.created, 20);
        assert_eq!(mysql_result.eav_counts["varchar"], pg_result.eav_counts["varchar"]);
        assert_eq!(mysql_result.eav_counts["decimal"], pg_result.eav_counts["decimal"]);

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'SYN-DUAL-%'").execute(&mysql_pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'SYN-DUAL-%'").execute(&pg_pool).await.unwrap();
    }
}
