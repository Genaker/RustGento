use crate::csv_parse::ParsedCsv;
use crate::validate::parse_decimal_value;
use entity::{StockItem, DEFAULT_STOCK_ID};
use std::collections::HashMap;

pub const STOCK_COLUMNS: [&str; 6] =
    ["qty", "is_in_stock", "manage_stock", "min_qty", "max_sale_qty", "min_sale_qty"];

fn default_stock_item(product_id: u32) -> StockItem {
    StockItem {
        item_id: 0, // unset; DB assigns on insert
        product_id,
        stock_id: DEFAULT_STOCK_ID,
        qty: 0.0,
        min_qty: 0.0,
        is_qty_decimal: 0,
        backorders: 0,
        min_sale_qty: 0.0,
        max_sale_qty: 0.0,
        is_in_stock: 1,
        manage_stock: 1,
        website_id: 0,
    }
}

/// Collects stock upsert rows from CSV, matching Go's `collectStock`
/// (`service/product/import_stock.go`):
///
/// - A no-op if the header contains none of [`STOCK_COLUMNS`].
/// - A row only produces a stock item if `qty` or `is_in_stock` has a valid,
///   non-empty value -- the other stock columns alone don't cause a row to
///   be emitted (matches Go's `populated` gate).
/// - An invalid `qty` or `is_in_stock` value is a warning that abandons the
///   rest of that row's stock fields, same as Go: qty/in-stock-ness are
///   safety-relevant, so a half-applied stock update is worse than none.
/// - An invalid `manage_stock`/`min_qty`/`min_sale_qty`/`max_sale_qty` value
///   is a warning that leaves that one field at its default -- **unlike**
///   Go, which silently coerces a parse failure on these columns to zero
///   (`fv, _ := strconv.ParseFloat(...)`, discarding the error). Silently
///   writing 0 for an unparseable inventory quantity looks like an oversight
///   rather than intended behavior, so this port warns instead.
pub fn collect_stock(csv: &ParsedCsv, sku_to_id: &HashMap<String, u32>) -> (Vec<StockItem>, Vec<String>) {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    if !STOCK_COLUMNS.iter().any(|c| csv.col_index(c).is_some()) {
        return (rows, warnings);
    }

    let qty_col = csv.col_index("qty");
    let in_stock_col = csv.col_index("is_in_stock");
    let manage_stock_col = csv.col_index("manage_stock");
    let min_qty_col = csv.col_index("min_qty");
    let min_sale_qty_col = csv.col_index("min_sale_qty");
    let max_sale_qty_col = csv.col_index("max_sale_qty");

    'rows: for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&product_id) = sku_to_id.get(sku) else { continue };

        let mut item = default_stock_item(product_id);
        let mut populated = false;

        if let Some(col) = qty_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => {
                        item.qty = v;
                        populated = true;
                    }
                    Err(msg) => {
                        warnings.push(format!("sku={sku}: {msg}"));
                        continue 'rows;
                    }
                }
            }
        }
        if let Some(col) = in_stock_col {
            if let Some(raw) = csv.field(row, col) {
                match raw.parse::<u16>() {
                    Ok(v) => {
                        item.is_in_stock = v;
                        populated = true;
                    }
                    Err(_) => {
                        warnings.push(format!("sku={sku}: invalid is_in_stock {raw:?}"));
                        continue 'rows;
                    }
                }
            }
        }
        if let Some(col) = manage_stock_col {
            if let Some(raw) = csv.field(row, col) {
                match raw.parse::<u16>() {
                    Ok(v) => item.manage_stock = v,
                    Err(_) => warnings.push(format!("sku={sku}: invalid manage_stock {raw:?}")),
                }
            }
        }
        if let Some(col) = min_qty_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => item.min_qty = v,
                    Err(msg) => warnings.push(format!("sku={sku}: {msg}")),
                }
            }
        }
        if let Some(col) = min_sale_qty_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => item.min_sale_qty = v,
                    Err(msg) => warnings.push(format!("sku={sku}: {msg}")),
                }
            }
        }
        if let Some(col) = max_sale_qty_col {
            if let Some(raw) = csv.field(row, col) {
                match parse_decimal_value(raw) {
                    Ok(v) => item.max_sale_qty = v,
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
    fn no_stock_columns_present_is_a_no_op() {
        let csv = parse("sku,name\nSKU-1,Widget\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn valid_qty_produces_a_row_with_stock_defaults() {
        let csv = parse("sku,qty\nSKU-1,50\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].qty, 50.0);
        assert_eq!(rows[0].is_in_stock, 1, "default is_in_stock when not provided");
        assert_eq!(rows[0].stock_id, DEFAULT_STOCK_ID);
    }

    #[test]
    fn valid_is_in_stock_alone_also_produces_a_row() {
        let csv = parse("sku,is_in_stock\nSKU-1,0\n");
        let (rows, _) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].is_in_stock, 0);
    }

    #[test]
    fn min_qty_alone_does_not_produce_a_row() {
        // min_qty is not a "gate" column -- populated stays false.
        let csv = parse("sku,min_qty\nSKU-1,5\n");
        let (rows, _) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(rows.is_empty());
    }

    #[test]
    fn blank_qty_cell_is_skipped_like_an_absent_column() {
        let csv = parse("sku,qty,is_in_stock\nSKU-1,,1\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1, "is_in_stock gate alone is enough");
        assert_eq!(rows[0].qty, 0.0, "left at default since the qty cell was blank");
    }

    #[test]
    fn blank_is_in_stock_cell_is_treated_as_absent() {
        let csv = parse("sku,qty,is_in_stock\nSKU-1,10,\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1, "qty gate alone is enough");
        assert_eq!(rows[0].is_in_stock, 1, "left at default since the cell was blank");
    }

    #[test]
    fn blank_manage_stock_min_qty_min_sale_qty_max_sale_qty_cells_leave_defaults_untouched() {
        let csv = parse("sku,qty,manage_stock,min_qty,min_sale_qty,max_sale_qty\nSKU-1,10,,,,\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(warnings.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manage_stock, 1);
        assert_eq!(rows[0].min_qty, 0.0);
        assert_eq!(rows[0].min_sale_qty, 0.0);
        assert_eq!(rows[0].max_sale_qty, 0.0);
    }

    #[test]
    fn invalid_is_in_stock_warns_and_abandons_the_row() {
        let csv = parse("sku,is_in_stock,manage_stock\nSKU-1,not-a-number,1\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(rows.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("invalid is_in_stock"));
    }

    #[test]
    fn manage_stock_min_sale_qty_and_max_sale_qty_are_applied_when_valid() {
        let csv = parse("sku,qty,manage_stock,min_sale_qty,max_sale_qty\nSKU-1,10,0,2,100\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(warnings.is_empty());
        assert_eq!(rows[0].manage_stock, 0);
        assert_eq!(rows[0].min_sale_qty, 2.0);
        assert_eq!(rows[0].max_sale_qty, 100.0);
    }

    #[test]
    fn invalid_manage_stock_warns_but_row_is_still_emitted() {
        let csv = parse("sku,qty,manage_stock\nSKU-1,10,not-a-number\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].manage_stock, 1, "left at default rather than silently coerced to zero");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_min_sale_qty_warns_but_row_is_still_emitted() {
        let csv = parse("sku,qty,min_sale_qty\nSKU-1,10,not-a-number\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].min_sale_qty, 0.0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_max_sale_qty_warns_but_row_is_still_emitted() {
        let csv = parse("sku,qty,max_sale_qty\nSKU-1,10,not-a-number\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].max_sale_qty, 0.0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn invalid_qty_warns_and_abandons_the_row() {
        let csv = parse("sku,qty,is_in_stock\nSKU-1,not-a-number,1\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert!(rows.is_empty(), "row should be abandoned when qty is invalid, even though is_in_stock is valid");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("sku=SKU-1"));
    }

    #[test]
    fn invalid_min_qty_warns_but_row_is_still_emitted() {
        let csv = parse("sku,qty,min_qty\nSKU-1,10,not-a-number\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::from([("SKU-1".to_string(), 1u32)]));
        assert_eq!(rows.len(), 1, "qty gate alone is enough to emit a row");
        assert_eq!(rows[0].min_qty, 0.0, "invalid min_qty left at default rather than silently set");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,qty\nUNKNOWN,10\n");
        let (rows, warnings) = collect_stock(&csv, &HashMap::new());
        assert!(rows.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn blank_sku_is_skipped() {
        let csv = parse("sku,qty\n,10\n");
        let (rows, _) = collect_stock(&csv, &HashMap::from([("".to_string(), 1u32)]));
        assert!(rows.is_empty());
    }

    #[test]
    fn multiple_rows_are_independent() {
        let csv = parse("sku,qty\nSKU-1,10\nSKU-2,20\n");
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u32), ("SKU-2".to_string(), 2u32)]);
        let (rows, _) = collect_stock(&csv, &sku_to_id);
        assert_eq!(rows.len(), 2);
    }
}
