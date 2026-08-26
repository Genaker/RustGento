use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashSet;

pub const OPTION_TYPE_FIELD: &str = "field";
pub const OPTION_TYPE_AREA: &str = "area";
pub const OPTION_TYPE_FILE: &str = "file";
pub const OPTION_TYPE_DATE: &str = "date";
pub const OPTION_TYPE_DATE_TIME: &str = "date_time";
pub const OPTION_TYPE_TIME: &str = "time";
pub const OPTION_TYPE_DROP_DOWN: &str = "drop_down";
pub const OPTION_TYPE_RADIO: &str = "radio";
pub const OPTION_TYPE_CHECKBOX: &str = "checkbox";
pub const OPTION_TYPE_MULTISELECT: &str = "multiselect";

/// The four option types that carry a list of [`ProductOptionTypeValue`]
/// choices rather than a free-form customer input.
pub fn is_select_option_type(t: &str) -> bool {
    matches!(
        t,
        OPTION_TYPE_DROP_DOWN | OPTION_TYPE_RADIO | OPTION_TYPE_CHECKBOX | OPTION_TYPE_MULTISELECT
    )
}

pub fn is_valid_option_type(t: &str) -> bool {
    valid_option_types().contains(t)
}

fn valid_option_types() -> HashSet<&'static str> {
    HashSet::from([
        OPTION_TYPE_FIELD,
        OPTION_TYPE_AREA,
        OPTION_TYPE_FILE,
        OPTION_TYPE_DATE,
        OPTION_TYPE_DATE_TIME,
        OPTION_TYPE_TIME,
        OPTION_TYPE_DROP_DOWN,
        OPTION_TYPE_RADIO,
        OPTION_TYPE_CHECKBOX,
        OPTION_TYPE_MULTISELECT,
    ])
}

/// Maps to `catalog_product_option`. Real Magento splits title into
/// `catalog_product_option_title` and price/price_type into
/// `catalog_product_option_price`, both per-store; this project simplifies
/// that to plain columns here (store_id=0 / "all stores" only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductOption {
    pub option_id: u64,
    pub product_id: u32,
    #[sqlx(rename = "type")]
    pub option_type: String,
    pub title: String,
    pub is_require: u16,
    pub price: f64,
    pub price_type: String,
    pub sku: Option<String>,
    pub max_characters: Option<i64>,
    pub sort_order: i64,
}

/// Maps to `catalog_product_option_type_value` -- one selectable choice
/// belonging to a select-type [`ProductOption`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductOptionTypeValue {
    pub option_type_id: u64,
    pub option_id: u32,
    pub title: String,
    pub price: f64,
    pub price_type: String,
    pub sku: Option<String>,
    pub sort_order: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_option_round_trip() {
        let o = ProductOption {
            option_id: 1,
            product_id: 5,
            option_type: OPTION_TYPE_FIELD.into(),
            title: "Engraving".into(),
            is_require: 1,
            price: 5.0,
            price_type: "fixed".into(),
            sku: Some("ENGRAVE".into()),
            max_characters: Some(50),
            sort_order: 0,
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<ProductOption>(&json).unwrap(), o);
    }

    #[test]
    fn product_option_type_value_round_trip() {
        let v = ProductOptionTypeValue {
            option_type_id: 1,
            option_id: 1,
            title: "Red".into(),
            price: 5.0,
            price_type: "fixed".into(),
            sku: None,
            sort_order: 0,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<ProductOptionTypeValue>(&json).unwrap(), v);
    }

    #[test]
    fn select_types_are_recognized() {
        assert!(is_select_option_type(OPTION_TYPE_DROP_DOWN));
        assert!(is_select_option_type(OPTION_TYPE_RADIO));
        assert!(is_select_option_type(OPTION_TYPE_CHECKBOX));
        assert!(is_select_option_type(OPTION_TYPE_MULTISELECT));
        assert!(!is_select_option_type(OPTION_TYPE_FIELD));
    }

    #[test]
    fn valid_types_accept_all_ten_and_reject_unknown() {
        for t in [
            OPTION_TYPE_FIELD,
            OPTION_TYPE_AREA,
            OPTION_TYPE_FILE,
            OPTION_TYPE_DATE,
            OPTION_TYPE_DATE_TIME,
            OPTION_TYPE_TIME,
            OPTION_TYPE_DROP_DOWN,
            OPTION_TYPE_RADIO,
            OPTION_TYPE_CHECKBOX,
            OPTION_TYPE_MULTISELECT,
        ] {
            assert!(is_valid_option_type(t));
        }
        assert!(!is_valid_option_type("not_a_type"));
    }
}
