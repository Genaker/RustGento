use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Mirrors Go's `model/entity.EavAttribute` (table `eav_attribute`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct EavAttribute {
    pub attribute_id: u16,
    pub entity_type_id: u16,
    pub attribute_code: String,
    pub attribute_model: Option<String>,
    pub backend_model: Option<String>,
    pub backend_type: String,
    pub backend_table: Option<String>,
    pub frontend_model: Option<String>,
    pub frontend_input: Option<String>,
    pub frontend_label: Option<String>,
    pub frontend_class: Option<String>,
    pub source_model: Option<String>,
    pub is_required: u16,
    pub is_user_defined: u16,
    pub default_value: Option<String>,
    pub is_unique: u16,
    pub note: Option<String>,
}

/// `entity_type_id` for the `catalog_product` EAV entity type in a standard
/// Magento install (fixed by Magento's `eav_entity_type` seed data).
pub const PRODUCT_ENTITY_TYPE_ID: u16 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(code: &str, backend_type: &str) -> EavAttribute {
        EavAttribute {
            attribute_id: 1,
            entity_type_id: PRODUCT_ENTITY_TYPE_ID,
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

    #[test]
    fn serializes_round_trip_through_json() {
        let attr = sample("name", "varchar");
        let json = serde_json::to_string(&attr).expect("serialize");
        let back: EavAttribute = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(attr, back);
    }

    #[test]
    fn product_entity_type_id_is_four() {
        assert_eq!(PRODUCT_ENTITY_TYPE_ID, 4);
    }
}
