use crate::uid;
use async_graphql::{InputObject, SimpleObject};
use repository::CategoryTreeNode;
use serde_json::Value;

fn get_str(flat: &Value, key: &str) -> Option<String> {
    flat.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn get_f64(flat: &Value, key: &str) -> Option<f64> {
    flat.get(key).and_then(|v| v.as_f64())
}

fn get_id_string(flat: &Value, key: &str) -> String {
    flat.get(key).and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or_default()
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct MediaGalleryItem {
    pub value_id: String,
    pub value: String,
    pub media_type: Option<String>,
    pub disabled: Option<bool>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq, Default)]
#[graphql(rename_fields = "snake_case")]
pub struct Product {
    pub entity_id: String,
    pub sku: String,
    pub name: Option<String>,
    pub price: Option<f64>,
    pub final_price: Option<f64>,
    pub url_key: Option<String>,
    pub image: Option<String>,
    pub short_description: Option<String>,
    pub description: Option<String>,
    pub is_in_stock: Option<bool>,
    pub qty: Option<f64>,
    pub type_id: Option<String>,
    pub category_ids: Option<Vec<String>>,
    pub media_gallery: Option<Vec<MediaGalleryItem>>,
}

impl Product {
    /// Builds a GraphQL `Product` from one repository flat-product map.
    /// `price`/`final_price` are guest-only (Go's `filterPriceForGuest`):
    /// preferentially read from the `index_prices` entry where
    /// `customer_group_id == 0`, falling back to the raw EAV `price`
    /// attribute (if any) only when no guest index-price row exists.
    pub fn from_flat(flat: &Value) -> Self {
        let guest_price = flat
            .get("index_prices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.iter().find(|p| p.get("customer_group_id").and_then(|v| v.as_u64()) == Some(0)));

        let price = guest_price.and_then(|p| p.get("price").and_then(|v| v.as_f64())).or_else(|| get_f64(flat, "price"));
        let final_price = guest_price.and_then(|p| p.get("final_price").and_then(|v| v.as_f64())).or_else(|| get_f64(flat, "final_price"));

        let stock_item = flat.get("stock_item");
        let is_in_stock = stock_item.and_then(|s| s.get("is_in_stock")).and_then(|v| v.as_u64()).map(|v| v == 1);
        let qty = stock_item.and_then(|s| s.get("qty")).and_then(|v| v.as_f64());

        let category_ids = flat
            .get("category_ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).map(|id| id.to_string()).collect());

        Product {
            entity_id: get_id_string(flat, "entity_id"),
            sku: get_str(flat, "sku").unwrap_or_default(),
            name: get_str(flat, "name"),
            price,
            final_price,
            url_key: get_str(flat, "url_key"),
            image: get_str(flat, "image"),
            short_description: get_str(flat, "short_description"),
            description: get_str(flat, "description"),
            is_in_stock,
            qty,
            type_id: get_str(flat, "type_id"),
            category_ids,
            media_gallery: Some(Vec::new()),
        }
    }
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct PageInfo {
    pub page_size: i32,
    pub current_page: i32,
    pub total_pages: i32,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct ProductSearchResult {
    pub items: Vec<Product>,
    pub total_count: i32,
    pub page_info: PageInfo,
}

#[derive(SimpleObject, Clone, Debug, PartialEq, Default)]
#[graphql(rename_fields = "snake_case")]
pub struct Category {
    pub entity_id: String,
    pub name: Option<String>,
    pub url_key: Option<String>,
    pub path: String,
    pub level: i32,
    pub parent_id: String,
    pub children: Vec<Category>,
    /// Not computed by this port (would require counting
    /// `catalog_category_product` rows per category) -- `None` rather than
    /// a fabricated `0`, since "not computed" and "zero products" are
    /// different facts.
    pub product_count: Option<i32>,
}

impl Category {
    /// Builds a GraphQL `Category` from one repository flat-category map
    /// (the `{entity_id, parent_id, path, level, attributes: {...}}` shape
    /// from `category_db::fetch_flat_*`), with no children populated --
    /// used for the flat `categories`/`category(id)` queries, which return
    /// a single level, not a tree.
    pub fn from_flat(flat: &Value) -> Self {
        let attrs = flat.get("attributes");
        let name = attrs.and_then(|a| a.get("name")).and_then(|a| a.get("value")).and_then(|v| v.as_str()).map(str::to_string);
        let url_key = attrs.and_then(|a| a.get("url_key")).and_then(|a| a.get("value")).and_then(|v| v.as_str()).map(str::to_string);

        Category {
            entity_id: get_id_string(flat, "entity_id"),
            name,
            url_key,
            path: get_str(flat, "path").unwrap_or_default(),
            level: flat.get("level").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            parent_id: get_id_string(flat, "parent_id"),
            children: Vec::new(),
            product_count: None,
        }
    }

    /// Builds a GraphQL `Category` tree recursively from a repository
    /// [`CategoryTreeNode`] -- used for the `categoryTree` query.
    pub fn from_tree_node(node: &CategoryTreeNode) -> Self {
        Category {
            entity_id: node.entity_id.to_string(),
            name: node.name.clone(),
            url_key: None,
            path: node.path.clone(),
            level: node.level as i32,
            parent_id: node.parent_id.to_string(),
            children: node.children.iter().map(Category::from_tree_node).collect(),
            product_count: None,
        }
    }
}

#[derive(InputObject, Clone, Debug, Default)]
#[graphql(rename_fields = "snake_case")]
pub struct CategoryFilterEqualTypeInput {
    #[graphql(name = "in")]
    pub r#in: Option<Vec<String>>,
    pub eq: Option<String>,
}

#[derive(InputObject, Clone, Debug, Default)]
#[graphql(rename_fields = "snake_case")]
pub struct CategoryFilterInput {
    pub category_uid: Option<CategoryFilterEqualTypeInput>,
}

#[derive(InputObject, Clone, Debug, Default)]
#[graphql(rename_fields = "snake_case")]
pub struct ProductAttributeFilterInput {
    pub category_uid: Option<CategoryFilterEqualTypeInput>,
}

#[derive(InputObject, Clone, Debug, Default)]
#[graphql(rename_fields = "snake_case")]
pub struct ProductAttributeSortInput {
    pub position: Option<String>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct CategoryTree {
    pub uid: String,
    pub meta_title: Option<String>,
    pub meta_keywords: Option<String>,
    pub meta_description: Option<String>,
    pub url_path: Option<String>,
    pub url_key: Option<String>,
}

impl CategoryTree {
    pub fn from_flat(flat: &Value) -> Self {
        let attrs = flat.get("attributes");
        let get_attr = |k: &str| attrs.and_then(|a| a.get(k)).and_then(|a| a.get("value")).and_then(|v| v.as_str()).map(str::to_string);
        let entity_id = flat.get("entity_id").and_then(|v| v.as_u64()).unwrap_or(0);
        CategoryTree {
            uid: uid::encode(entity_id),
            meta_title: get_attr("meta_title"),
            meta_keywords: get_attr("meta_keywords"),
            meta_description: get_attr("meta_description"),
            url_path: get_attr("url_path"),
            url_key: get_attr("url_key"),
        }
    }
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct CategoryResult {
    pub items: Vec<CategoryTree>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct Money {
    pub currency: String,
    pub value: f64,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct ProductDiscount {
    pub amount_off: Option<f64>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct ProductPrice {
    pub final_price: Money,
    pub regular_price: Money,
    pub discount: Option<ProductDiscount>,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct PriceRange {
    pub maximum_price: ProductPrice,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct ProductImage {
    pub url: String,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct MagentoProduct {
    pub id: i32,
    pub uid: String,
    pub name: Option<String>,
    pub price_range: PriceRange,
    pub sku: String,
    pub small_image: Option<ProductImage>,
    pub stock_status: String,
    pub rating_summary: f64,
    pub url_key: Option<String>,
}

/// Default currency used for the Venia-shaped price types -- there's no
/// real multi-currency store config here, so this is a fixed placeholder
/// rather than left ambiguous.
pub const DEFAULT_CURRENCY: &str = "USD";

impl MagentoProduct {
    pub fn from_flat(flat: &Value) -> Self {
        let product = Product::from_flat(flat);
        let entity_id: u64 = product.entity_id.parse().unwrap_or(0);
        let regular = product.price.unwrap_or(0.0);
        let final_price = product.final_price.unwrap_or(regular);

        MagentoProduct {
            id: entity_id as i32,
            uid: uid::encode(entity_id),
            name: product.name,
            price_range: PriceRange {
                maximum_price: ProductPrice {
                    final_price: Money { currency: DEFAULT_CURRENCY.to_string(), value: final_price },
                    regular_price: Money { currency: DEFAULT_CURRENCY.to_string(), value: regular },
                    discount: if final_price < regular {
                        Some(ProductDiscount { amount_off: Some(regular - final_price) })
                    } else {
                        None
                    },
                },
            },
            sku: product.sku,
            small_image: product.image.map(|url| ProductImage { url }),
            stock_status: if product.is_in_stock.unwrap_or(false) { "IN_STOCK".to_string() } else { "OUT_OF_STOCK".to_string() },
            rating_summary: 0.0,
            url_key: product.url_key,
        }
    }
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct SearchResultPageInfo {
    pub total_pages: i32,
}

#[derive(SimpleObject, Clone, Debug, PartialEq)]
#[graphql(rename_fields = "snake_case")]
pub struct Products {
    pub items: Vec<MagentoProduct>,
    pub page_info: SearchResultPageInfo,
    pub total_count: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn product_from_flat_prefers_guest_index_price_over_raw_attribute() {
        let flat = json!({
            "entity_id": 5, "sku": "SKU-5", "type_id": "simple",
            "price": 100.0,
            "index_prices": [{ "customer_group_id": 0, "price": 80.0, "final_price": 75.0 }],
        });
        let p = Product::from_flat(&flat);
        assert_eq!(p.price, Some(80.0));
        assert_eq!(p.final_price, Some(75.0));
    }

    #[test]
    fn product_from_flat_falls_back_to_raw_price_without_guest_index_row() {
        let flat = json!({ "entity_id": 5, "sku": "SKU-5", "type_id": "simple", "price": 42.0, "index_prices": [] });
        let p = Product::from_flat(&flat);
        assert_eq!(p.price, Some(42.0));
    }

    #[test]
    fn product_from_flat_ignores_non_guest_index_price_rows() {
        let flat = json!({
            "entity_id": 5, "sku": "SKU-5", "type_id": "simple", "price": 42.0,
            "index_prices": [{ "customer_group_id": 1, "price": 999.0, "final_price": 999.0 }],
        });
        let p = Product::from_flat(&flat);
        assert_eq!(p.price, Some(42.0), "group-1 pricing must not leak into the guest view");
    }

    #[test]
    fn product_from_flat_reads_stock_and_category_ids() {
        let flat = json!({
            "entity_id": 5, "sku": "SKU-5", "type_id": "simple",
            "stock_item": { "is_in_stock": 1, "qty": 10.0 },
            "category_ids": [1, 2, 3],
        });
        let p = Product::from_flat(&flat);
        assert_eq!(p.is_in_stock, Some(true));
        assert_eq!(p.qty, Some(10.0));
        assert_eq!(p.category_ids, Some(vec!["1".into(), "2".into(), "3".into()]));
    }

    #[test]
    fn category_from_flat_extracts_name_and_url_key_from_attributes() {
        let flat = json!({
            "entity_id": 2, "parent_id": 1, "path": "1/2", "level": 1,
            "attributes": { "name": { "value": "Shirts", "label": "Name", "store_id": 0 }, "url_key": { "value": "shirts" } },
        });
        let c = Category::from_flat(&flat);
        assert_eq!(c.entity_id, "2");
        assert_eq!(c.parent_id, "1");
        assert_eq!(c.name, Some("Shirts".to_string()));
        assert_eq!(c.url_key, Some("shirts".to_string()));
        assert!(c.children.is_empty());
    }

    #[test]
    fn magento_product_marks_discount_only_when_final_price_is_lower() {
        let flat = json!({
            "entity_id": 5, "sku": "SKU-5", "type_id": "simple",
            "index_prices": [{ "customer_group_id": 0, "price": 100.0, "final_price": 80.0 }],
        });
        let mp = MagentoProduct::from_flat(&flat);
        assert!(mp.price_range.maximum_price.discount.is_some());
        assert_eq!(mp.price_range.maximum_price.discount.unwrap().amount_off, Some(20.0));
    }

    #[test]
    fn magento_product_has_no_discount_when_prices_are_equal() {
        let flat = json!({
            "entity_id": 5, "sku": "SKU-5", "type_id": "simple",
            "index_prices": [{ "customer_group_id": 0, "price": 100.0, "final_price": 100.0 }],
        });
        let mp = MagentoProduct::from_flat(&flat);
        assert!(mp.price_range.maximum_price.discount.is_none());
    }

    #[test]
    fn magento_product_stock_status_reflects_stock_item() {
        let in_stock = json!({ "entity_id": 1, "sku": "S", "type_id": "simple", "stock_item": { "is_in_stock": 1 } });
        assert_eq!(MagentoProduct::from_flat(&in_stock).stock_status, "IN_STOCK");

        let out_of_stock = json!({ "entity_id": 1, "sku": "S", "type_id": "simple", "stock_item": { "is_in_stock": 0 } });
        assert_eq!(MagentoProduct::from_flat(&out_of_stock).stock_status, "OUT_OF_STOCK");

        let no_stock_row = json!({ "entity_id": 1, "sku": "S", "type_id": "simple" });
        assert_eq!(MagentoProduct::from_flat(&no_stock_row).stock_status, "OUT_OF_STOCK");
    }

    #[test]
    fn category_tree_from_flat_encodes_uid_and_reads_meta_attributes() {
        let flat = json!({
            "entity_id": 7, "parent_id": 1, "path": "1/7", "level": 1,
            "attributes": { "url_key": { "value": "shoes" } },
        });
        let ct = CategoryTree::from_flat(&flat);
        assert_eq!(ct.uid, uid::encode(7));
        assert_eq!(ct.url_key, Some("shoes".to_string()));
        assert_eq!(ct.meta_title, None);
    }
}
