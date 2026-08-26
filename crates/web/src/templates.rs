//! Askama template structs for the storefront pages. Askama checks these
//! at compile time against `templates/*.html` -- a typo'd field name or a
//! template referencing a field the struct doesn't have fails `cargo
//! build`, not a runtime request.

use askama::Template;

#[derive(Debug, Clone)]
pub struct GridProduct {
    pub entity_id: u64,
    pub name: String,
    pub sku: String,
    pub price: String,
    pub image: String,
}

#[derive(Template)]
#[template(path = "category.html")]
pub struct CategoryPage {
    pub title: String,
    pub meta_description: String,
    pub category_tree_html: String,
    pub top_nav_html: String,
    pub search_query: String,
    pub category_id: u64,
    pub category_name: String,
    pub products: Vec<GridProduct>,
    pub media_url: String,
    pub page: usize,
    pub total_pages: usize,
    pub limit: usize,
    pub page_numbers: Vec<usize>,
    pub prev_page: usize,
    pub next_page: usize,
}

#[derive(Debug, Clone)]
pub struct Breadcrumb {
    pub entity_id: u64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct IndexPriceRow {
    pub customer_group_id: String,
    pub website_id: String,
    pub price: String,
    pub final_price: String,
    pub min_price: String,
    pub max_price: String,
}

#[derive(Debug, Clone)]
pub struct Slide {
    pub eyebrow: String,
    pub heading: String,
    pub body: String,
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomePage {
    pub title: String,
    pub meta_description: String,
    pub category_tree_html: String,
    pub top_nav_html: String,
    pub search_query: String,
    pub slides: Vec<Slide>,
    pub product_count: i64,
    pub category_count: i64,
    pub featured_category_id: Option<u64>,
    pub tech_stack: Vec<String>,
}

#[derive(Template)]
#[template(path = "search.html")]
pub struct SearchPage {
    pub title: String,
    pub meta_description: String,
    pub category_tree_html: String,
    pub top_nav_html: String,
    pub search_query: String,
    pub products: Vec<GridProduct>,
    pub media_url: String,
    pub page: usize,
    pub total_pages: usize,
    pub limit: usize,
    pub page_numbers: Vec<usize>,
    pub prev_page: usize,
    pub next_page: usize,
}

#[derive(Template)]
#[template(path = "product.html")]
pub struct ProductPage {
    pub title: String,
    pub meta_description: String,
    pub category_tree_html: String,
    pub top_nav_html: String,
    pub search_query: String,
    pub media_url: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub entity_id: u64,
    pub product_name: String,
    pub product_sku: String,
    pub product_price: String,
    pub product_image: String,
    pub gallery: Vec<String>,
    pub in_stock: bool,
    pub stock_qty: Option<String>,
    pub description: String,
    pub index_prices: Vec<IndexPriceRow>,
}
