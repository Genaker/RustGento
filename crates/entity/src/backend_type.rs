use std::fmt;

/// The Magento EAV `backend_type` values we support (CE product/category attributes).
/// Mirrors the Go `import_eav.go` table-name map, but as a typed enum instead of
/// a `map[string]string` keyed by raw strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendType {
    Varchar,
    Int,
    Decimal,
    Text,
    Datetime,
}

impl BackendType {
    /// All variants, in the fixed order the Go flatten logic overlays them:
    /// varchar -> int -> decimal -> text -> datetime (last writer wins on collision).
    pub const ALL: [BackendType; 5] = [
        BackendType::Varchar,
        BackendType::Int,
        BackendType::Decimal,
        BackendType::Text,
        BackendType::Datetime,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            BackendType::Varchar => "varchar",
            BackendType::Int => "int",
            BackendType::Decimal => "decimal",
            BackendType::Text => "text",
            BackendType::Datetime => "datetime",
        }
    }

    /// The product-side EAV value table name for this backend type.
    pub fn product_table(&self) -> &'static str {
        match self {
            BackendType::Varchar => "catalog_product_entity_varchar",
            BackendType::Int => "catalog_product_entity_int",
            BackendType::Decimal => "catalog_product_entity_decimal",
            BackendType::Text => "catalog_product_entity_text",
            BackendType::Datetime => "catalog_product_entity_datetime",
        }
    }

    /// The category-side EAV value table name, if one exists.
    /// Magento categories have no decimal/datetime attribute tables.
    pub fn category_table(&self) -> Option<&'static str> {
        match self {
            BackendType::Varchar => Some("catalog_category_entity_varchar"),
            BackendType::Int => Some("catalog_category_entity_int"),
            BackendType::Text => Some("catalog_category_entity_text"),
            BackendType::Decimal | BackendType::Datetime => None,
        }
    }

    pub fn parse(s: &str) -> Option<BackendType> {
        match s {
            "varchar" => Some(BackendType::Varchar),
            "int" => Some(BackendType::Int),
            "decimal" => Some(BackendType::Decimal),
            "text" => Some(BackendType::Text),
            "datetime" => Some(BackendType::Datetime),
            _ => None,
        }
    }
}

impl fmt::Display for BackendType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trip_for_all_known_types() {
        for bt in BackendType::ALL {
            assert_eq!(BackendType::parse(bt.as_str()), Some(bt));
        }
    }

    #[test]
    fn parse_rejects_unknown_backend_type() {
        assert_eq!(BackendType::parse("static"), None);
        assert_eq!(BackendType::parse(""), None);
        assert_eq!(BackendType::parse("VARCHAR"), None);
    }

    #[test]
    fn product_table_names_match_magento_schema() {
        assert_eq!(BackendType::Varchar.product_table(), "catalog_product_entity_varchar");
        assert_eq!(BackendType::Int.product_table(), "catalog_product_entity_int");
        assert_eq!(BackendType::Decimal.product_table(), "catalog_product_entity_decimal");
        assert_eq!(BackendType::Text.product_table(), "catalog_product_entity_text");
        assert_eq!(BackendType::Datetime.product_table(), "catalog_product_entity_datetime");
    }

    #[test]
    fn category_table_is_none_for_decimal_and_datetime() {
        assert_eq!(BackendType::Decimal.category_table(), None);
        assert_eq!(BackendType::Datetime.category_table(), None);
        assert_eq!(BackendType::Varchar.category_table(), Some("catalog_category_entity_varchar"));
        assert_eq!(BackendType::Int.category_table(), Some("catalog_category_entity_int"));
        assert_eq!(BackendType::Text.category_table(), Some("catalog_category_entity_text"));
    }

    #[test]
    fn display_matches_as_str() {
        for bt in BackendType::ALL {
            assert_eq!(bt.to_string(), bt.as_str());
        }
    }

    #[test]
    fn all_order_matches_go_flatten_overlay_order() {
        let order: Vec<&str> = BackendType::ALL.iter().map(|b| b.as_str()).collect();
        assert_eq!(order, vec!["varchar", "int", "decimal", "text", "datetime"]);
    }
}
