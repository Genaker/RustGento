//! REST API layer -- products/categories/stock endpoints plus the `/api`
//! auth middleware group.

pub mod categories;
pub mod middleware;
pub mod products;
pub mod state;
pub mod stock;

#[cfg(test)]
mod test_support;

use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use config::AuthType;
use state::AppState;
use tower_http::compression::CompressionLayer;
use tower_http::decompression::RequestDecompressionLayer;

async fn health() -> StatusCode {
    StatusCode::OK
}

/// Builds the full application router: unauthenticated `/health` and
/// `/api/products`/`/api/products/{id}` (per the auth skip-list), the rest of
/// `/api/*` behind auth, all wrapped in the timing-header middleware and
/// gzip compression/decompression.
pub fn build_router(state: AppState) -> Router {
    let mut api = Router::new().nest("/products", products::router()).merge(categories::router()).nest("/stock", stock::router());

    api = match AuthType::from_env() {
        AuthType::Basic => {
            let cfg = middleware::BasicAuthConfig { user: std::env::var("API_USER").unwrap_or_default(), pass: std::env::var("API_PASS").unwrap_or_default() };
            api.layer(axum::middleware::from_fn_with_state(cfg, middleware::basic_auth))
        }
        AuthType::Key | AuthType::Token => {
            let cfg = middleware::KeyAuthConfig { api_key: std::env::var("API_KEY").unwrap_or_default() };
            api.layer(axum::middleware::from_fn_with_state(cfg, middleware::key_auth))
        }
    };

    Router::new()
        .route("/health", get(health))
        .nest("/api", api)
        .layer(axum::middleware::from_fn(middleware::timing_headers))
        .layer(CompressionLayer::new())
        .layer(RequestDecompressionLayer::new())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::Mutex;
    use tower::ServiceExt;

    // `build_router` reads AUTH_TYPE/API_USER/API_PASS from the process
    // environment, which `cargo test`'s parallel threads share -- any test
    // that sets these must hold this lock first to avoid racing a future
    // test that also touches them (see the same pattern in `config`).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn health_is_unauthenticated_and_timing_instrumented() {
        let _guard = ENV_LOCK.lock().unwrap();
        let Some(state) = test_support::test_state().await else { return };
        let app = build_router(state);
        let response = app.oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("X-Page-Generation-Time-ms"), "the full router must apply the timing middleware");
    }

    #[tokio::test]
    async fn products_skip_listed_route_is_reachable_without_auth_through_the_full_router() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("AUTH_TYPE", "basic");
        std::env::set_var("API_USER", "admin");
        std::env::set_var("API_PASS", "secret");

        let Some(state) = test_support::test_state().await else { return };
        let app = build_router(state);
        let response = app.oneshot(Request::builder().uri("/api/products?limit=1").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "the auth skip-list must still apply once nested under the full app");

        std::env::remove_var("AUTH_TYPE");
        std::env::remove_var("API_USER");
        std::env::remove_var("API_PASS");
    }
}
