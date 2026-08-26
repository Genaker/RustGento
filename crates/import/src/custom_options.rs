use crate::csv_parse::ParsedCsv;
use entity::{is_select_option_type, is_valid_option_type, ProductOptionTypeValue};
use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CustomOptionValue {
    pub title: String,
    pub price: f64,
    pub price_type: String,
    pub sku: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomOption {
    pub option_type: String,
    pub title: String,
    pub is_require: u16,
    pub price: f64,
    pub price_type: String,
    pub sku: Option<String>,
    pub max_characters: Option<i64>,
    pub sort_order: i64,
    /// Only populated for select-type options (drop_down/radio/checkbox/multiselect).
    pub values: Vec<CustomOptionValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProductCustomOptions {
    pub product_id: u64,
    pub options: Vec<CustomOption>,
}

/// Collects the "custom_options" column:
///
///   type:title:required:price:price_type:sku:max_characters[:values]
///
/// entries separated by ";". "values" is only meaningful for the four
/// select types (drop_down/radio/checkbox/multiselect): a "|"-separated
/// list of "title~price~price_type~sku" choices.
///
/// Example: "field:Engraving:1:5.00:fixed:ENGRAVE:50;drop_down:Color:1:0:fixed::0:Red~5~fixed~RED|Blue~10~fixed~BLUE"
pub fn collect_custom_options(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> (Vec<ProductCustomOptions>, Vec<String>) {
    let mut products = Vec::new();
    let mut warnings = Vec::new();

    let Some(col) = csv.col_index("custom_options") else { return (products, warnings) };

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&product_id) = sku_to_id.get(sku) else { continue };
        let Some(val) = csv.field(row, col) else { continue };

        let mut options = Vec::new();
        for entry in val.split(';') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let (opt, entry_warnings) = parse_option_entry(sku, entry);
            warnings.extend(entry_warnings);
            if let Some(opt) = opt {
                options.push(opt);
            }
        }
        if !options.is_empty() {
            products.push(ProductCustomOptions { product_id, options });
        }
    }

    (products, warnings)
}

/// Returns `(Some(option), warnings)` on success or `(None, warnings)` if
/// the entry couldn't be turned into an option at all -- `warnings` always
/// carries every non-fatal issue found along the way (e.g. an invalid
/// optional numeric field), even when the entry is otherwise valid.
fn parse_option_entry(sku: &str, entry: &str) -> (Option<CustomOption>, Vec<String>) {
    let mut warnings = Vec::new();
    let fields: Vec<&str> = entry.split(':').collect();
    if fields.len() < 2 {
        warnings.push(format!("sku={sku}: malformed custom_options entry {entry:?}, want at least type:title"));
        return (None, warnings);
    }

    let option_type = fields[0].trim();
    if !is_valid_option_type(option_type) {
        warnings.push(format!("sku={sku}: unknown custom option type {option_type:?} in entry {entry:?}"));
        return (None, warnings);
    }
    let title = fields[1].trim();
    if title.is_empty() {
        warnings.push(format!("sku={sku}: custom option entry {entry:?} has no title"));
        return (None, warnings);
    }

    let mut opt = CustomOption {
        option_type: option_type.to_string(),
        title: title.to_string(),
        is_require: 0,
        price: 0.0,
        price_type: "fixed".to_string(),
        sku: None,
        max_characters: None,
        sort_order: 0,
        values: Vec::new(),
    };

    // Invalid optional fields below are warnings-only (not a hard failure)
    // so they don't abandon an otherwise-valid option.
    if let Some(f) = fields.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<u16>() {
            Ok(v) => opt.is_require = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid required flag {f:?} in custom option {title:?}")),
        }
    }
    if let Some(f) = fields.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<f64>() {
            Ok(v) => opt.price = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid price {f:?} in custom option {title:?}")),
        }
    }
    if let Some(f) = fields.get(4).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if f == "fixed" || f == "percent" {
            opt.price_type = f.to_string();
        } else {
            warnings.push(format!("sku={sku}: invalid price_type {f:?} in custom option {title:?}, want fixed or percent"));
        }
    }
    if let Some(f) = fields.get(5).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        opt.sku = Some(f.to_string());
    }
    if let Some(f) = fields.get(6).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<i64>() {
            Ok(v) => opt.max_characters = Some(v),
            Err(_) => warnings.push(format!("sku={sku}: invalid max_characters {f:?} in custom option {title:?}")),
        }
    }

    if is_select_option_type(option_type) {
        let values_field = fields.get(7).map(|s| s.trim()).filter(|s| !s.is_empty());
        let Some(values_field) = values_field else {
            warnings.push(format!("sku={sku}: select-type custom option {title:?} has no values"));
            return (None, warnings);
        };
        for (pos, choice) in values_field.split('|').enumerate() {
            let choice = choice.trim();
            if choice.is_empty() {
                continue;
            }
            let (v, value_warnings) = parse_option_value(sku, title, choice, pos as i64);
            warnings.extend(value_warnings);
            if let Some(v) = v {
                opt.values.push(v);
            }
        }
        if opt.values.is_empty() {
            warnings.push(format!("sku={sku}: select-type custom option {title:?} has no valid values"));
            return (None, warnings);
        }
    }

    (Some(opt), warnings)
}

