use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Maps to the `catalog_category_entity` table.
/// See `product.rs`'s doc comment for why field widths/nullability here
/// follow the live `DESCRIBE` output rather than a naive reading of a typical
/// ORM's model definitions: `position`/`level`/`children_count` are 64-bit
/// despite reading like small numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct Category {
    pub entity_id: u64,
    pub attribute_set_id: u16,
    pub parent_id: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub path: String,
    pub position: i64,
    pub level: i64,
    pub children_count: i64,
}

macro_rules! category_value_table {
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

category_value_table!(CategoryInt, i64);
category_value_table!(CategoryVarchar, String);
category_value_table!(CategoryText, String);

/// Maps to the `catalog_category_product` join table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct CategoryProduct {
    pub entity_id: u64,
    pub category_id: u64,
    pub product_id: u32,
    pub position: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn category_round_trip() {
        let c = Category {
            entity_id: 2,
            attribute_set_id: 3,
            parent_id: 1,
            created_at: now(),
            updated_at: now(),
            path: "1/2".into(),
            position: 1,
            level: 1,
            children_count: 0,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Category>(&json).unwrap(), c);
    }

    #[test]
    fn category_value_tables_round_trip() {
        let v = CategoryVarchar { value_id: 1, attribute_id: 40, store_id: 0, entity_id: 2, value: Some("Shirts".into()) };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<CategoryVarchar>(&json).unwrap(), v);

        let i = CategoryInt { value_id: 2, attribute_id: 41, store_id: 0, entity_id: 2, value: Some(1) };
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(serde_json::from_str::<CategoryInt>(&json).unwrap(), i);
    }

    #[test]
    fn category_product_round_trip() {
        let cp = CategoryProduct { entity_id: 1, category_id: 2, product_id: 5, position: 0 };
        let json = serde_json::to_string(&cp).unwrap();
        assert_eq!(serde_json::from_str::<CategoryProduct>(&json).unwrap(), cp);
    }
}
