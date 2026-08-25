use crate::attributes::{AttributeMeta, AttributesByCode};
use crate::csv_parse::ParsedCsv;
use crate::validate::{parse_datetime_value, parse_decimal_value, parse_int_value};
use chrono::NaiveDateTime;
use entity::BackendType;
use std::collections::HashMap;

/// One resolved EAV value row, generic over the value's Rust type (`String`
/// for varchar/text, `i32` for int, `f64` for decimal, `NaiveDateTime` for
/// datetime) -- mirrors the shape common to all five
/// `catalog_product_entity_*` tables.
#[derive(Debug, Clone, PartialEq)]
pub struct EavValue<T> {
    pub entity_id: u64,
    pub attribute_id: u16,
    pub store_id: u16,
    pub value: T,
}

/// EAV values collected from one CSV import run, bucketed by backend type --
/// mirrors the per-table slices Go's import service flushes concurrently.
#[derive(Debug, Clone, Default)]
pub struct BucketedEav {
    pub varchar: Vec<EavValue<String>>,
    pub int: Vec<EavValue<i32>>,
    pub decimal: Vec<EavValue<f64>>,
    pub text: Vec<EavValue<String>>,
    pub datetime: Vec<EavValue<NaiveDateTime>>,
}

impl BucketedEav {
    pub fn total_len(&self) -> usize {
        self.varchar.len() + self.int.len() + self.decimal.len() + self.text.len() + self.datetime.len()
    }
}

