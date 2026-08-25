use crate::state::AppState;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use entity::{StockItem, DEFAULT_STOCK_ID};
use serde::Deserialize;
use serde_json::json;
use sqlx::MySqlPool;
use std::time::Instant;

pub fn router() -> Router<AppState> {
    Router::new().route("/import", post(import))
}

#[derive(Debug, Deserialize)]
pub struct StockItemInput {
    pub sku: String,
    pub qty: Option<f64>,
    pub is_in_stock: Option<u16>,
    pub manage_stock: Option<u16>,
    pub min_qty: Option<f64>,
    pub min_sale_qty: Option<f64>,
    pub max_sale_qty: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ImportBody {
    pub items: Vec<StockItemInput>,
    #[serde(default)]
    pub batch_size: usize,
}

/// Result of a JSON stock import: how many upserted, how many skipped
/// (blank/unknown SKU), and why. Matches Go's `StockImportResult`.
#[derive(Debug, Default)]
pub struct StockImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

/// Resolves SKUs to entity IDs and upserts stock items, matching Go's
/// `ImportStockJSON` -- the JSON-body twin of the CSV import path's stock
/// collector, always CE-style (`entity_id`, not EE `row_id`).
pub async fn import_stock_json(pool: &MySqlPool, items: Vec<StockItemInput>, batch_size: usize) -> Result<StockImportResult, sqlx::Error> {
    let batch_size = if batch_size == 0 { 500 } else { batch_size };
    let mut result = StockImportResult::default();

    let skus: Vec<String> = items.iter().filter(|i| !i.sku.is_empty()).map(|i| i.sku.clone()).collect();
    let sku_to_id = import::lookup_existing_skus(pool, &skus, batch_size).await?;

    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        if item.sku.is_empty() {
            result.skipped += 1;
            result.warnings.push("empty sku, skipping".to_string());
            continue;
        }
        let Some(&product_id) = sku_to_id.get(&item.sku) else {
            result.skipped += 1;
            result.warnings.push(format!("sku={}: product not found", item.sku));
            continue;
        };
        rows.push(StockItem {
            item_id: 0,
            product_id,
            stock_id: DEFAULT_STOCK_ID,
            qty: item.qty,
            min_qty: item.min_qty.unwrap_or(0.0),
            is_qty_decimal: 0,
            backorders: 0,
            min_sale_qty: item.min_sale_qty.unwrap_or(1.0),
            max_sale_qty: item.max_sale_qty.unwrap_or(0.0),
            is_in_stock: item.is_in_stock.unwrap_or(1),
            manage_stock: item.manage_stock.unwrap_or(1),
            website_id: 0,
        });
    }

    result.imported = rows.len();
    if !rows.is_empty() {
        import::flush_stock(pool, &rows, batch_size).await?;
    }
    Ok(result)
}

async fn import(State(state): State<AppState>, Json(body): Json<ImportBody>) -> Response {
    let start = Instant::now();
    if body.items.is_empty() {
        return error(StatusCode::BAD_REQUEST, "items array is required and must not be empty");
    }

    match import_stock_json(&state.pool, body.items, body.batch_size).await {
        Ok(res) => {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let mut response = Json(json!({
                "imported": res.imported,
                "skipped": res.skipped,
                "warnings": res.warnings,
                "request_duration_ms": ms as i64,
            }))
            .into_response();
            if let Ok(v) = HeaderValue::from_str(&(ms as i64).to_string()) {
                response.headers_mut().insert("X-Request-Duration-ms", v);
            }
            response
        }
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_sku_is_skipped_with_a_warning() {
        let Some(pool) = live_pool().await else { return };
        let items = vec![StockItemInput { sku: "".into(), qty: None, is_in_stock: None, manage_stock: None, min_qty: None, min_sale_qty: None, max_sale_qty: None }];
        let result = import_stock_json(&pool, items, 500).await.unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.skipped, 1);
        assert!(result.warnings[0].contains("empty sku"));
    }

    #[tokio::test]
    async fn unknown_sku_is_skipped_with_a_warning() {
        let Some(pool) = live_pool().await else { return };
        let items = vec![StockItemInput {
            sku: "DEFINITELY-NOT-A-REAL-SKU".into(),
            qty: None,
            is_in_stock: None,
            manage_stock: None,
            min_qty: None,
            min_sale_qty: None,
            max_sale_qty: None,
        }];
        let result = import_stock_json(&pool, items, 500).await.unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.skipped, 1);
        assert!(result.warnings[0].contains("product not found"));
    }

    #[tokio::test]
    async fn known_sku_is_imported() {
        let Some(pool) = live_pool().await else { return };
        let items = vec![StockItemInput {
            sku: "SAMPLE-SKU-0000".into(),
            qty: Some(77.0),
            is_in_stock: Some(1),
            manage_stock: None,
            min_qty: None,
            min_sale_qty: None,
            max_sale_qty: None,
        }];
        let result = import_stock_json(&pool, items, 500).await.unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 0);
    }

    async fn live_pool() -> Option<MySqlPool> {
        let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| "mysql://magento:magento@127.0.0.1:3309/magento".to_string());
        sqlx::mysql::MySqlPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(3))
            .connect(&url)
            .await
            .ok()
    }

    mod http {
        use crate::stock::router;
        use crate::test_support::test_state;
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        async fn body_json(response: axum::response::Response) -> serde_json::Value {
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice(&bytes).unwrap()
        }

        #[tokio::test]
        async fn empty_items_array_is_rejected_with_400() {
            let Some(state) = test_state().await else { return };
            let app = router().with_state(state);
            let response = app
                .oneshot(Request::builder().method("POST").uri("/import").header("content-type", "application/json").body(Body::from(r#"{"items":[]}"#)).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn valid_import_returns_200_with_duration_header() {
            let Some(state) = test_state().await else { return };
            let app = router().with_state(state);
            let body = r#"{"items":[{"sku":"SAMPLE-SKU-0000","qty":88}]}"#;
            let response = app
                .oneshot(Request::builder().method("POST").uri("/import").header("content-type", "application/json").body(Body::from(body)).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers().contains_key("X-Request-Duration-ms"));
            let json = body_json(response).await;
            assert_eq!(json["imported"], 1);
        }
    }
}
