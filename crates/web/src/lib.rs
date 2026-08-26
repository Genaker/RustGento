//! Server-rendered storefront pages -- category and product listing,
//! matching Go's `html/` package (compile-time-checked `askama` templates
//! instead of `html/template`, since a typo'd field only surfaces at
//! `cargo build` here rather than at request time).

mod category;
mod home;
mod image;
mod product;
pub mod state;
mod templates;

pub use state::WebState;

use axum::routing::get;
use axum::Router;

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(home::show))
        .route("/category/{id}", get(category::show))
        .route("/product/{id}", get(product::show))
        .route("/image/webp", get(image::show))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn test_state() -> Option<WebState> {
        let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| "mysql://magento:magento@127.0.0.1:3309/magento".to_string());
        let pool = sqlx::mysql::MySqlPoolOptions::new().acquire_timeout(std::time::Duration::from_secs(3)).connect(&url).await.ok()?;
        WebState::new(pool).await.ok()
    }

    #[tokio::test]
    async fn home_page_renders() {
        let Some(state) = test_state().await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers().get("content-type").unwrap().to_str().unwrap().to_string();
        assert!(content_type.contains("text/html"));
    }

    #[tokio::test]
    async fn unknown_category_id_is_404() {
        let Some(state) = test_state().await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/category/999999999").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_product_id_is_404() {
        let Some(state) = test_state().await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/product/999999999").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn image_without_src_is_bad_request() {
        let Some(state) = test_state().await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/image/webp").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn known_category_renders_a_page() {
        let Some(state) = test_state().await else { return };
        // The seed data always assigns products to category_id=2.
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/category/2").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers().get("content-type").unwrap().to_str().unwrap().to_string();
        assert!(content_type.contains("text/html"));
    }

    #[tokio::test]
    async fn known_product_renders_a_page() {
        let Some(pool) = ({
            let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| "mysql://magento:magento@127.0.0.1:3309/magento".to_string());
            sqlx::mysql::MySqlPoolOptions::new().acquire_timeout(std::time::Duration::from_secs(3)).connect(&url).await.ok()
        }) else {
            return;
        };
        let entity_id: Option<u64> = sqlx::query_scalar("SELECT entity_id FROM catalog_product_entity WHERE sku = 'SAMPLE-SKU-0000'").fetch_optional(&pool).await.unwrap();
        let Some(entity_id) = entity_id else { return };

        let state = WebState::new(pool).await.unwrap();
        let app = router(state);
        let response = app.oneshot(Request::builder().uri(format!("/product/{entity_id}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