/// Buckets every recognized attribute-value CSV column by backend type, for
/// every row whose `sku` resolves to a known product.
///
/// Unlike Go's price/stock collectors (which abandon a whole row on the
/// first invalid value), an invalid attribute value here only skips that one
/// cell: it's recorded as a warning, and the row's other attributes are still
/// imported. Discarding an otherwise-valid product's other 12 attributes
/// because one datetime was malformed would be a worse outcome than a
/// per-cell warning, and there's no Go precedent to preserve here since this
/// bucketing has no single-column "gate" semantics the way stock/price do.
pub fn bucket_rows(
    csv: &ParsedCsv,
    sku_to_id: &HashMap<String, u64>,
    attrs: &AttributesByCode,
    store_id: u16,
) -> (BucketedEav, Vec<String>) {
    let mut out = BucketedEav::default();
    let mut warnings = Vec::new();

    // Resolve (column index -> attribute) once, not per row.
    let known_cols: Vec<(usize, &str, AttributeMeta)> = csv
        .headers
        .iter()
        .enumerate()
        .filter_map(|(i, h)| attrs.get(h).map(|meta| (i, h.as_str(), *meta)))
        .collect();

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&entity_id) = sku_to_id.get(sku) else { continue };

        for &(col, code, meta) in &known_cols {
            let Some(raw) = csv.field(row, col) else { continue };

            match meta.backend_type {
                BackendType::Varchar => out.varchar.push(EavValue {
                    entity_id,
                    attribute_id: meta.id,
                    store_id,
                    value: raw.to_string(),
                }),
                BackendType::Text => out.text.push(EavValue {
                    entity_id,
                    attribute_id: meta.id,
                    store_id,
                    value: raw.to_string(),
                }),
                BackendType::Int => match parse_int_value(raw) {
                    Ok(value) => out.int.push(EavValue { entity_id, attribute_id: meta.id, store_id, value }),
                    Err(msg) => warnings.push(format!("sku={sku} attribute={code}: {msg}")),
                },
                BackendType::Decimal => match parse_decimal_value(raw) {
                    Ok(value) => out.decimal.push(EavValue { entity_id, attribute_id: meta.id, store_id, value }),
                    Err(msg) => warnings.push(format!("sku={sku} attribute={code}: {msg}")),
                },
                BackendType::Datetime => match parse_datetime_value(raw) {
                    Ok(value) => out.datetime.push(EavValue { entity_id, attribute_id: meta.id, store_id, value }),
                    Err(msg) => warnings.push(format!("sku={sku} attribute={code}: {msg}")),
                },
            }
        }
    }

    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use entity::EavAttribute;

    fn attr(id: u16, code: &str, backend_type: &str) -> EavAttribute {
        EavAttribute {
            attribute_id: id,
            entity_type_id: 4,
            attribute_code: code.to_string(),
            attribute_model: None,
            backend_model: None,
            backend_type: backend_type.to_string(),
            backend_table: None,
            frontend_model: None,
            frontend_input: None,
            frontend_label: None,
            frontend_class: None,
            source_model: None,
            is_required: 0,
            is_user_defined: 1,
            default_value: None,
            is_unique: 0,
            note: None,
        }
    }

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn buckets_all_five_backend_types_by_attribute_code() {
        let csv = parse("sku,name,color,price,description,special_from_date\nSKU-1,Widget,7,9.99,A nice widget,2026-01-01 00:00:00\n");
        let attrs = AttributesByCode::build(&[
            attr(1, "name", "varchar"),
            attr(2, "color", "int"),
            attr(3, "price", "decimal"),
            attr(4, "description", "text"),
            attr(5, "special_from_date", "datetime"),
        ]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 42u64)]);

        let (bucketed, warnings) = bucket_rows(&csv, &sku_to_id, &attrs, 0);

        assert!(warnings.is_empty());
        assert_eq!(bucketed.varchar, vec![EavValue { entity_id: 42, attribute_id: 1, store_id: 0, value: "Widget".into() }]);
        assert_eq!(bucketed.int, vec![EavValue { entity_id: 42, attribute_id: 2, store_id: 0, value: 7 }]);
        assert_eq!(bucketed.decimal, vec![EavValue { entity_id: 42, attribute_id: 3, store_id: 0, value: 9.99 }]);
        assert_eq!(bucketed.text, vec![EavValue { entity_id: 42, attribute_id: 4, store_id: 0, value: "A nice widget".into() }]);
        assert_eq!(bucketed.datetime.len(), 1);
        assert_eq!(bucketed.total_len(), 5);
    }

    #[test]
    fn unknown_sku_is_skipped_entirely() {
        let csv = parse("sku,name\nUNKNOWN-SKU,Widget\n");
        let attrs = AttributesByCode::build(&[attr(1, "name", "varchar")]);
        let (bucketed, warnings) = bucket_rows(&csv, &HashMap::new(), &attrs, 0);
        assert_eq!(bucketed.total_len(), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_column_is_ignored() {
        let csv = parse("sku,mystery_column\nSKU-1,whatever\n");
        let attrs = AttributesByCode::build(&[]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u64)]);
        let (bucketed, warnings) = bucket_rows(&csv, &sku_to_id, &attrs, 0);
        assert_eq!(bucketed.total_len(), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn blank_value_is_skipped_without_warning() {
        let csv = parse("sku,name\nSKU-1,\n");
        let attrs = AttributesByCode::build(&[attr(1, "name", "varchar")]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u64)]);
        let (bucketed, warnings) = bucket_rows(&csv, &sku_to_id, &attrs, 0);
        assert_eq!(bucketed.total_len(), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn invalid_int_produces_warning_but_does_not_abort_other_columns() {
        let csv = parse("sku,name,color\nSKU-1,Widget,not-a-number\n");
        let attrs = AttributesByCode::build(&[attr(1, "name", "varchar"), attr(2, "color", "int")]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u64)]);
        let (bucketed, warnings) = bucket_rows(&csv, &sku_to_id, &attrs, 0);
        assert_eq!(bucketed.varchar.len(), 1, "the valid varchar column should still be imported");
        assert_eq!(bucketed.int.len(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("sku=SKU-1"));
        assert!(warnings[0].contains("attribute=color"));
    }

    #[test]
    fn invalid_decimal_and_datetime_each_produce_a_warning() {
        let csv = parse("sku,price,special_from_date\nSKU-1,not-a-price,not-a-date\n");
        let attrs = AttributesByCode::build(&[attr(1, "price", "decimal"), attr(2, "special_from_date", "datetime")]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u64)]);
        let (bucketed, warnings) = bucket_rows(&csv, &sku_to_id, &attrs, 0);
        assert_eq!(bucketed.total_len(), 0);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn multiple_rows_are_independent() {
        let csv = parse("sku,name\nSKU-1,Widget\nSKU-2,Gadget\n");
        let attrs = AttributesByCode::build(&[attr(1, "name", "varchar")]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u64), ("SKU-2".to_string(), 2u64)]);
        let (bucketed, _) = bucket_rows(&csv, &sku_to_id, &attrs, 0);
        assert_eq!(bucketed.varchar.len(), 2);
    }

    #[test]
    fn store_id_is_stamped_onto_every_value() {
        let csv = parse("sku,name\nSKU-1,Widget\n");
        let attrs = AttributesByCode::build(&[attr(1, "name", "varchar")]);
        let sku_to_id = HashMap::from([("SKU-1".to_string(), 1u64)]);
        let (bucketed, _) = bucket_rows(&csv, &sku_to_id, &attrs, 7);
        assert_eq!(bucketed.varchar[0].store_id, 7);
    }
}