/// Returns `(Some(value), warnings)` on success or `(None, warnings)` if
/// the choice couldn't be turned into a value at all.
fn parse_option_value(sku: &str, option_title: &str, choice: &str, pos: i64) -> (Option<CustomOptionValue>, Vec<String>) {
    let mut warnings = Vec::new();
    let parts: Vec<&str> = choice.split('~').collect();
    let title = parts[0].trim();
    if title.is_empty() {
        warnings.push(format!("sku={sku}: custom option {option_title:?} has a value with no title, skipping"));
        return (None, warnings);
    }

    let mut v = CustomOptionValue { title: title.to_string(), price: 0.0, price_type: "fixed".to_string(), sku: None, sort_order: pos };

    if let Some(f) = parts.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<f64>() {
            Ok(p) => v.price = p,
            Err(_) => warnings.push(format!("sku={sku}: invalid price {f:?} for value {title:?} of custom option {option_title:?}")),
        }
    }
    if let Some(f) = parts.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if f == "fixed" || f == "percent" {
            v.price_type = f.to_string();
        } else {
            warnings.push(format!("sku={sku}: invalid price_type {f:?} for value {title:?} of custom option {option_title:?}"));
        }
    }
    if let Some(f) = parts.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        v.sku = Some(f.to_string());
    }

    (Some(v), warnings)
}

