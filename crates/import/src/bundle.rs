use crate::csv_parse::ParsedCsv;
use entity::{is_valid_bundle_option_type, ProductBundleSelection};
use sqlx::{MySql, MySqlPool, QueryBuilder};
use std::collections::HashMap;

pub const BUNDLE_OPTIONS_COLUMN: &str = "bundle_options";

#[derive(Debug, Clone, PartialEq)]
pub struct BundleSelection {
    pub product_id: u64,
    pub qty: f64,
    pub price_value: f64,
    pub price_type: String,
    pub is_default: u16,
    pub can_change_qty: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BundleOption {
    pub option_type: String,
    pub title: String,
    pub required: u16,
    pub selections: Vec<BundleSelection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParentBundleOptions {
    pub product_id: u64,
    pub options: Vec<BundleOption>,
}

/// Returns every distinct component SKU referenced by a selection across
/// the "bundle_options" column, so the caller can fold them into the same
/// batch SKU lookup used for product links -- a bundle component must
/// already exist, it isn't created from this column.
pub fn bundle_selection_skus(csv: &ParsedCsv) -> Vec<String> {
    let Some(col) = csv.col_index(BUNDLE_OPTIONS_COLUMN) else { return Vec::new() };
    let mut skus = Vec::new();
    for row in &csv.rows {
        let Some(val) = csv.field(row, col) else { continue };
        for entry in val.split(';') {
            let fields: Vec<&str> = entry.trim().splitn(4, ':').collect();
            if fields.len() < 4 {
                continue;
            }
            for sel in fields[3].split('|') {
                let parts: Vec<&str> = sel.trim().splitn(2, '~').collect();
                let sku = parts[0].trim();
                if !sku.is_empty() {
                    skus.push(sku.to_string());
                }
            }
        }
    }
    skus
}

/// Collects the "bundle_options" column:
///
///   type:title:required:selections
///
/// entries separated by ";", where "selections" is a "|"-separated list of
/// "sku~qty~price_value~price_type~is_default~can_change_qty" component
/// entries.
///
/// Example: "select:CPU:1:Intel i5~1~0~fixed~1~0|Intel i7~1~50~fixed~0~1"
pub fn collect_bundle_options(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> (Vec<ParentBundleOptions>, Vec<String>) {
    let mut products = Vec::new();
    let mut warnings = Vec::new();

    let Some(col) = csv.col_index(BUNDLE_OPTIONS_COLUMN) else { return (products, warnings) };

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
            let (opt, entry_warnings) = parse_bundle_option_entry(sku, entry, sku_to_id);
            warnings.extend(entry_warnings);
            if let Some(opt) = opt {
                options.push(opt);
            }
        }
        if !options.is_empty() {
            products.push(ParentBundleOptions { product_id, options });
        }
    }

    (products, warnings)
}

fn parse_bundle_option_entry(sku: &str, entry: &str, sku_to_id: &HashMap<String, u64>) -> (Option<BundleOption>, Vec<String>) {
    let mut warnings = Vec::new();
    let fields: Vec<&str> = entry.splitn(4, ':').collect();
    if fields.len() < 2 {
        warnings.push(format!("sku={sku}: malformed bundle option entry {entry:?}, want at least type:title"));
        return (None, warnings);
    }

    let option_type = fields[0].trim();
    if !is_valid_bundle_option_type(option_type) {
        warnings.push(format!("sku={sku}: unknown bundle option type {option_type:?} in entry {entry:?}"));
        return (None, warnings);
    }
    let title = fields[1].trim();
    if title.is_empty() {
        warnings.push(format!("sku={sku}: bundle option entry {entry:?} has no title"));
        return (None, warnings);
    }

    let mut opt = BundleOption { option_type: option_type.to_string(), title: title.to_string(), required: 0, selections: Vec::new() };
    if let Some(f) = fields.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<u16>() {
            Ok(v) => opt.required = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid required flag {f:?} in bundle option {title:?}")),
        }
    }

