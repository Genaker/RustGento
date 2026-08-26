use crate::state::WebState;
use crate::templates::{HomePage, Slide};
use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

pub async fn show(State(state): State<WebState>) -> Response {
    let product_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_product_entity").fetch_one(&state.pool).await.unwrap_or(0);
    let category_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog_category_entity").fetch_one(&state.pool).await.unwrap_or(0);
    let featured_category_id: Option<u64> = sqlx::query_scalar("SELECT category_id FROM catalog_category_product GROUP BY category_id ORDER BY COUNT(*) DESC LIMIT 1")
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    let category_tree_html = state.category_tree_html().await.unwrap_or_else(|e| {
        tracing::warn!("category tree render failed: {e}");
        String::new()
    });

    let page = HomePage {
        title: "RustGento — a Rust-native Magento catalog service".to_string(),
        meta_description: "A Rust reimplementation of a Magento-style catalog API, benchmarked feature-for-feature against an equivalent Go service.".to_string(),
        category_tree_html,
        slides: default_slides(),
        product_count,
        category_count,
        featured_category_id,
        tech_stack: ["Rust", "axum", "sqlx", "askama", "async-graphql", "tokio", "MySQL", "tower-http"].into_iter().map(String::from).collect(),
    };

    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("template render failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

fn default_slides() -> Vec<Slide> {
    vec![
        Slide {
            eyebrow: "Rust Reimplementation".to_string(),
            heading: "A Magento-style catalog, rebuilt in Rust".to_string(),
            body: "REST, GraphQL, a realtime price API, a bulk CSV importer, and this storefront -- all ported from an equivalent Go service, feature for feature.".to_string(),
        },
        Slide {
            eyebrow: "Benchmarked, Not Assumed".to_string(),
            heading: "Measured against the original Go service".to_string(),
            body: "Same MySQL instance, same CSV fixtures, same queries. The baseline import benchmark came out essentially tied once both sides batched their writes the same way -- performance work, not language mythology.".to_string(),
        },
        Slide {
            eyebrow: "Compile-Time Safety".to_string(),
            heading: "Typed all the way down".to_string(),
            body: "A typed EAV entity layer, compile-time-checked SQL, and compile-time-checked HTML templates -- a wrong field name fails cargo build, not a live request.".to_string(),
        },
        Slide {
            eyebrow: "Feature Parity".to_string(),
            heading: "9 Magento catalog features, ported and tested".to_string(),
            body: "Categories, tier & group pricing, product links, image galleries, custom options, downloadable products, bundles, and configurable products -- each with its own test suite.".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_slides_has_four_non_empty_slides() {
        let slides = default_slides();
        assert_eq!(slides.len(), 4);
        for s in &slides {
            assert!(!s.eyebrow.is_empty());
            assert!(!s.heading.is_empty());
            assert!(!s.body.is_empty());
        }
    }
}