/// Upserts (full-replaces) each affected product's custom option set --
/// matching how Magento's own admin option editor behaves (a save always
/// rewrites the whole set, not a merge), so reimporting the same CSV is
/// idempotent rather than accumulating duplicate options every run.
pub async fn flush_custom_options(pool: &MySqlPool, products: &[ProductCustomOptions], batch_size: usize) -> Result<(), sqlx::Error> {
    if products.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;

    let product_ids: Vec<u64> = products.iter().map(|p| p.product_id).collect();
    let placeholders = vec!["?"; product_ids.len()].join(",");

    let existing_option_ids: Vec<u64> = {
        let sql = format!("SELECT option_id FROM catalog_product_option WHERE product_id IN ({placeholders})");
        let mut q = sqlx::query_scalar(&sql);
        for id in &product_ids {
            q = q.bind(id);
        }
        q.fetch_all(&mut *tx).await?
    };
    if !existing_option_ids.is_empty() {
        let ph = vec!["?"; existing_option_ids.len()].join(",");
        let sql = format!("DELETE FROM catalog_product_option_type_value WHERE option_id IN ({ph})");
        let mut q = sqlx::query(&sql);
        for id in &existing_option_ids {
            q = q.bind(id);
        }
        q.execute(&mut *tx).await?;
    }
    {
        let sql = format!("DELETE FROM catalog_product_option WHERE product_id IN ({placeholders})");
        let mut q = sqlx::query(&sql);
        for id in &product_ids {
            q = q.bind(id);
        }
        q.execute(&mut *tx).await?;
    }

    // Flatten every option across every product into one list so it can be
    // bulk-inserted in chunked multi-row INSERTs instead of one INSERT per
    // option -- each chunk's option_ids are computed from LAST_INSERT_ID()
    // plus offset, the same consecutive-auto-increment trick flush_gallery
    // already uses for catalog_product_entity_media_gallery.
    let flat: Vec<(u64, &CustomOption)> = products.iter().flat_map(|p| p.options.iter().map(move |opt| (p.product_id, opt))).collect();

    let mut option_ids = Vec::with_capacity(flat.len());
    for chunk in flat.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new(
            "INSERT INTO catalog_product_option (product_id, type, title, is_require, price, price_type, sku, max_characters, sort_order) ",
        );
        qb.push_values(chunk, |mut b, (product_id, opt): &(u64, &CustomOption)| {
            b.push_bind(*product_id as u32)
                .push_bind(&opt.option_type)
                .push_bind(&opt.title)
                .push_bind(opt.is_require)
                .push_bind(opt.price)
                .push_bind(&opt.price_type)
                .push_bind(&opt.sku)
                .push_bind(opt.max_characters)
                .push_bind(opt.sort_order);
        });
        let result = qb.build().execute(&mut *tx).await?;
        let first_id = result.last_insert_id();
        option_ids.extend((0..chunk.len() as u64).map(|i| first_id + i));
    }

    let mut value_rows = Vec::new();
    for (i, (_, opt)) in flat.iter().enumerate() {
        let option_id = option_ids[i] as u32;
        for v in &opt.values {
            value_rows.push(ProductOptionTypeValue {
                option_type_id: 0,
                option_id,
                title: v.title.clone(),
                price: v.price,
                price_type: v.price_type.clone(),
                sku: v.sku.clone(),
                sort_order: v.sort_order,
            });
        }
    }
    for chunk in value_rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> =
            QueryBuilder::new("INSERT INTO catalog_product_option_type_value (option_id, title, price, price_type, sku, sort_order) ");
        qb.push_values(chunk, |mut b, v: &ProductOptionTypeValue| {
            b.push_bind(v.option_id).push_bind(&v.title).push_bind(v.price).push_bind(&v.price_type).push_bind(&v.sku).push_bind(v.sort_order);
        });
        qb.build().execute(&mut *tx).await?;
    }

    tx.commit().await
}

