use axum::body::Body;
use axum::extract::{MatchedPath, Request};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use std::time::Instant;

/// Adds Go's exact timing headers (`X-Page-Generation-Time-ms`,
/// `X-Page-Generation-Time-μs`, `X-Page-Generation-Time`, `Server-Timing`)
/// to every response.
pub async fn timing_headers(req: Request, next: Next) -> Response {
    let start = Instant::now();
    let mut response = next.run(req).await;
    let elapsed = start.elapsed();
    let ms = elapsed.as_secs_f64() * 1000.0;

    let headers = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&format!("{ms:.3}")) {
        headers.insert("X-Page-Generation-Time-ms", v);
    }
    // Go's original header name is "X-Page-Generation-Time-μs" -- a literal
    // Greek mu, which is NOT a valid HTTP header name per RFC 7230 (token
    // chars are ASCII-only). Go's net/http is lenient enough to send it
    // anyway; Rust's `http` crate correctly rejects it at the type level
    // (constructing that HeaderName panics). This isn't behavior worth
    // replicating -- it's an RFC violation that happens to work by accident
    // on a permissive server -- so this port uses the ASCII equivalent.
    headers.insert("X-Page-Generation-Time-Micros", HeaderValue::from(elapsed.as_micros() as u64));
    if let Ok(v) = HeaderValue::from_str(&format!("{elapsed:?}")) {
        headers.insert("X-Page-Generation-Time", v);
    }
    if let Ok(v) = HeaderValue::from_str(&format!("app;dur={ms:.3};desc=\"gogento-rust Response Time\"")) {
        headers.insert("Server-Timing", v);
    }
    response
}

/// Paths exempt from `/api` auth. Deliberately narrow: `/api/products/flat`
/// etc. are NOT in this list, so they require auth by default even though
/// the base `/api/products` listing doesn't.
pub const AUTH_SKIP_PATHS: [&str; 4] = ["/health", "/api/products", "/api/products/{id}", "/graphql"];

