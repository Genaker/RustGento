//! Realtime HMAC-gated price/inventory API, for latency-sensitive callers
//! that don't want a full GraphQL round trip.

pub mod hmac_auth;
pub mod price;
pub mod stock;

#[cfg(test)]
mod test_support;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;

#[derive(Clone)]
pub struct RealtimeState {
    pub pool: MySqlPool,
    /// `MAGENTO_CRYPT_KEY` -- when empty, the combined `/price-inventory`
    /// endpoint's HMAC gate is skipped entirely, matching Go (`if
    /// MAGENTO_CRYPT_KEY != "" { ...verify... }`).
    pub crypt_key: String,
}

pub fn router(state: RealtimeState) -> Router {
    Router::new()
        .route("/price-inventory", get(price_inventory))
        .route("/price", get(price_only))
        .route("/stock", get(stock_only))
        .route("/tier-prices", get(tier_prices))
        .with_state(state)
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

#[derive(Debug, Deserialize)]
struct SkuQuery {
    sku: String,
}

async fn price_only(State(state): State<RealtimeState>, Query(q): Query<SkuQuery>) -> Response {
    match price::lowest_price_by_sku(&state.pool, &q.sku).await {
        Ok(Some(p)) => Json(json!({ "sku": q.sku, "price": p })).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "sku not found"),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn stock_only(State(state): State<RealtimeState>, Query(q): Query<SkuQuery>) -> Response {
    match stock::stock_by_sku(&state.pool, &q.sku).await {
        Ok(Some(s)) => Json(json!({ "sku": q.sku, "qty": s.qty, "is_in_stock": s.is_in_stock == 1 })).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "sku not found"),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Tier pricing is a non-goal for this port (no
/// `catalog_product_entity_tier_price` table in the seeded schema) --
/// always returns an empty list, present for endpoint-shape parity rather
/// than omitted.
async fn tier_prices(Query(q): Query<SkuQuery>) -> Response {
    Json(json!({ "sku": q.sku, "tier_prices": Vec::<serde_json::Value>::new() })).into_response()
}

#[derive(Debug, Deserialize)]
struct PriceInventoryQuery {
    sku: String,
}

/// The combined, HMAC-gated endpoint: verifies `X-Customer-Sig` against
/// `X-Customer-ID` (skipped entirely if `crypt_key` is empty, matching Go),
/// then fetches price and stock concurrently.
async fn price_inventory(State(state): State<RealtimeState>, headers: axum::http::HeaderMap, Query(q): Query<PriceInventoryQuery>) -> Response {
    if !state.crypt_key.is_empty() {
        let customer_id = headers.get("X-Customer-ID").and_then(|v| v.to_str().ok());
        let signature = headers.get("X-Customer-Sig").and_then(|v| v.to_str().ok());
        match (customer_id, signature) {
            (Some(customer_id), Some(signature)) if hmac_auth::verify(state.crypt_key.as_bytes(), customer_id, signature) => {}
            _ => return error(StatusCode::UNAUTHORIZED, "invalid or missing signature"),
        }
    }

    let (price_result, stock_result) = tokio::join!(price::lowest_price_by_sku(&state.pool, &q.sku), stock::stock_by_sku(&state.pool, &q.sku));

    match (price_result, stock_result) {
        (Ok(price), Ok(stock)) => Json(json!({
            "sku": q.sku,
            "price": price,
            "qty": stock.as_ref().and_then(|s| s.qty),
            "is_in_stock": stock.as_ref().map(|s| s.is_in_stock == 1),
        }))
        .into_response(),
        (Err(e), _) | (_, Err(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn live_state(crypt_key: &str) -> Option<RealtimeState> {
        let pool = test_support::test_pool().await?;
        Some(RealtimeState { pool, crypt_key: crypt_key.to_string() })
    }

    #[tokio::test]
    async fn price_endpoint_returns_404_for_unknown_sku() {
        let Some(state) = live_state("").await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/price?sku=DEFINITELY-NOT-A-REAL-SKU").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn stock_endpoint_returns_stock_for_a_known_sku() {
        let Some(state) = live_state("").await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/stock?sku=SAMPLE-SKU-0000").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["sku"], "SAMPLE-SKU-0000");
    }

    #[tokio::test]
    async fn tier_prices_endpoint_always_returns_an_empty_stub_list() {
        let Some(state) = live_state("").await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/tier-prices?sku=SAMPLE-SKU-0000").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert!(json["tier_prices"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn price_inventory_is_open_when_crypt_key_is_unset() {
        let Some(state) = live_state("").await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/price-inventory?sku=SAMPLE-SKU-0000").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "no crypt key configured means the HMAC gate is skipped entirely, matching Go");
    }

    #[tokio::test]
    async fn price_inventory_rejects_missing_signature_when_crypt_key_is_set() {
        let Some(state) = live_state("test-crypt-key").await else { return };
        let app = router(state);
        let response = app.oneshot(Request::builder().uri("/price-inventory?sku=SAMPLE-SKU-0000").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn price_inventory_rejects_wrong_signature_when_crypt_key_is_set() {
        let Some(state) = live_state("test-crypt-key").await else { return };
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/price-inventory?sku=SAMPLE-SKU-0000")
                    .header("X-Customer-ID", "42")
                    .header("X-Customer-Sig", "0000000000000000000000000000000000000000000000000000000000000000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn price_inventory_accepts_a_correctly_signed_request() {
        let Some(state) = live_state("test-crypt-key").await else { return };
        let app = router(state);
        let sig = {
            use hmac::Mac;
            let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(b"test-crypt-key").unwrap();
            mac.update(b"42");
            hex::encode(mac.finalize().into_bytes())
        };
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/price-inventory?sku=SAMPLE-SKU-0000")
                    .header("X-Customer-ID", "42")
                    .header("X-Customer-Sig", sig)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["sku"], "SAMPLE-SKU-0000");
    }
}