pub fn total_option_count(products: &[ProductCustomOptions]) -> usize {
    products.iter().map(|p| p.options.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn no_column_is_a_no_op() {
        let csv = parse("sku,name\nSKU-A,Widget\n");
        let (products, warnings) = collect_custom_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn simple_and_select_types_in_one_entry() {
        let csv = parse(
            "sku,custom_options\nSKU-A,\"field:Engraving:1:5.00:fixed:ENGRAVE:50;drop_down:Color:1:0:fixed::0:Red~5~fixed~RED|Blue~10~fixed~BLUE\"\n",
        );
        let (products, warnings) = collect_custom_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].options.len(), 2);

        let field = &products[0].options[0];
        assert_eq!(field.option_type, "field");
        assert_eq!(field.title, "Engraving");
        assert_eq!(field.is_require, 1);
        assert_eq!(field.price, 5.0);
        assert_eq!(field.sku.as_deref(), Some("ENGRAVE"));
        assert_eq!(field.max_characters, Some(50));

        let dropdown = &products[0].options[1];
        assert_eq!(dropdown.option_type, "drop_down");
        assert_eq!(dropdown.values.len(), 2);
        assert_eq!(dropdown.values[0].title, "Red");
        assert_eq!(dropdown.values[0].price, 5.0);
        assert_eq!(dropdown.values[1].title, "Blue");
    }

    #[test]
    fn unknown_type_warns_and_skips_entry() {
        let csv = parse("sku,custom_options\nSKU-A,not_a_real_type:Title:0:0:fixed::0\n");
        let (products, warnings) = collect_custom_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown custom option type"));
    }

    #[test]
    fn missing_title_warns_and_skips_entry() {
        let csv = parse("sku,custom_options\nSKU-A,\"field::0:0:fixed::0\"\n");
        let (products, warnings) = collect_custom_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no title"));
    }

    #[test]
    fn select_type_with_no_values_warns_and_skips() {
        let csv = parse("sku,custom_options\nSKU-A,drop_down:Color:0:0:fixed::0\n");
        let (products, warnings) = collect_custom_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no values"));
    }

    #[test]
    fn invalid_numeric_fields_warn_but_option_still_created() {
        let csv = parse("sku,custom_options\nSKU-A,\"field:Engraving:not-a-number:not-a-number:invalid-type:ENGRAVE:not-a-number\"\n");
        let (products, warnings) = collect_custom_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert_eq!(products.len(), 1, "option still created despite bad numeric fields");
        assert_eq!(warnings.len(), 4, "required, price, price_type, max_characters");
        let opt = &products[0].options[0];
        assert_eq!(opt.is_require, 0);
        assert_eq!(opt.price, 0.0);
        assert_eq!(opt.price_type, "fixed");
        assert_eq!(opt.max_characters, None);
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,custom_options\nSKU-A,field:Engraving:0:0:fixed::0\n");
        let (products, warnings) = collect_custom_options(&csv, &HashMap::new());
        assert!(products.is_empty());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn flush_replaces_fully_on_reimport() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-OPT-TEST-1'").execute(&pool).await.unwrap();
        let product_id = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-OPT-TEST-1')")
            .execute(&pool).await.unwrap().last_insert_id();

        let first = vec![ProductCustomOptions {
            product_id,
            options: vec![CustomOption {
                option_type: "field".into(), title: "Engraving".into(), is_require: 1, price: 5.0,
                price_type: "fixed".into(), sku: None, max_characters: None, sort_order: 0, values: vec![],
            }],
        }];
        flush_custom_options(&pool, &first, 500).await.unwrap();

        let second = vec![ProductCustomOptions {
            product_id,
            options: vec![CustomOption {
                option_type: "drop_down".into(), title: "Color".into(), is_require: 0, price: 0.0,
                price_type: "fixed".into(), sku: None, max_characters: None, sort_order: 0,
                values: vec![CustomOptionValue { title: "Red".into(), price: 0.0, price_type: "fixed".into(), sku: None, sort_order: 0 }],
            }],
        }];
        flush_custom_options(&pool, &second, 500).await.unwrap();

        let option_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_option WHERE product_id = ?")
            .bind(product_id as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(option_count, 1, "full replace, not accumulation");
        let title: String = sqlx::query_scalar("SELECT title FROM catalog_product_option WHERE product_id = ?")
            .bind(product_id as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(title, "Color");

        let orphaned_values: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM catalog_product_option_type_value WHERE option_id NOT IN (SELECT option_id FROM catalog_product_option)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphaned_values, 0, "old option's type values must be cleaned up");

        sqlx::query(
            "DELETE v FROM catalog_product_option_type_value v \
             JOIN catalog_product_option o ON o.option_id = v.option_id WHERE o.product_id = ?",
        )
        .bind(product_id as u32)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM catalog_product_option WHERE product_id = ?").bind(product_id as u32).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-OPT-TEST-1'").execute(&pool).await.unwrap();
    }
}
