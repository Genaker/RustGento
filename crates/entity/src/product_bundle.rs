use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashSet;

pub const BUNDLE_OPTION_SELECT: &str = "select";
pub const BUNDLE_OPTION_RADIO: &str = "radio";
pub const BUNDLE_OPTION_CHECKBOX: &str = "checkbox";
pub const BUNDLE_OPTION_MULTI: &str = "multi";

pub fn is_valid_bundle_option_type(t: &str) -> bool {
    valid_bundle_option_types().contains(t)
}

fn valid_bundle_option_types() -> HashSet<&'static str> {
    HashSet::from([BUNDLE_OPTION_SELECT, BUNDLE_OPTION_RADIO, BUNDLE_OPTION_CHECKBOX, BUNDLE_OPTION_MULTI])
}

/// Maps to `catalog_product_bundle_option` -- one choice group (e.g. "CPU",
/// "Accessories") on a bundle-type product. `parent_id` is the bundle
/// product this option group belongs to. Real Magento splits title into a
/// per-store `catalog_product_bundle_option_value` table; this project
/// simplifies that to a plain column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductBundleOption {
    pub option_id: u64,
    pub parent_id: u32,
    pub required: u16,
    pub position: i64,
    #[sqlx(rename = "type")]
    pub option_type: String,
    pub title: String,
}

/// Maps to `catalog_product_bundle_selection` -- one selectable component
/// product within a [`ProductBundleOption`] group. `parent_product_id` is
/// the bundle product; `product_id` is the selectable component product.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductBundleSelection {
    pub selection_id: u64,
    pub option_id: u32,
    pub parent_product_id: u32,
    pub product_id: u32,
    pub position: i64,
    pub is_default: u16,
    pub selection_qty: f64,
    pub selection_price_value: f64,
    pub selection_price_type: String,
    pub selection_can_change_qty: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_bundle_option_round_trip() {
        let o = ProductBundleOption {
            option_id: 1,
            parent_id: 5,
            required: 1,
            position: 0,
            option_type: BUNDLE_OPTION_SELECT.into(),
            title: "CPU".into(),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(serde_json::from_str::<ProductBundleOption>(&json).unwrap(), o);
    }

    #[test]
    fn product_bundle_selection_round_trip() {
        let s = ProductBundleSelection {
            selection_id: 1,
            option_id: 1,
            parent_product_id: 5,
            product_id: 6,
            position: 0,
            is_default: 1,
            selection_qty: 1.0,
            selection_price_value: 0.0,
            selection_price_type: "fixed".into(),
            selection_can_change_qty: 0,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<ProductBundleSelection>(&json).unwrap(), s);
    }

    #[test]
    fn valid_bundle_option_types_accept_all_four_and_reject_unknown() {
        for t in [BUNDLE_OPTION_SELECT, BUNDLE_OPTION_RADIO, BUNDLE_OPTION_CHECKBOX, BUNDLE_OPTION_MULTI] {
            assert!(is_valid_bundle_option_type(t));
        }
        assert!(!is_valid_bundle_option_type("not_a_type"));
    }
}
