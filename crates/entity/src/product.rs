use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Maps to the `catalog_product_entity` table.
/// CE schema only -- no `row_id` (Enterprise Edition) column; EE support is
/// out of scope for this project (see the top-level README).
///
/// Field types below match the ACTUAL live schema (`DESCRIBE
/// catalog_product_entity`), which can be wider or differently-signed than
/// a quick read of a typical ORM's model definitions would suggest (e.g.
/// `entity_id` is `bigint unsigned`, and `has_options` is a signed
/// `smallint` rather than unsigned). Getting this wrong doesn't surface
/// until you actually decode a row with `sqlx::query_as` -- binding into an
/// INSERT tolerates width mismatches, but strict decode does not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Product {
    pub entity_id: u64,
    pub attribute_set_id: u16,
    pub type_id: String,
    pub sku: String,
    pub has_options: i16,
    pub required_options: u16,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

macro_rules! eav_value_table {
    ($name:ident, $value_ty:ty) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
        pub struct $name {
            pub value_id: u64,
            pub attribute_id: u16,
            pub store_id: u16,
            pub entity_id: u64,
            pub value: Option<$value_ty>,
        }
    };
}

eav_value_table!(ProductVarchar, String);
eav_value_table!(ProductInt, i64);
eav_value_table!(ProductDecimal, f64);
eav_value_table!(ProductText, String);
eav_value_table!(ProductDatetime, NaiveDateTime);

/// Maps to the `cataloginventory_stock_item` table. Only the
/// subset of columns this port actually reads/writes is modeled; sqlx's
/// derived `FromRow` looks up fields by name and ignores columns not
/// declared here, so `SELECT *` against the wider real table is fine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct StockItem {
    pub item_id: u64,
    pub product_id: u64,
    pub stock_id: u16,
    pub qty: Option<f64>,
    pub min_qty: f64,
    pub is_qty_decimal: u16,
    pub backorders: u16,
    pub min_sale_qty: f64,
    pub max_sale_qty: f64,
    pub is_in_stock: u16,
    pub manage_stock: u16,
    pub website_id: u16,
}

/// Maps to the `catalog_product_index_price` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductIndexPrice {
    pub entity_id: u64,
    pub customer_group_id: u64,
    pub website_id: u16,
    pub tax_class_id: Option<u16>,
    pub price: Option<f64>,
    pub final_price: Option<f64>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub tier_price: Option<f64>,
}

/// `customer_group_id` used for guest/not-logged-in pricing throughout Magento.
pub const GUEST_CUSTOMER_GROUP_ID: u64 = 0;

/// Fixed `stock_id` used by single-source (non-MSI) stock rows, matching the
/// import pipeline's hardcoded value.
pub const DEFAULT_STOCK_ID: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn product_serializes_round_trip() {
        let p = Product {
            entity_id: 1,
            attribute_set_id: 4,
            type_id: "simple".into(),
            sku: "SKU-1".into(),
            has_options: 0,
            required_options: 0,
            created_at: now(),
            updated_at: now(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Product = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn eav_value_tables_round_trip() {
        let v = ProductVarchar { value_id: 1, attribute_id: 100, store_id: 0, entity_id: 5, value: Some("hello".into()) };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<ProductVarchar>(&json).unwrap(), v);

        let i = ProductInt { value_id: 2, attribute_id: 101, store_id: 0, entity_id: 5, value: Some(42) };
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(serde_json::from_str::<ProductInt>(&json).unwrap(), i);

        let d = ProductDecimal { value_id: 3, attribute_id: 102, store_id: 0, entity_id: 5, value: Some(9.99) };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<ProductDecimal>(&json).unwrap(), d);
    }

    #[test]
    fn eav_value_can_be_null() {
        let v = ProductVarchar { value_id: 1, attribute_id: 100, store_id: 0, entity_id: 5, value: None };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<ProductVarchar>(&json).unwrap(), v);
    }

    #[test]
    fn guest_customer_group_and_default_stock_id_constants() {
        assert_eq!(GUEST_CUSTOMER_GROUP_ID, 0);
        assert_eq!(DEFAULT_STOCK_ID, 1);
    }

    #[test]
    fn stock_item_round_trip() {
        let s = StockItem {
            item_id: 1,
            product_id: 5,
            stock_id: DEFAULT_STOCK_ID,
            qty: Some(100.0),
            min_qty: 0.0,
            is_qty_decimal: 0,
            backorders: 0,
            min_sale_qty: 1.0,
            max_sale_qty: 0.0,
            is_in_stock: 1,
            manage_stock: 0,
            website_id: 0,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<StockItem>(&json).unwrap(), s);
    }

    #[test]
    fn product_index_price_round_trip() {
        let p = ProductIndexPrice {
            entity_id: 5,
            customer_group_id: GUEST_CUSTOMER_GROUP_ID,
            website_id: 1,
            tax_class_id: Some(0),
            price: Some(9.99),
            final_price: Some(9.99),
            min_price: Some(9.99),
            max_price: Some(9.99),
            tier_price: Some(0.0),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<ProductIndexPrice>(&json).unwrap(), p);
    }
}