    let selections_field = fields.get(3).map(|s| s.trim()).filter(|s| !s.is_empty());
    let Some(selections_field) = selections_field else {
        warnings.push(format!("sku={sku}: bundle option {title:?} has no selections"));
        return (None, warnings);
    };
    for sel in selections_field.split('|') {
        let sel = sel.trim();
        if sel.is_empty() {
            continue;
        }
        let (s, sel_warnings) = parse_bundle_selection(sku, title, sel, sku_to_id);
        warnings.extend(sel_warnings);
        if let Some(s) = s {
            opt.selections.push(s);
        }
    }
    if opt.selections.is_empty() {
        warnings.push(format!("sku={sku}: bundle option {title:?} has no valid selections"));
        return (None, warnings);
    }

    (Some(opt), warnings)
}

fn parse_bundle_selection(sku: &str, option_title: &str, entry: &str, sku_to_id: &HashMap<String, u64>) -> (Option<BundleSelection>, Vec<String>) {
    let mut warnings = Vec::new();
    let parts: Vec<&str> = entry.split('~').collect();
    let component_sku = parts[0].trim();
    if component_sku.is_empty() {
        warnings.push(format!("sku={sku}: bundle option {option_title:?} has a selection with no SKU, skipping"));
        return (None, warnings);
    }
    let Some(&component_id) = sku_to_id.get(component_sku) else {
        warnings.push(format!("sku={sku}: bundle option {option_title:?} references unknown SKU {component_sku:?}, skipping"));
        return (None, warnings);
    };

    let mut s = BundleSelection { product_id: component_id, qty: 1.0, price_value: 0.0, price_type: "fixed".to_string(), is_default: 0, can_change_qty: 1 };
    if let Some(f) = parts.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<f64>() {
            Ok(v) => s.qty = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid qty {f:?} for {component_sku:?} in bundle option {option_title:?}")),
        }
    }
    if let Some(f) = parts.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<f64>() {
            Ok(v) => s.price_value = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid price_value {f:?} for {component_sku:?} in bundle option {option_title:?}")),
        }
    }
    if let Some(f) = parts.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if f == "fixed" || f == "percent" {
            s.price_type = f.to_string();
        } else {
            warnings.push(format!("sku={sku}: invalid price_type {f:?} for {component_sku:?} in bundle option {option_title:?}, want fixed or percent"));
        }
    }
    if let Some(f) = parts.get(4).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<u16>() {
            Ok(v) => s.is_default = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid is_default {f:?} for {component_sku:?} in bundle option {option_title:?}")),
        }
    }
    if let Some(f) = parts.get(5).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match f.parse::<u16>() {
            Ok(v) => s.can_change_qty = v,
            Err(_) => warnings.push(format!("sku={sku}: invalid can_change_qty {f:?} for {component_sku:?} in bundle option {option_title:?}")),
        }
    }

    (Some(s), warnings)
}

