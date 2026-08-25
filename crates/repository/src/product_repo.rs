use crate::attribute_cache::AttributeCodeMap;
use entity::{Product, ProductDatetime, ProductDecimal, ProductIndexPrice, ProductInt, ProductText, ProductVarchar, StockItem};
use serde_json::{json, Map, Value};

const DATETIME_FMT: &str = "%Y-%m-%d %H:%M:%S";

/// All EAV value rows for a single product, one `Vec` per backend-type table —
/// mirrors the GORM `Preload`ed slices on Go's `Product` struct.
#[derive(Debug, Clone, Default)]
pub struct ProductEavRows {
    pub varchar: Vec<ProductVarchar>,
    pub int: Vec<ProductInt>,
    pub decimal: Vec<ProductDecimal>,
    pub text: Vec<ProductText>,
    pub datetime: Vec<ProductDatetime>,
}

/// Flattens a product's static fields + EAV values into one `attribute_code -> value`
/// map, matching Go's `FlattenProductAttributesWithCodes` in
/// `model/repository/product/product_repository.go`:
///
/// 1. Seed static keys: entity_id, sku, type_id, created_at, updated_at.
/// 2. Overlay varchar -> int -> decimal -> text -> datetime, in that fixed order.
///    If two tables define the same attribute_code (attribute_id collision across
///    types), the later table in this order wins — replicated as-is, not "fixed",
///    since it's Go's actual observable behavior.
/// 3. Add category_ids, stock_item (nested object, if present), index_prices
///    (array), and media_gallery (always empty for now — gallery import/flatten
///    is an explicit non-goal for this port, the field is present for schema
///    shape parity, not populated).
pub fn flatten_product(
    product: &Product,
    rows: &ProductEavRows,
    code_map: &AttributeCodeMap,
    category_ids: &[u32],
    stock_item: Option<&StockItem>,
    index_prices: &[ProductIndexPrice],
) -> Map<String, Value> {
    let mut out = Map::new();

    out.insert("entity_id".to_string(), json!(product.entity_id));
    out.insert("sku".to_string(), json!(product.sku));
    out.insert("type_id".to_string(), json!(product.type_id));
    out.insert(
        "created_at".to_string(),
        json!(product.created_at.format(DATETIME_FMT).to_string()),
    );
    out.insert(
        "updated_at".to_string(),
        json!(product.updated_at.format(DATETIME_FMT).to_string()),
    );

    for v in &rows.varchar {
        out.insert(code_map.code_for(v.attribute_id), json!(v.value));
    }
    for v in &rows.int {
        out.insert(code_map.code_for(v.attribute_id), json!(v.value));
    }
    for v in &rows.decimal {
        out.insert(code_map.code_for(v.attribute_id), json!(v.value));
    }
    for v in &rows.text {
        out.insert(code_map.code_for(v.attribute_id), json!(v.value));
    }
    for v in &rows.datetime {
        out.insert(
            code_map.code_for(v.attribute_id),
            json!(v.value.format(DATETIME_FMT).to_string()),
        );
    }

    out.insert("category_ids".to_string(), json!(category_ids));
    out.insert("media_gallery".to_string(), json!(Vec::<Value>::new()));

    if let Some(stock) = stock_item {
        out.insert(
            "stock_item".to_string(),
            json!({
                "qty": stock.qty,
                "is_in_stock": stock.is_in_stock,
                "manage_stock": stock.manage_stock,
                "min_qty": stock.min_qty,
                "min_sale_qty": stock.min_sale_qty,
                "max_sale_qty": stock.max_sale_qty,
            }),
        );
    }

    let prices: Vec<Value> = index_prices
        .iter()
        .map(|p| {
            json!({
                "customer_group_id": p.customer_group_id,
                "website_id": p.website_id,
                "price": p.price,
                "final_price": p.final_price,
                "min_price": p.min_price,
                "max_price": p.max_price,
                "tier_price": p.tier_price,
            })
        })
        .collect();
    out.insert("index_prices".to_string(), json!(prices));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn product() -> Product {
        Product {
            entity_id: 42,
            attribute_set_id: 4,
            type_id: "simple".into(),
            sku: "SAMPLE-SKU-0042".into(),
            has_options: 0,
            required_options: 0,
            created_at: dt("2026-01-01 10:00:00"),
            updated_at: dt("2026-01-02 11:00:00"),
        }
    }

    fn code_map() -> AttributeCodeMap {
        AttributeCodeMap::build(&[
            entity::EavAttribute {
                attribute_id: 100,
                entity_type_id: 4,
                attribute_code: "name".into(),
                attribute_model: None,
                backend_model: None,
                backend_type: "varchar".into(),
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
            },
            entity::EavAttribute {
                attribute_id: 200,
                entity_type_id: 4,
                attribute_code: "price".into(),
                attribute_model: None,
                backend_model: None,
                backend_type: "decimal".into(),
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
            },
        ])
    }

    #[test]
    fn seeds_static_fields() {
        let flat = flatten_product(&product(), &ProductEavRows::default(), &code_map(), &[], None, &[]);
        assert_eq!(flat["entity_id"], json!(42));
        assert_eq!(flat["sku"], json!("SAMPLE-SKU-0042"));
        assert_eq!(flat["type_id"], json!("simple"));
        assert_eq!(flat["created_at"], json!("2026-01-01 10:00:00"));
        assert_eq!(flat["updated_at"], json!("2026-01-02 11:00:00"));
    }

    #[test]
    fn overlays_varchar_and_decimal_by_attribute_code() {
        let rows = ProductEavRows {
            varchar: vec![entity::ProductVarchar { value_id: 1, attribute_id: 100, store_id: 0, entity_id: 42, value: "Widget".into() }],
            decimal: vec![entity::ProductDecimal { value_id: 2, attribute_id: 200, store_id: 0, entity_id: 42, value: 19.99 }],
            ..Default::default()
        };
        let flat = flatten_product(&product(), &rows, &code_map(), &[], None, &[]);
        assert_eq!(flat["name"], json!("Widget"));
        assert_eq!(flat["price"], json!(19.99));
    }

    #[test]
    fn overlays_text_and_datetime_by_attribute_code() {
        let mut map = code_map();
        map = crate::attribute_cache::AttributeCodeMap::build(&[
            entity::EavAttribute {
                attribute_id: 300, entity_type_id: 4, attribute_code: "description".into(),
                attribute_model: None, backend_model: None, backend_type: "text".into(), backend_table: None,
                frontend_model: None, frontend_input: None, frontend_label: None, frontend_class: None,
                source_model: None, is_required: 0, is_user_defined: 1, default_value: None, is_unique: 0, note: None,
            },
            entity::EavAttribute {
                attribute_id: 400, entity_type_id: 4, attribute_code: "special_from_date".into(),
                attribute_model: None, backend_model: None, backend_type: "datetime".into(), backend_table: None,
                frontend_model: None, frontend_input: None, frontend_label: None, frontend_class: None,
                source_model: None, is_required: 0, is_user_defined: 1, default_value: None, is_unique: 0, note: None,
            },
        ]);
        let rows = ProductEavRows {
            text: vec![entity::ProductText { value_id: 1, attribute_id: 300, store_id: 0, entity_id: 42, value: "A nice widget".into() }],
            datetime: vec![entity::ProductDatetime { value_id: 2, attribute_id: 400, store_id: 0, entity_id: 42, value: dt("2026-03-05 00:00:00") }],
            ..Default::default()
        };
        let flat = flatten_product(&product(), &rows, &map, &[], None, &[]);
        assert_eq!(flat["description"], json!("A nice widget"));
        assert_eq!(flat["special_from_date"], json!("2026-03-05 00:00:00"));
    }

    #[test]
    fn later_backend_type_wins_on_attribute_id_collision() {
        // Same attribute_id (100, coded "name") present in both varchar and int
        // tables — Go's overlay order (varchar -> int -> decimal -> text -> datetime)
        // means int's value should win since it's applied after varchar.
        let rows = ProductEavRows {
            varchar: vec![entity::ProductVarchar { value_id: 1, attribute_id: 100, store_id: 0, entity_id: 42, value: "from varchar".into() }],
            int: vec![entity::ProductInt { value_id: 2, attribute_id: 100, store_id: 0, entity_id: 42, value: 7 }],
            ..Default::default()
        };
        let flat = flatten_product(&product(), &rows, &code_map(), &[], None, &[]);
        assert_eq!(flat["name"], json!(7));
    }

    #[test]
    fn unknown_attribute_id_falls_back_to_numeric_key() {
        let rows = ProductEavRows {
            varchar: vec![entity::ProductVarchar { value_id: 1, attribute_id: 999, store_id: 0, entity_id: 42, value: "mystery".into() }],
            ..Default::default()
        };
        let flat = flatten_product(&product(), &rows, &code_map(), &[], None, &[]);
        assert_eq!(flat["999"], json!("mystery"));
    }

    #[test]
    fn includes_category_ids_and_empty_media_gallery() {
        let flat = flatten_product(&product(), &ProductEavRows::default(), &code_map(), &[1, 2, 3], None, &[]);
        assert_eq!(flat["category_ids"], json!([1, 2, 3]));
        assert_eq!(flat["media_gallery"], json!(Vec::<Value>::new()));
    }

    #[test]
    fn stock_item_absent_when_none() {
        let flat = flatten_product(&product(), &ProductEavRows::default(), &code_map(), &[], None, &[]);
        assert!(!flat.contains_key("stock_item"));
    }

    #[test]
    fn stock_item_present_when_given() {
        let stock = StockItem {
            item_id: 1, product_id: 42, stock_id: 1, qty: 50.0, min_qty: 0.0,
            is_qty_decimal: 0, backorders: 0, min_sale_qty: 1.0, max_sale_qty: 0.0,
            is_in_stock: 1, manage_stock: 0, website_id: 0,
        };
        let flat = flatten_product(&product(), &ProductEavRows::default(), &code_map(), &[], Some(&stock), &[]);
        assert_eq!(flat["stock_item"]["qty"], json!(50.0));
        assert_eq!(flat["stock_item"]["is_in_stock"], json!(1));
    }

    #[test]
    fn index_prices_flattened_as_array() {
        let price = ProductIndexPrice {
            entity_id: 42, customer_group_id: 0, website_id: 1, tax_class_id: 0,
            price: 19.99, final_price: 15.99, min_price: 15.99, max_price: 19.99, tier_price: 0.0,
        };
        let flat = flatten_product(&product(), &ProductEavRows::default(), &code_map(), &[], None, &[price]);
        let prices = flat["index_prices"].as_array().unwrap();
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0]["customer_group_id"], json!(0));
        assert_eq!(prices[0]["final_price"], json!(15.99));
    }

    #[test]
    fn empty_index_prices_is_empty_array() {
        let flat = flatten_product(&product(), &ProductEavRows::default(), &code_map(), &[], None, &[]);
        assert_eq!(flat["index_prices"], json!(Vec::<Value>::new()));
    }
}