fn is_skipped(req: &Request) -> bool {
    req.extensions()
        .get::<MatchedPath>()
        .map(|p| AUTH_SKIP_PATHS.contains(&p.as_str()))
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct BasicAuthConfig {
    pub user: String,
    pub pass: String,
}

/// HTTP Basic auth, matching Go's default `AUTH_TYPE=basic` mode.
pub async fn basic_auth(
    axum::extract::State(cfg): axum::extract::State<BasicAuthConfig>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if is_skipped(&req) {
        return Ok(next.run(req).await);
    }
    let Some(auth_header) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if !validate_basic_auth(auth_header, &cfg.user, &cfg.pass) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

fn validate_basic_auth(header_value: &str, expected_user: &str, expected_pass: &str) -> bool {
    let Some(encoded) = header_value.strip_prefix("Basic ") else { return false };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else { return false };
    let Ok(decoded) = String::from_utf8(decoded) else { return false };
    let Some((user, pass)) = decoded.split_once(':') else { return false };
    user == expected_user && pass == expected_pass
}

#[derive(Debug, Clone)]
pub struct KeyAuthConfig {
    pub api_key: String,
}

/// A static API-key check, matching Go's `AUTH_TYPE=key` mode (checks
/// `Authorization: Bearer <key>` or an `X-API-Key` header against `API_KEY`).
pub async fn key_auth(axum::extract::State(cfg): axum::extract::State<KeyAuthConfig>, req: Request, next: Next) -> Result<Response, StatusCode> {
    if is_skipped(&req) {
        return Ok(next.run(req).await);
    }
    let provided = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")));
    if provided != Some(cfg.api_key.as_str()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

/// A no-op passthrough used when `AUTH_TYPE` is anything else this project
/// doesn't specifically special-case, or as the base layer wired before
/// selecting basic/key at startup.
pub async fn no_auth(req: Request<Body>, next: Next) -> Response {
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn timing_headers_are_set_on_every_response() {
        let app: Router = Router::new().route("/ping", get(|| async { "pong" })).layer(axum::middleware::from_fn(timing_headers));

        let response = app.oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap()).await.unwrap();

        assert!(response.headers().contains_key("X-Page-Generation-Time-ms"));
        assert!(response.headers().contains_key("X-Page-Generation-Time-Micros"));
        assert!(response.headers().contains_key("X-Page-Generation-Time"));
        assert!(response.headers().contains_key("Server-Timing"));
        let server_timing = response.headers().get("Server-Timing").unwrap().to_str().unwrap();
        assert!(server_timing.starts_with("app;dur="));
    }

    #[test]
    fn auth_skip_paths_match_go_exactly() {
        assert_eq!(AUTH_SKIP_PATHS, ["/health", "/api/products", "/api/products/{id}", "/graphql"]);
    }

    #[test]
    fn validate_basic_auth_accepts_correct_credentials() {
        let header = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("admin:secret"));
        assert!(validate_basic_auth(&header, "admin", "secret"));
    }

    #[test]
    fn validate_basic_auth_rejects_wrong_password() {
        let header = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("admin:wrong"));
        assert!(!validate_basic_auth(&header, "admin", "secret"));
    }

    #[test]
    fn validate_basic_auth_rejects_missing_prefix() {
        assert!(!validate_basic_auth("Bearer sometoken", "admin", "secret"));
    }

    #[test]
    fn validate_basic_auth_rejects_invalid_base64() {
        assert!(!validate_basic_auth("Basic not-valid-base64!!!", "admin", "secret"));
    }

    #[test]
    fn validate_basic_auth_rejects_missing_colon() {
        let header = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("no-colon-here"));
        assert!(!validate_basic_auth(&header, "admin", "secret"));
    }

    fn basic_auth_app() -> Router {
        Router::new()
            .route("/api/products", get(|| async { "ok" }))
            .route("/api/products/{id}", get(|| async { "ok" }))
            .route("/api/products/flat", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                BasicAuthConfig { user: "admin".into(), pass: "secret".into() },
                basic_auth,
            ))
    }

    #[tokio::test]
    async fn skip_listed_path_bypasses_auth_entirely() {
        let response = basic_auth_app().oneshot(Request::builder().uri("/api/products").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn skip_listed_path_with_param_bypasses_auth() {
        let response = basic_auth_app().oneshot(Request::builder().uri("/api/products/42").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_skip_listed_path_requires_auth() {
        // /api/products/flat is deliberately NOT in the skip-list (see Go parity note).
        let response = basic_auth_app().oneshot(Request::builder().uri("/api/products/flat").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_skip_listed_path_with_valid_credentials_succeeds() {
        let header = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("admin:secret"));
        let response = basic_auth_app()
            .oneshot(Request::builder().uri("/api/products/flat").header(header::AUTHORIZATION, header).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    fn key_auth_app(api_key: &str) -> Router {
        Router::new()
            .route("/api/products", get(|| async { "ok" }))
            .route("/api/products/flat", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(KeyAuthConfig { api_key: api_key.to_string() }, key_auth))
    }

    #[tokio::test]
    async fn key_auth_skip_listed_path_bypasses_check() {
        let response = key_auth_app("secret-key").oneshot(Request::builder().uri("/api/products").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn key_auth_rejects_missing_key() {
        let response = key_auth_app("secret-key").oneshot(Request::builder().uri("/api/products/flat").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn key_auth_accepts_x_api_key_header() {
        let response = key_auth_app("secret-key")
            .oneshot(Request::builder().uri("/api/products/flat").header("X-API-Key", "secret-key").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn key_auth_accepts_bearer_authorization_header() {
        let response = key_auth_app("secret-key")
            .oneshot(Request::builder().uri("/api/products/flat").header(header::AUTHORIZATION, "Bearer secret-key").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn key_auth_rejects_wrong_key() {
        let response = key_auth_app("secret-key")
            .oneshot(Request::builder().uri("/api/products/flat").header("X-API-Key", "wrong-key").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// The skip-list match relies on `MatchedPath` resolving to the FULL
    /// path pattern including any `.nest()` prefix -- this is how the real
    /// app wires the auth layer (see `build_router`), so it's worth pinning
    /// specifically rather than only testing top-level routes.
    #[tokio::test]
    async fn skip_list_still_resolves_correctly_when_router_is_nested() {
        let inner = Router::new()
            .route("/products", get(|| async { "ok" }))
            .route("/products/flat", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                BasicAuthConfig { user: "admin".into(), pass: "secret".into() },
                basic_auth,
            ));
        let app = Router::new().nest("/api", inner);

        let skipped = app.clone().oneshot(Request::builder().uri("/api/products").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(skipped.status(), StatusCode::OK, "nested /api/products must still be recognized as skip-listed");

        let not_skipped = app.oneshot(Request::builder().uri("/api/products/flat").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(not_skipped.status(), StatusCode::UNAUTHORIZED, "nested /api/products/flat must still require auth");
    }
}
