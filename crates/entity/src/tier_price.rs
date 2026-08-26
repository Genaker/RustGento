use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Maps to `catalog_product_entity_tier_price`. `entity_id`/`row_id` are
/// nullable in the live schema even though this project's CE-only writer
/// always populates `entity_id` -- matching the actual `DESCRIBE` output
/// rather than what a bare CE-schema reading would suggest.
///
/// There is no separate "group price" mechanism in Magento: a qty=1 tier
/// for one specific customer group *is* a group price, so this one table
/// covers both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct TierPrice {
    pub value_id: u64,
    pub entity_id: Option<u64>,
    pub row_id: Option<u64>,
    pub all_groups: u8,
    pub customer_group_id: u16,
    pub qty: f64,
    pub value: f64,
    pub website_id: u16,
    pub percentage_value: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_price_round_trip() {
        let t = TierPrice {
            value_id: 1,
            entity_id: Some(5),
            row_id: None,
            all_groups: 1,
            customer_group_id: 0,
            qty: 5.0,
            value: 8.99,
            website_id: 1,
            percentage_value: None,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<TierPrice>(&json).unwrap(), t);
    }
}
