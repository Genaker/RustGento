use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Mirrors Go's `model/entity/product.Product` (table `catalog_product_entity`).
/// CE schema only — no `row_id` (Enterprise Edition) column, by design (see plan
/// non-goals: EE support is out of scope for this port).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Product {
    pub entity_id: u32,
    pub attribute_set_id: u16,
    pub type_id: String,
    pub sku: String,
    pub has_options: u16,
    pub required_options: u16,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

macro_rules! eav_value_table {
    ($name:ident, $value_ty:ty) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
        pub struct $name {
            pub value_id: u32,
            pub attribute_id: u16,
            pub store_id: u16,
            pub entity_id: u32,
            pub value: $value_ty,
        }
    };
}

eav_value_table!(ProductVarchar, String);
eav_value_table!(ProductInt, i32);
eav_value_table!(ProductDecimal, f64);
eav_value_table!(ProductText, String);
eav_value_table!(ProductDatetime, NaiveDateTime);

/// Mirrors Go's `StockItem` (table `cataloginventory_stock_item`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct StockItem {
    pub item_id: u32,
    pub product_id: u32,
    pub stock_id: u16,
    pub qty: f64,
    pub min_qty: f64,
    pub is_qty_decimal: u16,
    pub backorders: u16,
    pub min_sale_qty: f64,
    pub max_sale_qty: f64,
    pub is_in_stock: u16,
    pub manage_stock: u16,
    pub website_id: u16,
}

/// Mirrors Go's `ProductIndexPrice` (table `catalog_product_index_price`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductIndexPrice {
    pub entity_id: u32,
    pub customer_group_id: u32,
    pub website_id: u16,
    pub tax_class_id: u16,
    pub price: f64,
    pub final_price: f64,
    pub min_price: f64,
    pub max_price: f64,
    pub tier_price: f64,
}

/// `customer_group_id` used for guest/not-logged-in pricing throughout Magento.
pub const GUEST_CUSTOMER_GROUP_ID: u32 = 0;

/// Fixed `stock_id` used by single-source (non-MSI) stock rows, matching the
/// Go import service's hardcoded value.
pub const DEFAULT_STOCK_ID: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
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
        let v = ProductVarchar { value_id: 1, attribute_id: 100, store_id: 0, entity_id: 5, value: "hello".into() };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<ProductVarchar>(&json).unwrap(), v);

        let i = ProductInt { value_id: 2, attribute_id: 101, store_id: 0, entity_id: 5, value: 42 };
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(serde_json::from_str::<ProductInt>(&json).unwrap(), i);

        let d = ProductDecimal { value_id: 3, attribute_id: 102, store_id: 0, entity_id: 5, value: 9.99 };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<ProductDecimal>(&json).unwrap(), d);
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
            qty: 100.0,
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
            tax_class_id: 0,
            price: 9.99,
            final_price: 9.99,
            min_price: 9.99,
            max_price: 9.99,
            tier_price: 0.0,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<ProductIndexPrice>(&json).unwrap(), p);
    }
}
