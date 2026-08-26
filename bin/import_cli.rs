//! Standalone product-import benchmark CLI. Reads a CSV file and reports a
//! timing/count summary comparable against an equivalent Go implementation's
//! benchmark output.

use clap::{Parser, ValueEnum};
use config::{DbConfig, PgDbConfig};
use import::{import_products, import_products_pg, ImportOptions, PgWriteMode};
use std::fs::File;
use std::process::ExitCode;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq)]
enum Driver {
    Mysql,
    Postgres,
}

/// Mirrors `import::PgWriteMode` -- a separate CLI-facing enum since
/// `clap::ValueEnum` can't be derived on a type in another crate.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Default)]
enum PgWriteModeArg {
    #[default]
    Insert,
    Copy,
}

impl From<PgWriteModeArg> for PgWriteMode {
    fn from(arg: PgWriteModeArg) -> Self {
        match arg {
            PgWriteModeArg::Insert => PgWriteMode::Insert,
            PgWriteModeArg::Copy => PgWriteMode::Copy,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "gogento-import", about = "Benchmark: import products from CSV into a Magento-shaped MySQL or Postgres schema")]
struct Args {
    /// Path to the CSV file to import.
    #[arg(short, long)]
    file: String,

    /// Target database. `postgres` writes to the scoped "core" schema in
    /// `sql/postgres_schema.sql` (entity + EAV values + stock + price only
    /// -- see that file's header for what's out of scope).
    #[arg(long, value_enum, default_value_t = Driver::Mysql)]
    driver: Driver,

    /// Postgres-only: how the 5 EAV value-table flushes write their rows.
    /// `insert` (default) is plain batched `INSERT ... ON CONFLICT`, the
    /// same shape as the MySQL path -- simpler and the safer choice.
    /// `copy` streams via `COPY FROM STDIN` into a staging table then does
    /// one set-based merge -- ~30% faster on a fresh-insert workload (see
    /// the README's Postgres performance section) but a newer, less
    /// exercised code path. Ignored when `--driver mysql`.
    #[arg(long, value_enum, default_value_t = PgWriteModeArg::Insert)]
    pg_write_mode: PgWriteModeArg,

    /// Store ID to write EAV values under.
    #[arg(long, default_value_t = 0)]
    store: u16,

    /// Rows per batched upsert statement.
    #[arg(long, default_value_t = 500)]
    batch_size: usize,

    /// attribute_set_id assigned to newly created products.
    #[arg(long, default_value_t = 4)]
    attribute_set: u16,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    let file = match File::open(&args.file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {}: {e}", args.file);
            return ExitCode::FAILURE;
        }
    };

    let opts = ImportOptions { store_id: args.store, batch_size: args.batch_size, attribute_set_id: args.attribute_set };

    let result = match args.driver {
        Driver::Mysql => {
            let pool = match DbConfig::from_env().build_pool().await {
                Ok(pool) => pool,
                Err(e) => {
                    eprintln!("failed to connect to database: {e}");
                    return ExitCode::FAILURE;
                }
            };
            import_products(&pool, file, opts).await
        }
        Driver::Postgres => {
            let pool = match PgDbConfig::from_env().build_pool().await {
                Ok(pool) => pool,
                Err(e) => {
                    eprintln!("failed to connect to database: {e}");
                    return ExitCode::FAILURE;
                }
            };
            import_products_pg(&pool, file, opts, args.pg_write_mode.into()).await
        }
    };
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("import failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let total_eav = result.total_eav_rows();
    let products_per_sec = result.created as f64 / result.total_time.as_secs_f64();
    let products_per_min = products_per_sec * 60.0;
    let eav_per_sec = total_eav as f64 / result.total_time.as_secs_f64();

    println!("=== Rust Import Performance ===");
    println!("Driver:         {:?}", args.driver);
    if args.driver == Driver::Postgres {
        println!("PG write mode:  {:?}", args.pg_write_mode);
    }
    println!("Rows in CSV:    {}", result.total_rows);
    println!("Products:       {} created, {} updated", result.created, result.updated);
    println!(
        "EAV rows:       {} (varchar={} int={} decimal={} text={} datetime={})",
        total_eav,
        result.eav_counts.get("varchar").copied().unwrap_or(0),
        result.eav_counts.get("int").copied().unwrap_or(0),
        result.eav_counts.get("decimal").copied().unwrap_or(0),
        result.eav_counts.get("text").copied().unwrap_or(0),
        result.eav_counts.get("datetime").copied().unwrap_or(0),
    );
    println!("Stock rows:     {}", result.stock_count);
    println!("Price rows:     {}", result.price_count);
    let extended_total = result.category_link_count
        + result.tier_price_count
        + result.product_link_count
        + result.custom_option_count
        + result.downloadable_link_count
        + result.downloadable_sample_count
        + result.bundle_option_count
        + result.bundle_selection_count
        + result.configurable_attribute_count
        + result.configurable_link_count;
    if extended_total > 0 {
        println!(
            "Extended:       category_links={} tier_prices={} product_links={} custom_options={} \
             downloadable_links={} downloadable_samples={} bundle_options={} bundle_selections={} \
             configurable_attributes={} configurable_links={}",
            result.category_link_count,
            result.tier_price_count,
            result.product_link_count,
            result.custom_option_count,
            result.downloadable_link_count,
            result.downloadable_sample_count,
            result.bundle_option_count,
            result.bundle_selection_count,
            result.configurable_attribute_count,
            result.configurable_link_count,
        );
    }
    println!("Total time:     {:?}", result.total_time);
    println!("  - Processing: {:?}", result.process_time);
    println!("  - DB time:    {:?}", result.db_time);
    println!("Rate:           {products_per_sec:.0} products/sec | {products_per_min:.0} products/min | {eav_per_sec:.0} EAV rows/sec");
    if !result.warnings.is_empty() {
        println!("Warnings ({}):", result.warnings.len());
        for w in &result.warnings {
            println!("  - {w}");
        }
    }
    println!("===============================");

    ExitCode::SUCCESS
}
