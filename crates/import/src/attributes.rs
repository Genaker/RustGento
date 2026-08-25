use entity::{BackendType, EavAttribute};
use std::collections::HashMap;

/// The resolved (attribute_id, backend_type) for one attribute code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributeMeta {
    pub id: u16,
    pub backend_type: BackendType,
}

/// `attribute_code -> AttributeMeta` for the EAV attributes an import run
/// cares about (loaded from `eav_attribute WHERE entity_type_id = 4`).
/// Attributes with an unrecognized `backend_type` (e.g. Magento's "static"
/// pseudo-type, used for computed/non-EAV attributes like `sku`) are
/// silently excluded, matching Go's `knownColumns` treatment of
/// `backend_type != "static"`.
#[derive(Debug, Clone, Default)]
pub struct AttributesByCode(HashMap<String, AttributeMeta>);

impl AttributesByCode {
    pub fn build(attrs: &[EavAttribute]) -> Self {
        let mut map = HashMap::with_capacity(attrs.len());
        for a in attrs {
            if let Some(backend_type) = BackendType::parse(&a.backend_type) {
                map.insert(a.attribute_code.clone(), AttributeMeta { id: a.attribute_id, backend_type });
            }
        }
        AttributesByCode(map)
    }

    pub fn get(&self, code: &str) -> Option<&AttributeMeta> {
        self.0.get(code)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(id: u16, code: &str, backend_type: &str) -> EavAttribute {
        EavAttribute {
            attribute_id: id,
            entity_type_id: 4,
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
    fn builds_lookup_by_attribute_code() {
        let attrs = AttributesByCode::build(&[attr(100, "name", "varchar"), attr(200, "price", "decimal")]);
        assert_eq!(attrs.get("name"), Some(&AttributeMeta { id: 100, backend_type: BackendType::Varchar }));
        assert_eq!(attrs.get("price"), Some(&AttributeMeta { id: 200, backend_type: BackendType::Decimal }));
        assert_eq!(attrs.len(), 2);
    }

    #[test]
    fn unknown_backend_type_is_excluded() {
        let attrs = AttributesByCode::build(&[attr(1, "sku", "static")]);
        assert_eq!(attrs.get("sku"), None);
        assert!(attrs.is_empty());
    }

    #[test]
    fn get_returns_none_for_missing_code() {
        let attrs = AttributesByCode::build(&[attr(100, "name", "varchar")]);
        assert_eq!(attrs.get("nonexistent"), None);
    }

    #[test]
    fn default_is_empty() {
        assert!(AttributesByCode::default().is_empty());
        assert_eq!(AttributesByCode::default().len(), 0);
    }
}
