use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use repository::product_db::{self, ProductInput as DbProductInput};
use serde::Deserialize;
use serde_json::json;
use std::time::Instant;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update).delete(delete_one))
        .route("/flat", get(flat))
        .route("/full", get(flat))
        .route("/flat/{ids}", get(flat_by_ids))
}

fn duration_response(status: StatusCode, mut body: serde_json::Map<String, serde_json::Value>, start: Instant) -> Response {
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    body.insert("request_duration_ms".to_string(), json!(ms as i64));
    let mut response = (status, Json(serde_json::Value::Object(body))).into_response();
    if let Ok(v) = HeaderValue::from_str(&(ms as i64).to_string()) {
        response.headers_mut().insert("X-Request-Duration-ms", v);
    }
    response
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    limit: usize,
}

async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let start = Instant::now();
    match product_db::find_all(&state.pool, q.limit).await {
        Ok(products) => {
            let mut body = serde_json::Map::new();
            body.insert("products".to_string(), json!(products));
            body.insert("count".to_string(), json!(products_len(&products)));
            duration_response(StatusCode::OK, body, start)
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), start),
    }
}

fn products_len<T>(v: &[T]) -> usize {
    v.len()
}

fn error_response(status: StatusCode, message: &str, start: Instant) -> Response {
    let mut body = serde_json::Map::new();
    body.insert("error".to_string(), json!(message));
    duration_response(status, body, start)
}

async fn get_one(State(state): State<AppState>, Path(id): Path<u64>) -> Response {
    let start = Instant::now();
    match product_db::find_by_id(&state.pool, id).await {
        Ok(Some(product)) => {
            let mut body = serde_json::Map::new();
            body.insert("product".to_string(), json!(product));
            duration_response(StatusCode::OK, body, start)
        }
        Ok(None) => error_response(StatusCode::NOT_FOUND, "product not found", start),
        Err(e) => error_response(StatusCode::NOT_FOUND, &e.to_string(), start),
    }
}

#[derive(Debug, Deserialize)]
pub struct ProductInput {
    #[serde(default)]
    pub attribute_set_id: u16,
    #[serde(default)]
    pub type_id: String,
    #[serde(default)]
    pub sku: String,
    #[serde(default)]
    pub has_options: i16,
    #[serde(default)]
    pub required_options: u16,
}

impl From<ProductInput> for DbProductInput {
    fn from(i: ProductInput) -> Self {
        DbProductInput {
            attribute_set_id: i.attribute_set_id,
            type_id: i.type_id,
            sku: i.sku,
            has_options: i.has_options,
            required_options: i.required_options,
        }
    }
}

async fn create(State(state): State<AppState>, Json(input): Json<ProductInput>) -> Response {
    let start = Instant::now();
    let db_input: DbProductInput = input.into();
    match product_db::create(&state.pool, &db_input).await {
        Ok(id) => {
            let mut body = serde_json::Map::new();
            body.insert("product".to_string(), json!({ "entity_id": id, "sku": db_input.sku, "type_id": db_input.type_id }));
            duration_response(StatusCode::CREATED, body, start)
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), start),
    }
}

async fn update(State(state): State<AppState>, Path(id): Path<u64>, Json(input): Json<ProductInput>) -> Response {
    let start = Instant::now();
    let db_input: DbProductInput = input.into();
    match product_db::update(&state.pool, id, &db_input).await {
        Ok(true) => {
            let mut body = serde_json::Map::new();
            body.insert("product".to_string(), json!({ "entity_id": id, "sku": db_input.sku, "type_id": db_input.type_id }));
            duration_response(StatusCode::OK, body, start)
        }
        Ok(false) => error_response(StatusCode::NOT_FOUND, "product not found", start),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), start),
    }
}

async fn delete_one(State(state): State<AppState>, Path(id): Path<u64>) -> Response {
    let start = Instant::now();
    match product_db::delete(&state.pool, id).await {
        Ok(true) => (StatusCode::NO_CONTENT, ()).into_response(),
        Ok(false) => error_response(StatusCode::NOT_FOUND, "product not found", start),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), start),
    }
}

