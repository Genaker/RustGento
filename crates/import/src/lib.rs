//! Import service -- mirrors GoGento's `service/product/import_*.go`: parse
//! a product CSV, resolve/insert entities, bucket attribute/stock/price
//! values by target table, and flush everything concurrently. This is the
//! primary benchmarked path for the Go-vs-Rust comparison (see the project
//! plan's "benchmark methodology" section).

mod attributes;
mod csv_parse;
mod eav_bucket;
mod entities;
mod error;
mod flush;
mod price_bucket;
mod run;
mod sku_lookup;
mod stock_bucket;
mod validate;

#[cfg(test)]
mod test_support;

pub use attributes::{AttributeMeta, AttributesByCode};
pub use csv_parse::{parse_csv, ParsedCsv};
pub use eav_bucket::{bucket_rows, BucketedEav, EavValue};
pub use entities::insert_new_products;
pub use error::ImportError;
pub use flush::{flush_datetime, flush_decimal, flush_int, flush_price, flush_stock, flush_text, flush_varchar};
pub use price_bucket::{collect_price, PRICE_COLUMNS, PRICE_WEBSITE_ID};
pub use run::{import_products, ImportOptions, ImportResult};
pub use sku_lookup::lookup_existing_skus;
pub use stock_bucket::{collect_stock, STOCK_COLUMNS};
pub use validate::{parse_datetime_value, parse_decimal_value, parse_int_value};
