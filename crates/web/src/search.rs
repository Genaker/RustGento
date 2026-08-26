use crate::category::string_field;
use crate::pagination::paginate;
use crate::state::WebState;
use crate::templates::{GridProduct, SearchPage};
use askama::Template;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    p: Option<usize>,
    limit: Option<usize>,
}

pub async fn show(State(state): State<WebState>, Query(q): Query<SearchQuery>) -> Response {
    let query = q.q.unwrap_or_default().trim().to_string();

    let product_ids = if query.is_empty() {
        Vec::new()
    } else {
        match repository::product_db::search_ids(&state.pool, state.name_attribute_id, &query).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::error!("product search failed: {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Error searching products").into_response();
            }
        }
    };

    let pagination = paginate(product_ids.len(), q.p.unwrap_or(1), q.limit.unwrap_or(20));
    let start = (pagination.page - 1) * pagination.limit;
    let end = (start + pagination.limit).min(product_ids.len());
    let page_ids = product_ids.get(start.min(product_ids.len())..end).unwrap_or(&[]);

    let flat_products = if page_ids.is_empty() {
        Vec::new()
    } else {
        match repository::product_db::fetch_flat_by_ids(&state.pool, &state.product_cache, &state.product_code_map, 0, page_ids).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("search result flatten failed: {e}");
                Vec::new()
            }
        }
    };

    let products: Vec<GridProduct> = flat_products
        .iter()
        .map(|p| GridProduct {
            entity_id: p.get("entity_id").and_then(|v| v.as_u64()).unwrap_or_default(),
            name: string_field(p, "name"),
            sku: string_field(p, "sku"),
            price: string_field(p, "price"),
            image: string_field(p, "image"),
        })
        .collect();

    let (category_tree_html, top_nav_html) = state.nav_fragments().await;

    let page = SearchPage {
        title: format!("Search: {query} - RustGento"),
        meta_description: format!("Search results for {query}"),
        category_tree_html,
        top_nav_html,
        search_query: query,
        products,
        media_url: state.media_url.clone(),
        page: pagination.page,
        total_pages: pagination.total_pages,
        limit: pagination.limit,
        page_numbers: pagination.page_numbers,
        prev_page: pagination.prev_page,
        next_page: pagination.next_page,
    };

    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("template render failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}
