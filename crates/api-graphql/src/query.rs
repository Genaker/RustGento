use crate::context::{GraphQLContext, StoreId};
use crate::models::*;
use crate::pagination::paginate;
use crate::uid;
use async_graphql::{Context, Object, Result};
use repository::{category_db, product_db};

pub struct Query;

async fn all_flat_products(ctx: &Context<'_>) -> Result<Vec<serde_json::Value>> {
    let gql = ctx.data::<GraphQLContext>()?;
    let store_id = ctx.data::<StoreId>().map(|s| s.0).unwrap_or(0);
    let force_bypass = !gql.product_flat_cache_enabled;
    let flat = product_db::fetch_flat_list(&gql.pool, &gql.product_cache, &gql.product_code_map, store_id, 0, force_bypass).await?;
    Ok(flat)
}

// Deliberately NOT `rename_fields = "none"` here: Go's schema uses camelCase
// for root query names (`categoryTree`, `magentoCategories`,
// `magentoProducts`) and arguments (`pageSize`, `currentPage`, `categoryId`)
// -- async-graphql's default camelCase conversion already matches that. Only
// the OUTPUT TYPES in `models.rs` need `rename_fields = "none"`, since Go's
// schema keeps those snake_case (`entity_id`, `is_in_stock`, `total_count`).
#[Object]
impl Query {
    async fn products(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 20)] page_size: i32,
        #[graphql(default = 1)] current_page: i32,
        skus: Option<Vec<String>>,
        category_id: Option<String>,
    ) -> Result<ProductSearchResult> {
        let mut flat = all_flat_products(ctx).await?;

        if let Some(skus) = &skus {
            flat.retain(|p| p.get("sku").and_then(|v| v.as_str()).map(|s| skus.iter().any(|w| w == s)).unwrap_or(false));
        }
        if let Some(category_id) = &category_id {
            let Ok(cat_id) = category_id.parse::<u64>() else {
                return Err("invalid categoryId".into());
            };
            flat.retain(|p| p.get("category_ids").and_then(|v| v.as_array()).map(|ids| ids.iter().any(|id| id.as_u64() == Some(cat_id))).unwrap_or(false));
        }

        let page = paginate(flat.len(), page_size, current_page);
        let items = flat[page.start..page.end].iter().map(Product::from_flat).collect();

        Ok(ProductSearchResult {
            items,
            total_count: flat.len() as i32,
            page_info: PageInfo { page_size, current_page, total_pages: page.total_pages },
        })
    }

    /// Scans the full flat-product set for a matching `sku` or `url_key` --
    /// matches Go's own approach (no targeted single-column DB lookup here).
    async fn product(&self, ctx: &Context<'_>, sku: Option<String>, url_key: Option<String>) -> Result<Option<Product>> {
        let flat = all_flat_products(ctx).await?;
        let found = flat.iter().find(|p| {
            (sku.is_some() && p.get("sku").and_then(|v| v.as_str()) == sku.as_deref())
                || (url_key.is_some() && p.get("url_key").and_then(|v| v.as_str()) == url_key.as_deref())
        });
        Ok(found.map(Product::from_flat))
    }

    async fn categories(&self, ctx: &Context<'_>) -> Result<Vec<Category>> {
        let gql = ctx.data::<GraphQLContext>()?;
        let store_id = ctx.data::<StoreId>().map(|s| s.0).unwrap_or(0);
        let flat = category_db::fetch_flat_list(&gql.pool, &gql.category_cache, &gql.category_meta, store_id).await?;
        Ok(flat.iter().map(Category::from_flat).collect())
    }

    async fn category(&self, ctx: &Context<'_>, id: String) -> Result<Option<Category>> {
        let gql = ctx.data::<GraphQLContext>()?;
        let store_id = ctx.data::<StoreId>().map(|s| s.0).unwrap_or(0);
        let Ok(id) = id.parse::<u64>() else { return Ok(None) };
        let flat = category_db::fetch_flat_by_id(&gql.pool, &gql.category_cache, &gql.category_meta, store_id, id).await?;
        Ok(flat.map(|f| Category::from_flat(&f)))
    }

    async fn category_tree(&self, ctx: &Context<'_>) -> Result<Vec<Category>> {
        let gql = ctx.data::<GraphQLContext>()?;
        let tree = category_db::build_tree(&gql.pool, &gql.category_meta, 0).await?;
        Ok(tree.iter().map(Category::from_tree_node).collect())
    }

    /// Magento/Venia `GetCategories`-compatible query.
    async fn magento_categories(&self, ctx: &Context<'_>, filters: Option<CategoryFilterInput>) -> Result<CategoryResult> {
        let gql = ctx.data::<GraphQLContext>()?;
        let store_id = ctx.data::<StoreId>().map(|s| s.0).unwrap_or(0);
        let flat = category_db::fetch_flat_list(&gql.pool, &gql.category_cache, &gql.category_meta, store_id).await?;

        let matches_filter = |entity_id: u64, filter: &CategoryFilterEqualTypeInput| -> bool {
            if let Some(eq) = &filter.eq {
                return uid::decode(eq) == Some(entity_id);
            }
            if let Some(in_list) = &filter.r#in {
                return in_list.iter().any(|u| uid::decode(u) == Some(entity_id));
            }
            true
        };

        let items = flat
            .iter()
            .filter(|c| {
                let Some(filters) = &filters else { return true };
                let Some(cat_filter) = &filters.category_uid else { return true };
                let entity_id = c.get("entity_id").and_then(|v| v.as_u64()).unwrap_or(0);
                matches_filter(entity_id, cat_filter)
            })
            .map(CategoryTree::from_flat)
            .collect();

        Ok(CategoryResult { items })
    }

    /// Magento/Venia `products`-compatible query (the `MagentoProduct` /
    /// `PriceRange` / `Money` shape), filterable by `category_uid`.
    async fn magento_products(
        &self,
        ctx: &Context<'_>,
        filter: Option<ProductAttributeFilterInput>,
        _sort: Option<ProductAttributeSortInput>,
        #[graphql(default = 12)] page_size: i32,
        #[graphql(default = 1)] current_page: i32,
    ) -> Result<Products> {
        let mut flat = all_flat_products(ctx).await?;

        if let Some(filter) = &filter {
            if let Some(cat_filter) = &filter.category_uid {
                let matches_uid = |c: Option<u64>| -> bool {
                    let Some(id) = c else { return false };
                    if let Some(eq) = &cat_filter.eq {
                        return uid::decode(eq) == Some(id);
                    }
                    if let Some(list) = &cat_filter.r#in {
                        return list.iter().any(|u| uid::decode(u) == Some(id));
                    }
                    true
                };
                flat.retain(|p| {
                    p.get("category_ids")
                        .and_then(|v| v.as_array())
                        .map(|ids| ids.iter().any(|id| matches_uid(id.as_u64())))
                        .unwrap_or(false)
                });
            }
        }

        let page = paginate(flat.len(), page_size, current_page);
        let items = flat[page.start..page.end].iter().map(MagentoProduct::from_flat).collect();

        Ok(Products { items, total_count: flat.len() as i32, page_info: SearchResultPageInfo { total_pages: page.total_pages } })
    }

    /// Elasticsearch-backed full-text search -- an explicit non-goal for
    /// this port (see the project plan). Present in the schema for
    /// introspection parity; always returns an empty result set rather than
    /// being omitted or panicking.
    async fn search(
        &self,
        #[graphql(default = 20)] page_size: i32,
        #[graphql(default = 1)] current_page: i32,
        _query: String,
        _category_id: Option<String>,
    ) -> Result<ProductSearchResult> {
        let page = paginate(0, page_size, current_page);
        Ok(ProductSearchResult { items: Vec::new(), total_count: 0, page_info: PageInfo { page_size, current_page, total_pages: page.total_pages } })
    }

    /// Calls a registered extension by name -- the runtime extension
    /// registry (`core/registry`) is an explicit non-goal for this port
    /// (see the project plan). Always returns `null`.
    #[graphql(name = "_extension")]
    async fn extension(&self, _name: String, _args: Option<String>) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::test_schema;

    #[tokio::test]
    async fn products_query_paginates_and_reports_total_count() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute("query { products(pageSize: 2, currentPage: 1) { total_count page_info { page_size current_page total_pages } items { entity_id sku } } }").await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        let items = json["products"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(json["products"]["total_count"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn products_query_filters_by_sku() {
        let Some(schema) = test_schema().await else { return };
        let res = schema
            .execute(r#"query { products(pageSize: 20, currentPage: 1, skus: ["SAMPLE-SKU-0000"]) { total_count items { sku } } }"#)
            .await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert_eq!(json["products"]["total_count"], 1);
        assert_eq!(json["products"]["items"][0]["sku"], "SAMPLE-SKU-0000");
    }

    #[tokio::test]
    async fn products_query_rejects_invalid_category_id() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute(r#"query { products(categoryId: "not-a-number") { total_count } }"#).await;
        assert!(!res.errors.is_empty(), "an invalid categoryId must produce a GraphQL error, not a silent empty result");
    }

    #[tokio::test]
    async fn product_query_finds_by_sku() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute(r#"query { product(sku: "SAMPLE-SKU-0000") { entity_id sku } }"#).await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert_eq!(json["product"]["sku"], "SAMPLE-SKU-0000");
    }

    #[tokio::test]
    async fn product_query_returns_null_for_unknown_sku() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute(r#"query { product(sku: "DEFINITELY-NOT-A-REAL-SKU") { sku } }"#).await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert!(json["product"].is_null());
    }

    #[tokio::test]
    async fn categories_query_returns_seeded_categories() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute("query { categories { entity_id path level } }").await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert!(!json["categories"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn category_query_finds_by_id_and_returns_null_for_unknown() {
        let Some(schema) = test_schema().await else { return };
        let found = schema.execute(r#"query { category(id: "1") { entity_id } }"#).await;
        assert!(found.errors.is_empty(), "errors: {:?}", found.errors);
        assert_eq!(serde_json::to_value(&found.data).unwrap()["category"]["entity_id"], "1");

        let missing = schema.execute(r#"query { category(id: "999999999") { entity_id } }"#).await;
        assert!(serde_json::to_value(&missing.data).unwrap()["category"].is_null());

        let invalid = schema.execute(r#"query { category(id: "not-a-number") { entity_id } }"#).await;
        assert!(invalid.errors.is_empty(), "a non-numeric id should resolve to null, not a GraphQL error");
        assert!(serde_json::to_value(&invalid.data).unwrap()["category"].is_null());
    }

    #[tokio::test]
    async fn category_tree_query_returns_a_rooted_hierarchy() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute("query { categoryTree { entity_id parent_id children { entity_id } } }").await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert!(!json["categoryTree"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn magento_categories_query_filters_by_uid_eq() {
        let Some(schema) = test_schema().await else { return };
        let uid = crate::uid::encode(1);
        let res = schema.execute(format!(r#"query {{ magentoCategories(filters: {{ category_uid: {{ eq: "{uid}" }} }}) {{ items {{ uid }} }} }}"#)).await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        let items = json["magentoCategories"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["uid"], uid);
    }

    #[tokio::test]
    async fn magento_categories_query_returns_all_without_a_filter() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute("query { magentoCategories { items { uid } } }").await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert!(!json["magentoCategories"]["items"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn magento_products_query_paginates() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute("query { magentoProducts(pageSize: 2, currentPage: 1) { total_count items { uid sku } } }").await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert_eq!(json["magentoProducts"]["items"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn magento_products_query_filters_by_category_uid() {
        let Some(schema) = test_schema().await else { return };
        // Seed data links products to category entity_id=2.
        let uid = crate::uid::encode(2);
        let res = schema
            .execute(format!(
                r#"query {{ magentoProducts(filter: {{ category_uid: {{ eq: "{uid}" }} }}, pageSize: 500) {{ total_count items {{ sku }} }} }}"#
            ))
            .await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert!(json["magentoProducts"]["total_count"].as_i64().unwrap() > 0, "seed data links products to category 2");
    }

    #[tokio::test]
    async fn magento_products_query_filter_excludes_products_in_other_categories() {
        let Some(schema) = test_schema().await else { return };
        let uid = crate::uid::encode(999_999_999); // no products linked to a nonexistent category
        let res = schema.execute(format!(r#"query {{ magentoProducts(filter: {{ category_uid: {{ eq: "{uid}" }} }}) {{ total_count }} }}"#)).await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert_eq!(json["magentoProducts"]["total_count"], 0);
    }

    #[tokio::test]
    async fn search_query_always_returns_an_empty_stub_result() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute(r#"query { search(query: "anything", pageSize: 5, currentPage: 1) { total_count page_info { total_pages } items { sku } } }"#).await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let json = serde_json::to_value(&res.data).unwrap();
        assert_eq!(json["search"]["total_count"], 0);
        assert!(json["search"]["items"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn extension_query_always_returns_null() {
        let Some(schema) = test_schema().await else { return };
        let res = schema.execute(r#"query { _extension(name: "ping", args: "{}") }"#).await;
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        assert!(serde_json::to_value(&res.data).unwrap()["_extension"].is_null());
    }
}
