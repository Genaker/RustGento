//! Import service -- parse a product CSV, resolve/insert entities, bucket
//! attribute/stock/price values by target table, and flush everything
//! concurrently. This is the primary benchmarked path (see the top-level
//! README's benchmark section).

mod attributes;
mod bundle;
mod categories;
mod configurable;
mod csv_parse;
mod custom_options;
mod downloadable;
mod eav_bucket;
mod entities;
mod error;
mod flush;
mod gallery;
mod links;
mod price_bucket;
mod run;
mod sku_lookup;
mod stock_bucket;
mod tier_prices;
mod validate;

#[cfg(test)]
mod test_support;

pub use attributes::{AttributeMeta, AttributesByCode};
pub use bundle::{
    bundle_selection_skus, collect_bundle_options, flush_bundle_options, option_count as bundle_option_count,
    selection_count as bundle_selection_count, BundleOption, BundleSelection, ParentBundleOptions, BUNDLE_OPTIONS_COLUMN,
};
pub use categories::{collect_categories, flush_categories, CategoryAssignment};
pub use configurable::{
    collect_configurable, configurable_child_skus, flush_configurable, CONFIGURABLE_ATTRIBUTES_COLUMN,
    CONFIGURABLE_VARIATIONS_COLUMN,
};
pub use csv_parse::{parse_csv, ParsedCsv};
pub use custom_options::{
    collect_custom_options, flush_custom_options, total_option_count as custom_option_count, CustomOption,
    CustomOptionValue, ProductCustomOptions,
};
pub use downloadable::{collect_downloadable, flush_downloadable, DOWNLOADABLE_LINK_COLUMN, DOWNLOADABLE_SAMPLE_COLUMN};
pub use eav_bucket::{bucket_rows, BucketedEav, EavValue};
pub use entities::{insert_new_products, NewProduct};
pub use error::ImportError;
pub use flush::{flush_datetime, flush_decimal, flush_int, flush_price, flush_stock, flush_text, flush_varchar};
pub use gallery::{collect_gallery, flush_gallery, GalleryRow, GALLERY_COLUMNS};
pub use links::{collect_product_links, flush_product_links, link_sku_columns, PRODUCT_LINK_COLUMNS};
pub use price_bucket::{collect_price, PRICE_COLUMNS, PRICE_WEBSITE_ID};
pub use run::{import_products, ImportOptions, ImportResult};
pub use sku_lookup::lookup_existing_skus;
pub use stock_bucket::{collect_stock, STOCK_COLUMNS};
pub use tier_prices::{collect_tier_prices, flush_tier_prices};
pub use validate::{parse_datetime_value, parse_decimal_value, parse_int_value};
