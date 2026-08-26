use crate::category::string_field;
use crate::state::WebState;
use crate::templates::{Breadcrumb, IndexPriceRow, ProductPage};
use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// Category IDs excluded from breadcrumbs -- the unnamed true root (0/1)
/// and the single default bucket category every seeded product starts in
/// (2) aren't meaningful storefront navigation, matching Go's identical
/// hardcoded exclusion set in `buildCategoryBreadcrumbs`.
const BREADCRUMB_EXCLUDED_IDS: [u64; 3] = [0, 1, 2];

pub async fn show(State(state): State<WebState>, Path(entity_id): Path<u64>) -> Response {
    let flat = match repository::product_db::fetch_flat_by_id(&state.pool, &state.product_cache, &state.product_code_map, 0, entity_id, false).await {
        Ok(Some(flat)) => flat,
        Ok(None) => return (StatusCode::NOT_FOUND, "Product not found").into_response(),
        Err(e) => {
            tracing::error!("product fetch failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Error fetching product").into_response();
        }
    };

    let category_ids: Vec<u64> = flat.get("category_ids").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
    let breadcrumbs = match category_ids.last() {
        Some(&last_category_id) => build_breadcrumbs(&state, last_category_id).await,
        None => Vec::new(),
    };

    let gallery: Vec<String> = flat.get("media_gallery").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|v| v.get("value")?.as_str().map(str::to_string)).collect()).unwrap_or_default();

    let index_prices: Vec<IndexPriceRow> = flat
        .get("index_prices")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|p| IndexPriceRow {
                    customer_group_id: string_field(p, "customer_group_id"),
                    website_id: string_field(p, "website_id"),
                    price: string_field(p, "price"),
                    final_price: string_field(p, "final_price"),
                    min_price: string_field(p, "min_price"),
                    max_price: string_field(p, "max_price"),
                })
                .collect()
        })
        .unwrap_or_default();

    let (in_stock, stock_qty) = match flat.get("stock_item") {
        Some(s) => {
            let in_stock = s.get("is_in_stock").and_then(|v| v.as_u64()).unwrap_or(0) == 1;
            let qty = s.get("qty").filter(|v| !v.is_null()).map(|v| match v {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            });
            (in_stock, qty)
        }
        None => (false, None),
    };

    let product_name = string_field(&flat, "name");
    let product_sku = string_field(&flat, "sku");
    let (category_tree_html, top_nav_html) = state.nav_fragments().await;

    let page = ProductPage {
        title: format!("Product Page - {product_name} - {product_sku} - RustGento"),
        meta_description: format!("Buy {product_name} (SKU: {product_sku})"),
        category_tree_html,
        top_nav_html,
        search_query: String::new(),
        media_url: state.media_url.clone(),
        breadcrumbs,
        entity_id,
        product_name,
        product_sku,
        product_price: string_field(&flat, "price"),
        product_image: string_field(&flat, "image"),
        gallery,
        in_stock,
        stock_qty,
        description: string_field(&flat, "description"),
        index_prices,
    };

    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("template render failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

/// Walks a category's materialized path (e.g. "1/2/5") up to (but not
/// including) `last_category_id` itself... actually including it as the
/// final crumb, resolving each segment's display name -- mirrors Go's
/// `buildCategoryBreadcrumbs`.
async fn build_breadcrumbs(state: &WebState, last_category_id: u64) -> Vec<Breadcrumb> {
    let Ok(Some(category)) = repository::category_db::find_by_id(&state.pool, last_category_id).await else {
        return Vec::new();
    };

    let mut ids: Vec<u64> = Vec::new();
    for part in category.path.split('/') {
        let Ok(id) = part.parse::<u64>() else { continue };
        if BREADCRUMB_EXCLUDED_IDS.contains(&id) {
            continue;
        }
        ids.push(id);
    }
    if ids.is_empty() {
        return Vec::new();
    }

    let mut breadcrumbs = Vec::with_capacity(ids.len());
    for id in ids {
        if let Ok(Some(flat)) = repository::category_db::fetch_flat_by_id(&state.pool, &state.category_cache, &state.category_meta, 0, id).await {
            let name = flat.get("name").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| id.to_string());
            breadcrumbs.push(Breadcrumb { entity_id: id, name });
        }
    }
    breadcrumbs
}