/// Replaces each affected bundle product's full option/selection tree --
/// the same full-replace-on-reimport approach as custom options and
/// downloadable links.
pub async fn flush_bundle_options(pool: &MySqlPool, products: &[ParentBundleOptions], batch_size: usize) -> Result<(), sqlx::Error> {
    if products.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;

    let parent_ids: Vec<u64> = products.iter().map(|p| p.product_id).collect();
    let placeholders = vec!["?"; parent_ids.len()].join(",");

    let existing_option_ids: Vec<u64> = {
        let sql = format!("SELECT option_id FROM catalog_product_bundle_option WHERE parent_id IN ({placeholders})");
        let mut q = sqlx::query_scalar(&sql);
        for id in &parent_ids {
            q = q.bind(*id as u32);
        }
        q.fetch_all(&mut *tx).await?
    };
    if !existing_option_ids.is_empty() {
        let ph = vec!["?"; existing_option_ids.len()].join(",");
        let sql = format!("DELETE FROM catalog_product_bundle_selection WHERE option_id IN ({ph})");
        let mut q = sqlx::query(&sql);
        for id in &existing_option_ids {
            q = q.bind(*id as u32);
        }
        q.execute(&mut *tx).await?;
    }
    {
        let sql = format!("DELETE FROM catalog_product_bundle_option WHERE parent_id IN ({placeholders})");
        let mut q = sqlx::query(&sql);
        for id in &parent_ids {
            q = q.bind(*id as u32);
        }
        q.execute(&mut *tx).await?;
    }

    // Flatten every option across every product into one list, same
    // batched-insert-then-backfilled-ID approach as flush_custom_options:
    // chunked multi-row INSERTs instead of one INSERT per option.
    let flat: Vec<(u64, i64, &BundleOption)> = products
        .iter()
        .flat_map(|p| p.options.iter().enumerate().map(move |(pos, opt)| (p.product_id, pos as i64, opt)))
        .collect();

    let mut option_ids = Vec::with_capacity(flat.len());
    for chunk in flat.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new("INSERT INTO catalog_product_bundle_option (parent_id, required, position, type, title) ");
        qb.push_values(chunk, |mut b, (parent_id, position, opt): &(u64, i64, &BundleOption)| {
            b.push_bind(*parent_id as u32).push_bind(opt.required).push_bind(*position).push_bind(&opt.option_type).push_bind(&opt.title);
        });
        let result = qb.build().execute(&mut *tx).await?;
        let first_id = result.last_insert_id();
        option_ids.extend((0..chunk.len() as u64).map(|i| first_id + i));
    }

    let mut selection_rows = Vec::new();
    for (i, (parent_id, _, opt)) in flat.iter().enumerate() {
        let option_id = option_ids[i] as u32;
        for (sel_pos, sel) in opt.selections.iter().enumerate() {
            selection_rows.push(ProductBundleSelection {
                selection_id: 0,
                option_id,
                parent_product_id: *parent_id as u32,
                product_id: sel.product_id as u32,
                position: sel_pos as i64,
                is_default: sel.is_default,
                selection_qty: sel.qty,
                selection_price_value: sel.price_value,
                selection_price_type: sel.price_type.clone(),
                selection_can_change_qty: sel.can_change_qty,
            });
        }
    }
    for chunk in selection_rows.chunks(batch_size.max(1)) {
        let mut qb: QueryBuilder<MySql> = QueryBuilder::new(
            "INSERT INTO catalog_product_bundle_selection \
             (option_id, parent_product_id, product_id, position, is_default, selection_qty, selection_price_value, selection_price_type, selection_can_change_qty) ",
        );
        qb.push_values(chunk, |mut b, s: &ProductBundleSelection| {
            b.push_bind(s.option_id)
                .push_bind(s.parent_product_id)
                .push_bind(s.product_id)
                .push_bind(s.position)
                .push_bind(s.is_default)
                .push_bind(s.selection_qty)
                .push_bind(s.selection_price_value)
                .push_bind(&s.selection_price_type)
                .push_bind(s.selection_can_change_qty);
        });
        qb.build().execute(&mut *tx).await?;
    }

    tx.commit().await
}

pub fn option_count(products: &[ParentBundleOptions]) -> usize {
    products.iter().map(|p| p.options.len()).sum()
}

