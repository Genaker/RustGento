use crate::csv_parse::ParsedCsv;
use crate::validate::parse_decimal_value;
use entity::{ProductIndexPrice, GUEST_CUSTOMER_GROUP_ID};
use std::collections::HashMap;

pub const PRICE_COLUMNS: [&str; 5] = ["price_index", "final_price", "min_price", "max_price", "tier_price"];

/// Fixed `website_id` this simplified price-index writer uses -- this
/// bypasses Magento's real price indexer entirely and is only a single
/// group/website index write.
pub const PRICE_WEBSITE_ID: u16 = 1;

fn default_index_price(entity_id: u64) -> ProductIndexPrice {
    ProductIndexPrice {
        entity_id,
        customer_group_id: GUEST_CUSTOMER_GROUP_ID,
        website_id: PRICE_WEBSITE_ID,
        tax_class_id: Some(0),
        price: Some(0.0),
        final_price: Some(0.0),
        min_price: Some(0.0),
        max_price: Some(0.0),
        tier_price: Some(0.0),
    }
}

/// Collects price-index upsert rows from CSV: a simplified, single-group/
/// website price index written directly from CSV columns, not a real
/// Magento price indexer run.
///
/// - A no-op if the header contains none of [`PRICE_COLUMNS`].
/// - `price_index`, if valid and non-empty, seeds `price`/`final_price`/
///   `min_price`/`max_price` all at once; `final_price` then overrides just
///   the final price. Either column alone is enough to emit a row;
///   `min_price`/`max_price`/`tier_price` alone are not.
/// - An invalid `price_index` value is a warning that abandons the rest of
///   the row. An invalid `final_price`/`min_price`/`max_price`/`tier_price`
///   is a warning that leaves that field at its default rather than
///   silently coercing the parse failure to zero -- a zero price on invalid
///   input would look like real data, not an error, so an invalid
///   `final_price` alone is not treated as sufficient to emit a row either.
pub fn collect_price(csv: &ParsedCsv, sku_to_id: &HashMap<String, u64>) -> (Vec<ProductIndexPrice>, Vec<String>) {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    if !PRICE_COLUMNS.iter().any(|c| csv.col_index(c).is_some()) {
        return (rows, warnings);
    }

    let price_index_col = csv.col_index("price_index");
    let final_price_col = csv.col_index("final_price");
    let min_price_col = csv.col_index("min_price");
    let max_price_col = csv.col_index("max_price");
    let tier_price_col = csv.col_index("tier_price");

    'rows: for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&entity_id) = sku_to_id.get(sku) else { continue };

        let mut item = default_index_price(entity_id);
        let mut populated = false;

        if let Some(col) = price_index_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => {
                        item.price = Some(v);
                        item.final_price = Some(v);
                        item.min_price = Some(v);
                        item.max_price = Some(v);
                        populated = true;
                    }
                    Err(msg) => {
                        warnings.push(format!("sku={sku}: {msg}"));
                        continue 'rows;
                    }
                }
            }
        }
        if let Some(col) = final_price_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => {
                        item.final_price = Some(v);
                        populated = true;
                    }
                    Err(msg) => warnings.push(format!("sku={sku}: {msg}")),
                }
            }
        }
        if let Some(col) = min_price_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => item.min_price = Some(v),
                    Err(msg) => warnings.push(format!("sku={sku}: {msg}")),
                }
            }
        }
        if let Some(col) = max_price_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => item.max_price = Some(v),
                    Err(msg) => warnings.push(format!("sku={sku}: {msg}")),
                }
            }
        }
        if let Some(col) = tier_price_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => item.tier_price = Some(v),
                    Err(msg) => warnings.push(format!("sku={sku}: {msg}")),
                }
            }
        }

        if populated {
            rows.push(item);
        }
    }

    (rows, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn no_price_columns_present_is_a_no_op() {
        let csv = parse("sku,name\nSKU-1,Widget\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn price_index_seeds_all_four_price_fields() {
        let csv = parse("sku,price_index\nSKU-1,19.99\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].price, Some(19.99));
        assert_eq!(rows[0].final_price, Some(19.99));
        assert_eq!(rows[0].min_price, Some(19.99));
        assert_eq!(rows[0].max_price, Some(19.99));
        assert_eq!(rows[0].customer_group_id, GUEST_CUSTOMER_GROUP_ID);
        assert_eq!(rows[0].website_id, PRICE_WEBSITE_ID);
    }

    #[test]
    fn final_price_overrides_price_index_seed() {
        let csv = parse("sku,price_index,final_price\nSKU-1,19.99,15.99\n");
        let (rows, _) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(rows[0].price, Some(19.99), "price_index still sets the base price");
        assert_eq!(rows[0].final_price, Some(15.99), "final_price overrides just the final price");
        assert_eq!(rows[0].min_price, Some(19.99));
    }

    #[test]
    fn final_price_alone_is_enough_to_emit_a_row() {
        let csv = parse("sku,final_price\nSKU-1,15.99\n");
        let (rows, _) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].final_price, Some(15.99));
    }

    #[test]
    fn min_max_tier_alone_do_not_emit_a_row() {
        let csv = parse("sku,min_price,max_price,tier_price\nSKU-1,5,10,3\n");
        let (rows, _) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty());
    }

    #[test]
    fn invalid_max_price_warns_but_row_from_price_index_still_emitted() {
        let csv = parse("sku,price_index,max_price\nSKU-1,19.99,not-a-number\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].max_price, Some(19.99), "left at price_index's seeded value, not overwritten with garbage");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_tier_price_warns_but_row_from_price_index_still_emitted() {
        let csv = parse("sku,price_index,tier_price\nSKU-1,19.99,not-a-number\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tier_price, Some(0.0), "left at default, not overwritten with garbage");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn blank_final_price_cell_leaves_price_index_seed_untouched() {
        // The column is present in the header, but this row's cell is blank:
        // exercises the `csv.field(...) == None` path distinctly from the
        // column being absent entirely.
        let csv = parse("sku,price_index,final_price\nSKU-1,19.99,\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].final_price, Some(19.99));
    }

    #[test]
    fn blank_price_index_cell_is_treated_as_absent() {
        let csv = parse("sku,price_index,final_price\nSKU-1,,15.99\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1, "final_price gate alone is enough");
        assert_eq!(rows[0].price, Some(0.0), "price_index never ran, so the base price stays at default");
    }

    #[test]
    fn blank_min_max_tier_price_cells_leave_defaults_untouched() {
        let csv = parse("sku,price_index,min_price,max_price,tier_price\nSKU-1,19.99,,,\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].min_price, Some(19.99), "left at price_index's seeded value");
        assert_eq!(rows[0].max_price, Some(19.99));
        assert_eq!(rows[0].tier_price, Some(0.0));
    }

    #[test]
    fn invalid_price_index_warns_and_abandons_the_row() {
        let csv = parse("sku,price_index,final_price\nSKU-1,not-a-number,15.99\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty(), "row abandoned when price_index is invalid, even though final_price is valid");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_final_price_warns_and_is_not_sufficient_alone_to_emit_a_row() {
        let csv = parse("sku,final_price\nSKU-1,not-a-number\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert!(rows.is_empty(), "an invalid final_price must not silently become a zero-priced row");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_min_price_warns_but_row_from_price_index_still_emitted() {
        let csv = parse("sku,price_index,min_price\nSKU-1,19.99,not-a-number\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::from([("SKU-1".to_string(), 1u64)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].min_price, Some(19.99), "left at price_index's seeded value, not overwritten with garbage");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,price_index\nUNKNOWN,19.99\n");
        let (rows, warnings) = collect_price(&csv, &HashMap::new());
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }
}
