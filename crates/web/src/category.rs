use crate::state::WebState;
use crate::templates::{CategoryPage, GridProduct};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use std::cmp::{max, min};

#[derive(Debug, Deserialize)]
pub struct CategoryQuery {
    p: Option<usize>,
    limit: Option<usize>,
}

/// Computed pagination window over a product-ID list -- mirrors Go's
/// `calculatePagination`: clamps the requested page into range and shows
/// at most 5 page-number links, centered on the current page where
/// possible.
struct Pagination {
    page: usize,
    limit: usize,
    total_pages: usize,
    page_numbers: Vec<usize>,
    prev_page: usize,
    next_page: usize,
}

fn paginate(total_items: usize, requested_page: usize, limit: usize) -> Pagination {
    let limit = limit.max(1);
    let total_pages = total_items.div_ceil(limit).max(1);
    let page = requested_page.clamp(1, total_pages);

    const MAX_PAGES_SHOWN: usize = 5;
    let start_page = max(1, page as isize - MAX_PAGES_SHOWN as isize / 2) as usize;
    let mut end_page = min(total_pages, start_page + MAX_PAGES_SHOWN - 1);
    let start_page = if end_page - start_page + 1 < MAX_PAGES_SHOWN { max(1, end_page as isize - MAX_PAGES_SHOWN as isize + 1) as usize } else { start_page };
    if end_page < start_page {
        end_page = start_page;
    }

    Pagination {
        page,
        limit,
        total_pages,
        page_numbers: (start_page..=end_page).collect(),
        prev_page: page.saturating_sub(1).max(1),
        next_page: (page + 1).min(total_pages),
    }
}

pub async fn show(State(state): State<WebState>, Path(category_id): Path<u64>, Query(q): Query<CategoryQuery>) -> Response {
    let flat = match repository::category_db::fetch_flat_by_id(&state.pool, &state.category_cache, &state.category_meta, 0, category_id).await {
        Ok(Some(flat)) => flat,
        Ok(None) => return (StatusCode::NOT_FOUND, "Category not found").into_response(),
        Err(e) => {
            tracing::error!("category fetch failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Error fetching category").into_response();
        }
    };

    let product_ids = match repository::product_ids_in_category(&state.pool, category_id).await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::error!("category product list failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Error fetching category products").into_response();
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
                tracing::error!("category product flatten failed: {e}");
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

    let category_tree_html = match state.category_tree_html().await {
        Ok(html) => html,
        Err(e) => {
            tracing::warn!("category tree render failed: {e}");
            String::new()
        }
    };

    let category_name = flat.get("name").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| category_id.to_string());
    let title = format!("Category Page - {category_name} - RustGento");

    let page = CategoryPage {
        title,
        meta_description: format!("Browse our {category_name} collection. Find the best products in our catalog."),
        category_tree_html,
        category_id,
        category_name,
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

/// Reads a flattened attribute value as a display string. EAV values arrive
/// as whatever JSON type their backend_type maps to (numbers stay numbers),
/// so this normalizes any scalar into a String for template display rather
/// than assuming every attribute is already a JSON string.
pub(crate) fn string_field(obj: &serde_json::Value, key: &str) -> String {
    match obj.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_defaults_to_page_one() {
        let p = paginate(45, 1, 20);
        assert_eq!(p.page, 1);
        assert_eq!(p.total_pages, 3);
        assert_eq!(p.page_numbers, vec![1, 2, 3]);
        assert_eq!(p.prev_page, 1);
        assert_eq!(p.next_page, 2);
    }

    #[test]
    fn paginate_clamps_page_beyond_total() {
        let p = paginate(10, 99, 20);
        assert_eq!(p.page, 1);
        assert_eq!(p.total_pages, 1);
    }

    #[test]
    fn paginate_clamps_page_below_one() {
        let p = paginate(45, 0, 20);
        assert_eq!(p.page, 1);
    }

    #[test]
    fn paginate_shows_at_most_five_page_numbers_centered_on_current() {
        let p = paginate(400, 10, 20); // 20 total pages, on page 10
        assert_eq!(p.page_numbers.len(), 5);
        assert!(p.page_numbers.contains(&10));
        assert_eq!(p.prev_page, 9);
        assert_eq!(p.next_page, 11);
    }

    #[test]
    fn paginate_page_numbers_near_the_end_dont_run_past_total() {
        let p = paginate(400, 20, 20); // last page
        assert_eq!(*p.page_numbers.last().unwrap(), 20);
        assert_eq!(p.page_numbers.len(), 5);
        assert_eq!(p.next_page, 20);
    }

    #[test]
    fn paginate_single_page_has_no_prev_or_next() {
        let p = paginate(5, 1, 20);
        assert_eq!(p.total_pages, 1);
        assert_eq!(p.prev_page, 1);
        assert_eq!(p.next_page, 1);
    }

    #[test]
    fn string_field_reads_string_and_numeric_values() {
        let obj = serde_json::json!({"name": "Widget", "price": 9.99, "missing_is_null": null});
        assert_eq!(string_field(&obj, "name"), "Widget");
        assert_eq!(string_field(&obj, "price"), "9.99");
        assert_eq!(string_field(&obj, "missing_is_null"), "");
        assert_eq!(string_field(&obj, "does_not_exist"), "");
    }
}