pub fn selection_count(products: &[ParentBundleOptions]) -> usize {
    products.iter().flat_map(|p| &p.options).map(|o| o.selections.len()).sum()
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
        let (products, warnings) = collect_bundle_options(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn options_and_selections() {
        let sku_to_id = HashMap::from([("BUNDLE-1".to_string(), 1u64), ("CPU-A".to_string(), 2u64), ("CPU-B".to_string(), 3u64), ("MOUSE".to_string(), 4u64)]);
        let csv = parse(
            "sku,bundle_options\n\
             BUNDLE-1,\"select:CPU:1:CPU-A~1~0~fixed~1~0|CPU-B~1~50~fixed~0~1;checkbox:Extras:0:MOUSE~2~10~fixed~0~1\"\n",
        );
        let (products, warnings) = collect_bundle_options(&csv, &sku_to_id);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].options.len(), 2);
        assert_eq!(option_count(&products), 2);
        assert_eq!(selection_count(&products), 3);

        let cpu = &products[0].options[0];
        assert_eq!(cpu.option_type, "select");
        assert_eq!(cpu.required, 1);
        assert_eq!(cpu.selections.len(), 2);
        assert_eq!(cpu.selections[0].product_id, 2);
        assert_eq!(cpu.selections[0].is_default, 1);
        assert_eq!(cpu.selections[1].price_value, 50.0);

        let extras = &products[0].options[1];
        assert_eq!(extras.option_type, "checkbox");
        assert_eq!(extras.selections[0].product_id, 4);
        assert_eq!(extras.selections[0].qty, 2.0);
    }

    #[test]
    fn unknown_component_sku_warns_and_drops_selection() {
        let csv = parse("sku,bundle_options\nBUNDLE-1,select:CPU:1:DOES-NOT-EXIST~1~0~fixed~1~0\n");
        let (products, warnings) = collect_bundle_options(&csv, &HashMap::from([("BUNDLE-1".to_string(), 1u64)]));
        assert!(products.is_empty(), "no valid selections means the whole option is dropped too");
        assert_eq!(warnings.len(), 2, "unknown SKU + no valid selections");
        assert!(warnings[0].contains("unknown SKU"));
    }

    #[test]
    fn unknown_type_warns_and_skips() {
        let csv = parse("sku,bundle_options\nBUNDLE-1,not_a_type:CPU:1:X~1~0~fixed~1~0\n");
        let (products, warnings) = collect_bundle_options(&csv, &HashMap::from([("BUNDLE-1".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown bundle option type"));
    }

    #[test]
    fn no_selections_warns_and_skips() {
        let csv = parse("sku,bundle_options\nBUNDLE-1,select:CPU:1:\n");
        let (products, warnings) = collect_bundle_options(&csv, &HashMap::from([("BUNDLE-1".to_string(), 1u64)]));
        assert!(products.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no selections"));
    }

    #[test]
    fn bundle_selection_skus_collects_every_referenced_sku() {
        let csv = parse("sku,bundle_options\nBUNDLE-1,select:CPU:1:CPU-A~1~0~fixed~1~0|CPU-B~1~0~fixed~0~1\n");
        let mut skus = bundle_selection_skus(&csv);
        skus.sort();
        assert_eq!(skus, vec!["CPU-A", "CPU-B"]);
    }

    #[tokio::test]
    async fn flush_replaces_fully_on_reimport() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-BUNDLE-1', 'RUST-BUNDLE-COMPONENT')").execute(&pool).await.unwrap();
        let component = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'simple', 'RUST-BUNDLE-COMPONENT')")
            .execute(&pool).await.unwrap().last_insert_id();
        let bundle_id = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'bundle', 'RUST-BUNDLE-1')")
            .execute(&pool).await.unwrap().last_insert_id();

        let first = vec![ParentBundleOptions {
            product_id: bundle_id,
            options: vec![BundleOption {
                option_type: "select".into(), title: "Choice".into(), required: 1,
                selections: vec![BundleSelection { product_id: component, qty: 1.0, price_value: 0.0, price_type: "fixed".into(), is_default: 1, can_change_qty: 0 }],
            }],
        }];
        flush_bundle_options(&pool, &first, 500).await.unwrap();

        let second = vec![ParentBundleOptions {
            product_id: bundle_id,
            options: vec![BundleOption {
                option_type: "checkbox".into(), title: "NewChoice".into(), required: 0,
                selections: vec![BundleSelection { product_id: component, qty: 1.0, price_value: 5.0, price_type: "fixed".into(), is_default: 0, can_change_qty: 1 }],
            }],
        }];
        flush_bundle_options(&pool, &second, 500).await.unwrap();

        let option_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_bundle_option WHERE parent_id = ?").bind(bundle_id as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(option_count, 1, "full replace, not accumulation");
        let title: String = sqlx::query_scalar("SELECT title FROM catalog_product_bundle_option WHERE parent_id = ?").bind(bundle_id as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(title, "NewChoice");
        let orphaned: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM catalog_product_bundle_selection WHERE option_id NOT IN (SELECT option_id FROM catalog_product_bundle_option)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphaned, 0, "old option's selections must be cleaned up");

        sqlx::query(
            "DELETE s FROM catalog_product_bundle_selection s \
             JOIN catalog_product_bundle_option o ON o.option_id = s.option_id WHERE o.parent_id = ?",
        )
        .bind(bundle_id as u32)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM catalog_product_bundle_option WHERE parent_id = ?").bind(bundle_id as u32).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku IN ('RUST-BUNDLE-1', 'RUST-BUNDLE-COMPONENT')").execute(&pool).await.unwrap();
    }
}
