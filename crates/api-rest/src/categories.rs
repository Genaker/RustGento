use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use repository::category_db;
use serde::Deserialize;
use serde_json::json;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/categories", get(list))
        .route("/categories/full", get(list))
        .route("/category/tree", get(tree))
        .route("/category/cache", get(cache_all))
        .route("/category/cache/{id}", get(cache_one))
        .route("/category/{ids}/flat", get(flat_by_ids))
        .route("/category/{id}", get(get_one))
}

#[derive(Debug, Deserialize)]
pub struct StoreQuery {
    #[serde(default)]
    store_id: u16,
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

async fn list(State(state): State<AppState>, Query(q): Query<StoreQuery>) -> Response {
    match category_db::fetch_flat_list(&state.pool, &state.category_cache, &state.category_meta, q.store_id).await {
        Ok(categories) => Json(json!({ "categories": categories, "total": categories.len() })).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_one(State(state): State<AppState>, Path(id): Path<u64>, Query(q): Query<StoreQuery>) -> Response {
    match category_db::fetch_flat_by_id(&state.pool, &state.category_cache, &state.category_meta, q.store_id, id).await {
        Ok(Some(cat)) => Json(cat).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "category not found"),
        Err(_) => error(StatusCode::NOT_FOUND, "category not found"),
    }
}

async fn flat_by_ids(State(state): State<AppState>, Path(ids_param): Path<String>, Query(q): Query<StoreQuery>) -> Response {
    let ids: Vec<u64> = ids_param.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    if ids.is_empty() {
        return error(StatusCode::BAD_REQUEST, "no valid category ids");
    }
    match category_db::fetch_flat_by_ids(&state.pool, &state.category_cache, &state.category_meta, q.store_id, &ids).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn tree(State(state): State<AppState>) -> Response {
    match category_db::build_tree(&state.pool, &state.category_meta, 0).await {
        Ok(tree) => Json(tree).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Introspects the in-process category flat cache for a store -- since this
/// port's cache is filled lazily on fetch (see [`repository::FlatCache`]),
/// an empty result here just means nothing has been fetched into that store
/// yet, not that categories don't exist.
async fn cache_all(State(state): State<AppState>, Query(q): Query<StoreQuery>) -> Response {
    if state.category_cache.len_for_store(q.store_id) == 0 {
        return error(StatusCode::NOT_FOUND, "no cache for store");
    }
    // Populate-then-read: a cache introspection endpoint is a reasonable
    // place to also warm it, mirroring how Go's cache is filled as a side
    // effect of normal fetch traffic.
    match category_db::fetch_flat_list(&state.pool, &state.category_cache, &state.category_meta, q.store_id).await {
        Ok(categories) => Json(categories).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn cache_one(State(state): State<AppState>, Path(id): Path<u64>, Query(q): Query<StoreQuery>) -> Response {
    match category_db::fetch_flat_by_id(&state.pool, &state.category_cache, &state.category_meta, q.store_id, id).await {
        Ok(Some(cat)) => Json(cat).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "not found in cache"),
        Err(_) => error(StatusCode::NOT_FOUND, "not found in cache"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_state;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn list_returns_seeded_categories() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/categories").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert!(json["total"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn get_one_returns_404_for_unknown_id() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/999999999").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_one_returns_the_category_for_a_known_id() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/1").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["entity_id"], 1);
    }

    #[tokio::test]
    async fn flat_by_ids_rejects_when_no_ids_parse() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/not-a-number,also-not/flat").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn flat_by_ids_returns_matching_categories() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/1,999999999/flat").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json.as_array().unwrap().len(), 1, "only entity_id=1 exists");
    }

    #[tokio::test]
    async fn tree_returns_a_rooted_hierarchy() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/tree").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert!(!json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cache_all_is_not_found_before_anything_is_fetched_then_found_after() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);

        let before = app.clone().oneshot(Request::builder().uri("/category/cache").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(before.status(), StatusCode::NOT_FOUND, "cache starts empty for a fresh state");

        // Warm the cache via a direct fetch, then cache_all should find it.
        let _ = app.clone().oneshot(Request::builder().uri("/categories").body(Body::empty()).unwrap()).await.unwrap();
        let after = app.oneshot(Request::builder().uri("/category/cache").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(after.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cache_one_is_read_through_and_needs_no_prior_warm_up() {
        // Unlike `cache_all` (a pure introspection that 404s until something
        // populates the cache), `cache_one` fetches on a miss -- so a known
        // category is found immediately, with no separate warm-up step.
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/cache/1").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cache_one_returns_404_for_a_nonexistent_category() {
        let Some(state) = test_state().await else { return };
        let app = router().with_state(state);
        let response = app.oneshot(Request::builder().uri("/category/cache/999999999").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