async fn flat(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let start = Instant::now();
    let force_bypass = !state.product_flat_cache_enabled;
    match product_db::fetch_flat_list(&state.pool, &state.product_cache, &state.product_code_map, 0, q.limit, force_bypass).await {
        Ok(products) => {
            let mut body = serde_json::Map::new();
            body.insert("products".to_string(), json!(products));
            body.insert("count".to_string(), json!(products.len()));
            duration_response(StatusCode::OK, body, start)
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), start),
    }
}

async fn flat_by_ids(State(state): State<AppState>, Path(ids_param): Path<String>) -> Response {
    let start = Instant::now();
    let ids: Vec<u64> = ids_param.split(',').filter_map(|s| s.trim().parse().ok()).collect();

    match product_db::fetch_flat_by_ids(&state.pool, &state.product_cache, &state.product_code_map, 0, &ids).await {
        Ok(products) => {
            let mut body = serde_json::Map::new();
            body.insert("products".to_string(), json!(products));
            body.insert("count".to_string(), json!(products.len()));
            duration_response(StatusCode::OK, body, start)
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_state;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn list_returns_products_and_a_matching_count() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/?limit=3").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["products"].as_array().unwrap().len(), 3);
        assert_eq!(json["count"], 3);
        assert!(json["request_duration_ms"].is_i64());
    }

    #[tokio::test]
    async fn get_one_returns_404_for_unknown_id() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/999999999").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = body_json(response).await;
        assert_eq!(json["error"], "product not found");
    }

    #[tokio::test]
    async fn full_crud_lifecycle_through_the_router() {
        let Some(state) = test_state().await else { return };
        let pool = state.pool.clone();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku LIKE 'RUST-API-REST-CRUD-TEST%'").execute(&pool).await.unwrap();

        let app = router().with_state(state);

        let create_body = r#"{"attribute_set_id":4,"type_id":"simple","sku":"RUST-API-REST-CRUD-TEST","has_options":0,"required_options":0}"#;
        let create_res = app
            .clone()
            .oneshot(Request::builder().method(Method::POST).uri("/").header("content-type", "application/json").body(Body::from(create_body)).unwrap())
            .await
            .unwrap();
        assert_eq!(create_res.status(), StatusCode::CREATED);
        let created = body_json(create_res).await;
        let id = created["product"]["entity_id"].as_u64().unwrap();

        let get_res = app.clone().oneshot(Request::builder().uri(format!("/{id}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(get_res.status(), StatusCode::OK);

        let update_body = r#"{"attribute_set_id":4,"type_id":"simple","sku":"RUST-API-REST-CRUD-TEST-RENAMED","has_options":0,"required_options":0}"#;
        let update_res = app
            .clone()
            .oneshot(Request::builder().method(Method::PUT).uri(format!("/{id}")).header("content-type", "application/json").body(Body::from(update_body)).unwrap())
            .await
            .unwrap();
        assert_eq!(update_res.status(), StatusCode::OK);
        let updated = body_json(update_res).await;
        assert_eq!(updated["product"]["sku"], "RUST-API-REST-CRUD-TEST-RENAMED");

        let delete_res = app.clone().oneshot(Request::builder().method(Method::DELETE).uri(format!("/{id}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(delete_res.status(), StatusCode::NO_CONTENT);

        let gone_res = app.clone().oneshot(Request::builder().method(Method::DELETE).uri(format!("/{id}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(gone_res.status(), StatusCode::NOT_FOUND, "deleting an already-gone product reports not-found");
    }

    #[tokio::test]
    async fn flat_endpoint_returns_flattened_attributes() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/flat?limit=2").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        let products = json["products"].as_array().unwrap();
        assert_eq!(products.len(), 2);
        assert!(products[0].get("sku").is_some());
    }

    #[tokio::test]
    async fn flat_by_ids_returns_only_matching_ids_and_ignores_garbage() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/flat/1,2,not-a-number,999999999").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        // "not-a-number" is skipped, "999999999" doesn't exist -- only 1 and 2 resolve.
        assert_eq!(json["count"], 2);
    }
}
