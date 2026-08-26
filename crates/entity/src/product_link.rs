use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Link type IDs, matching real Magento's
/// `Magento\Catalog\Model\Product\Link` constants.
pub const LINK_TYPE_RELATED: u16 = 1;
pub const LINK_TYPE_GROUPED: u16 = 3;
pub const LINK_TYPE_UPSELL: u16 = 4;
pub const LINK_TYPE_CROSSSELL: u16 = 5;

/// Maps to `catalog_product_link`, unique on
/// `(product_id, linked_product_id, link_type_id)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductLink {
    pub link_id: u64,
    pub product_id: u32,
    pub linked_product_id: u32,
    pub link_type_id: u16,
    /// Orders grouped/related/up-sell/cross-sell links for display. Real
    /// Magento stores this in a separate EAV-style
    /// `catalog_product_link_attribute_int` table keyed by `link_type_id`;
    /// this project simplifies that to a plain column.
    pub position: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_link_round_trip() {
        let l = ProductLink { link_id: 1, product_id: 5, linked_product_id: 6, link_type_id: LINK_TYPE_RELATED, position: 0 };
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<ProductLink>(&json).unwrap(), l);
    }

    #[test]
    fn link_type_constants() {
        assert_eq!(LINK_TYPE_RELATED, 1);
        assert_eq!(LINK_TYPE_GROUPED, 3);
        assert_eq!(LINK_TYPE_UPSELL, 4);
        assert_eq!(LINK_TYPE_CROSSSELL, 5);
    }
}
