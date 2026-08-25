use crate::error::ImportError;
use csv::ReaderBuilder;
use std::io::Read;

/// A parsed product-import CSV: header names, the resolved index of the
/// required `sku` column, and every data row as raw trimmed-on-access strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCsv {
    pub headers: Vec<String>,
    pub sku_col: usize,
    pub rows: Vec<Vec<String>>,
}

/// Parses CSV data. The first row must be a header row containing a `sku`
/// column (any other column not otherwise recognized later in the pipeline
/// is simply ignored with a warning -- this function itself only validates
/// structure, not column names).
pub fn parse_csv<R: Read>(reader: R) -> Result<ParsedCsv, ImportError> {
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(reader);

    let headers: Vec<String> = rdr
        .headers()?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();

    if headers.is_empty() {
        return Err(ImportError::EmptyCsv);
    }

    let sku_col = headers
        .iter()
        .position(|h| h == "sku")
        .ok_or(ImportError::MissingSkuColumn)?;

    let mut rows = Vec::new();
    for record in rdr.records() {
        let record = record?;
        rows.push(record.iter().map(|f| f.to_string()).collect());
    }

    Ok(ParsedCsv { headers, sku_col, rows })
}

impl ParsedCsv {
    /// Index of a header column by exact name, or `None` if absent.
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == name)
    }

    /// The trimmed `sku` value for a row, or `None` if blank/out of range.
    pub fn sku<'a>(&self, row: &'a [String]) -> Option<&'a str> {
        self.field(row, self.sku_col)
    }

    /// The trimmed value of `row[col]`, or `None` if out of range or blank
    /// after trimming -- matches Go's `strings.TrimSpace(row[ci]); v != ""` guard.
    pub fn field<'a>(&self, row: &'a [String], col: usize) -> Option<&'a str> {
        row.get(col)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(data: &str) -> Result<ParsedCsv, ImportError> {
        parse_csv(Cursor::new(data.as_bytes().to_vec()))
    }

    #[test]
    fn parses_headers_and_rows() {
        let csv = parse("sku,name,price\nSKU-1,Widget,9.99\nSKU-2,Gadget,19.99\n").unwrap();
        assert_eq!(csv.headers, vec!["sku", "name", "price"]);
        assert_eq!(csv.sku_col, 0);
        assert_eq!(csv.rows.len(), 2);
        assert_eq!(csv.rows[0], vec!["SKU-1", "Widget", "9.99"]);
    }

    #[test]
    fn sku_column_can_be_anywhere() {
        let csv = parse("name,sku\nWidget,SKU-1\n").unwrap();
        assert_eq!(csv.sku_col, 1);
    }

    #[test]
    fn missing_sku_column_is_an_error() {
        let err = parse("name,price\nWidget,9.99\n").unwrap_err();
        assert!(matches!(err, ImportError::MissingSkuColumn));
    }

    #[test]
    fn empty_input_is_an_error() {
        let err = parse("").unwrap_err();
        assert!(matches!(err, ImportError::EmptyCsv));
    }

    #[test]
    fn header_whitespace_is_trimmed() {
        let csv = parse(" sku , name \nSKU-1,Widget\n").unwrap();
        assert_eq!(csv.headers, vec!["sku", "name"]);
    }

    #[test]
    fn col_index_resolves_known_and_unknown_columns() {
        let csv = parse("sku,name\nSKU-1,Widget\n").unwrap();
        assert_eq!(csv.col_index("name"), Some(1));
        assert_eq!(csv.col_index("nonexistent"), None);
    }

    #[test]
    fn sku_returns_none_for_blank_value() {
        let csv = parse("sku,name\n ,Widget\n").unwrap();
        assert_eq!(csv.sku(&csv.rows[0]), None);
    }

    #[test]
    fn sku_returns_trimmed_value() {
        let csv = parse("sku,name\n SKU-1 ,Widget\n").unwrap();
        assert_eq!(csv.sku(&csv.rows[0]), Some("SKU-1"));
    }

    #[test]
    fn field_out_of_range_is_none() {
        let csv = parse("sku,name,price\nSKU-1\n").unwrap();
        assert_eq!(csv.field(&csv.rows[0], 2), None);
    }

    #[test]
    fn ragged_short_rows_are_tolerated() {
        // `flexible(true)` allows rows shorter than the header (e.g. trailing
        // optional columns omitted) rather than erroring.
        let csv = parse("sku,name,price\nSKU-1,Widget\n").unwrap();
        assert_eq!(csv.rows[0].len(), 2);
        assert_eq!(csv.field(&csv.rows[0], 2), None);
    }
}
