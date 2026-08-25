use entity::{Category, CategoryInt, CategoryText, CategoryVarchar, EavAttribute};
use serde_json::{json, Value};
use std::collections::HashMap;

/// `attribute_id -> (code, label)` for category EAV attributes. A separate
/// type from `AttributeCodeMap` because category flatten output includes a
/// human-readable label per attribute (Go's `FlattenCategoryAttributesWithLabels`),
/// which product flatten does not.
#[derive(Debug, Clone, Default)]
pub struct CategoryAttributeMeta {
    by_id: HashMap<u16, (String, String)>,
}

impl CategoryAttributeMeta {
    pub fn build(attrs: &[EavAttribute]) -> Self {
        let mut by_id = HashMap::with_capacity(attrs.len());
        for a in attrs {
            let label = a.frontend_label.clone().unwrap_or_else(|| a.attribute_code.clone());
            by_id.insert(a.attribute_id, (a.attribute_code.clone(), label));
        }
        CategoryAttributeMeta { by_id }
    }

    fn resolve(&self, attribute_id: u16) -> (String, String) {
        self.by_id
            .get(&attribute_id)
            .cloned()
            .unwrap_or_else(|| (attribute_id.to_string(), attribute_id.to_string()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CategoryEavRows {
    pub int: Vec<CategoryInt>,
    pub varchar: Vec<CategoryVarchar>,
    pub text: Vec<CategoryText>,
}

/// Flattens a category's EAV values into `attribute_code -> {value, label, store_id}`,
/// matching Go's `FlattenCategoryAttributesWithLabels`. Categories have no
/// decimal/datetime attribute tables in Magento, so only int/varchar/text are
/// overlaid (in that order — int -> varchar -> text; unlike products, Go's
/// category flatten doesn't process varchar first, since `int` typically holds
/// structural attributes like `is_active`/`include_in_menu` that are seeded
/// before the more numerous varchar/text label attributes).
pub fn flatten_category(
    category: &Category,
    rows: &CategoryEavRows,
    meta: &CategoryAttributeMeta,
) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();

    out.insert("entity_id".to_string(), json!(category.entity_id));
    out.insert("parent_id".to_string(), json!(category.parent_id));
    out.insert("path".to_string(), json!(category.path));
    out.insert("level".to_string(), json!(category.level));

    for v in &rows.int {
        let (code, label) = meta.resolve(v.attribute_id);
        out.insert(code, json!({ "value": v.value, "label": label, "store_id": v.store_id }));
    }
    for v in &rows.varchar {
        let (code, label) = meta.resolve(v.attribute_id);
        out.insert(code, json!({ "value": v.value, "label": label, "store_id": v.store_id }));
    }
    for v in &rows.text {
        let (code, label) = meta.resolve(v.attribute_id);
        out.insert(code, json!({ "value": v.value, "label": label, "store_id": v.store_id }));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn dt() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn category() -> Category {
        Category {
            entity_id: 2,
            attribute_set_id: 3,
            parent_id: 1,
            created_at: dt(),
            updated_at: dt(),
            path: "1/2".into(),
            position: 1,
            level: 1,
            children_count: 0,
        }
    }

    fn attr(id: u16, code: &str, label: Option<&str>) -> EavAttribute {
        EavAttribute {
            attribute_id: id,
            entity_type_id: 3,
            attribute_code: code.to_string(),
            attribute_model: None,
            backend_model: None,
            backend_type: "varchar".into(),
            backend_table: None,
            frontend_model: None,
            frontend_input: None,
            frontend_label: label.map(|s| s.to_string()),
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
    fn seeds_static_fields() {
        let flat = flatten_category(&category(), &CategoryEavRows::default(), &CategoryAttributeMeta::default());
        assert_eq!(flat["entity_id"], json!(2));
        assert_eq!(flat["parent_id"], json!(1));
        assert_eq!(flat["path"], json!("1/2"));
        assert_eq!(flat["level"], json!(1));
    }

    #[test]
    fn varchar_value_includes_label_and_store_id() {
        let meta = CategoryAttributeMeta::build(&[attr(50, "name", Some("Category Name"))]);
        let rows = CategoryEavRows {
            varchar: vec![CategoryVarchar { value_id: 1, attribute_id: 50, store_id: 1, entity_id: 2, value: "Shirts".into() }],
            ..Default::default()
        };
        let flat = flatten_category(&category(), &rows, &meta);
        assert_eq!(flat["name"]["value"], json!("Shirts"));
        assert_eq!(flat["name"]["label"], json!("Category Name"));
        assert_eq!(flat["name"]["store_id"], json!(1));
    }

    #[test]
    fn label_falls_back_to_attribute_code_when_no_frontend_label() {
        let meta = CategoryAttributeMeta::build(&[attr(51, "is_active", None)]);
        let rows = CategoryEavRows {
            int: vec![CategoryInt { value_id: 1, attribute_id: 51, store_id: 0, entity_id: 2, value: 1 }],
            ..Default::default()
        };
        let flat = flatten_category(&category(), &rows, &meta);
        assert_eq!(flat["is_active"]["label"], json!("is_active"));
    }

    #[test]
    fn unknown_attribute_id_falls_back_to_numeric_code_and_label() {
        let rows = CategoryEavRows {
            text: vec![CategoryText { value_id: 1, attribute_id: 777, store_id: 0, entity_id: 2, value: "desc".into() }],
            ..Default::default()
        };
        let flat = flatten_category(&category(), &rows, &CategoryAttributeMeta::default());
        assert_eq!(flat["777"]["value"], json!("desc"));
        assert_eq!(flat["777"]["label"], json!("777"));
    }

    #[test]
    fn text_overlay_after_int_and_varchar_on_collision() {
        let meta = CategoryAttributeMeta::build(&[attr(60, "description", Some("Description"))]);
        let rows = CategoryEavRows {
            int: vec![CategoryInt { value_id: 1, attribute_id: 60, store_id: 0, entity_id: 2, value: 0 }],
            text: vec![CategoryText { value_id: 2, attribute_id: 60, store_id: 0, entity_id: 2, value: "final value".into() }],
            ..Default::default()
        };
        let flat = flatten_category(&category(), &rows, &meta);
        assert_eq!(flat["description"]["value"], json!("final value"));
    }
}
