use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Maps to `catalog_product_super_attribute` -- one EAV attribute (e.g.
/// "color", "size") that varies across a configurable product's child
/// (simple) products. `product_id` is the configurable parent. Real
/// Magento has a separate `catalog_product_super_attribute_label` table
/// for a per-store label override; this project omits it and would use
/// the attribute's own `eav_attribute.frontend_label` instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductSuperAttribute {
    pub product_super_attribute_id: u64,
    pub product_id: u32,
    pub attribute_id: u16,
    pub position: i64,
}

/// Maps to `catalog_product_super_link` -- links a simple (child) product
/// to its owning configurable (parent) product. `product_id` is unique
/// alone, matching real Magento: a simple product can be a child of at
/// most one configurable product.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductSuperLink {
    pub link_id: u64,
    pub product_id: u32,
    pub parent_id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_super_attribute_round_trip() {
        let a = ProductSuperAttribute { product_super_attribute_id: 1, product_id: 5, attribute_id: 92, position: 0 };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<ProductSuperAttribute>(&json).unwrap(), a);
    }

    #[test]
    fn product_super_link_round_trip() {
        let l = ProductSuperLink { link_id: 1, product_id: 6, parent_id: 5 };
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<ProductSuperLink>(&json).unwrap(), l);
    }
}
