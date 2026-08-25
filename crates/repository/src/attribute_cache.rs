use entity::EavAttribute;
use std::collections::HashMap;

/// Maps `attribute_id -> attribute_code`, same role as Go's process-global
/// `attributeCodeMap` (`model/repository/product/product_repository.go`).
/// Unlike the Go version this isn't a lazily-initialized global — callers
/// build one from a fetched `Vec<EavAttribute>` and hold onto it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttributeCodeMap {
    by_id: HashMap<u16, String>,
}

impl AttributeCodeMap {
    pub fn build(attrs: &[EavAttribute]) -> Self {
        let mut by_id = HashMap::with_capacity(attrs.len());
        for a in attrs {
            by_id.insert(a.attribute_id, a.attribute_code.clone());
        }
        AttributeCodeMap { by_id }
    }

    /// Resolves an attribute_id to its code, falling back to the numeric ID
    /// as a string when unknown — matching Go's fallback behavior in the
    /// flatten path so an unrecognized attribute doesn't silently vanish.
    pub fn code_for(&self, attribute_id: u16) -> String {
        self.by_id
            .get(&attribute_id)
            .cloned()
            .unwrap_or_else(|| attribute_id.to_string())
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(id: u16, code: &str) -> EavAttribute {
        EavAttribute {
            attribute_id: id,
            entity_type_id: 4,
            attribute_code: code.to_string(),
            attribute_model: None,
            backend_model: None,
            backend_type: "varchar".to_string(),
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
    fn resolves_known_attribute_code() {
        let map = AttributeCodeMap::build(&[attr(100, "name"), attr(101, "price")]);
        assert_eq!(map.code_for(100), "name");
        assert_eq!(map.code_for(101), "price");
        assert_eq!(map.len(), 2);
        assert!(!map.is_empty());
    }

    #[test]
    fn falls_back_to_numeric_id_for_unknown_attribute() {
        let map = AttributeCodeMap::build(&[attr(100, "name")]);
        assert_eq!(map.code_for(999), "999");
    }

    #[test]
    fn empty_map_falls_back_for_everything() {
        let map = AttributeCodeMap::build(&[]);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.code_for(1), "1");
    }

    #[test]
    fn default_map_is_empty() {
        let map = AttributeCodeMap::default();
        assert!(map.is_empty());
    }
}
