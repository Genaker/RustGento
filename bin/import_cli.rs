//! Standalone product-import benchmark CLI -- the Rust counterpart to
//! GoGento's `go run . products:import` (`cmd/product_import.go`). Reads a
//! CSV file and reports the same shape of timing/count summary so results
//! are directly diffable against the Go benchmark output.

use clap::Parser;
use config::DbConfig;
use import::{import_products, ImportOptions};
use std::fs::File;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "gogento-import", about = "Benchmark: import products from CSV into a Magento-shaped MySQL schema")]
struct Args {
    /// Path to the CSV file to import.
    #[arg(short, long)]
    file: String,

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

    let pool = match DbConfig::from_env().build_pool().await {
        Ok(pool) => pool,
        Err(e) => {
            eprintln!("failed to connect to database: {e}");
            return ExitCode::FAILURE;
        }
    };

    let file = match File::open(&args.file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {}: {e}", args.file);
            return ExitCode::FAILURE;
        }
    };

    let opts = ImportOptions { store_id: args.store, batch_size: args.batch_size, attribute_set_id: args.attribute_set };

    let result = match import_products(&pool, file, opts).await {
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
