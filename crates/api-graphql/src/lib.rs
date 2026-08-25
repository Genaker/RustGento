//! GraphQL layer -- mirrors GoGento's `graphql/` package: schema, resolvers,
//! and the axum wiring for `/graphql` + `/playground`.

pub mod context;
pub mod models;
pub mod pagination;
pub mod query;
pub mod schema;
pub mod store;
pub mod uid;

#[cfg(test)]
mod test_support;

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use context::{GraphQLContext, StoreId};
use schema::GogentoSchema;

async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/graphql")))
}

async fn graphql_handler(
    State(schema): State<GogentoSchema>,
    headers: axum::http::HeaderMap,
    axum::extract::RawQuery(query_string): axum::extract::RawQuery,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let mut request = req.into_inner();

    let header_store = headers.get("Store").and_then(|v| v.to_str().ok());
    let body_store_owned: Option<String> = request
        .variables
        .get("__Store")
        .and_then(|v| v.clone().into_json().ok())
        .and_then(|j| j.as_str().map(str::to_string).or_else(|| j.as_i64().map(|n| n.to_string())));
    let query_store: Option<String> =
        query_string.as_deref().and_then(|qs| url::form_urlencoded::parse(qs.as_bytes()).find(|(k, _)| k == "__Store").map(|(_, v)| v.into_owned()));

    let store_id = store::resolve_store_id(header_store, body_store_owned.as_deref(), query_store.as_deref());
    request = request.data(StoreId(store_id));

    schema.execute(request).await.into()
}

/// Builds the `/graphql` (POST) + `/playground` (GET) routes, unauthenticated
/// (matching Go's root-route registration -- GraphQL sits outside the `/api`
/// auth group).
pub fn router(context: GraphQLContext) -> Router {
    let schema = schema::build_schema(context);
    Router::new().route("/graphql", get(graphql_playground).post(graphql_handler)).route("/playground", get(graphql_playground)).with_state(schema)
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn live_router() -> Option<Router> {
        let pool = {
            let url = std::env::var("GOGENTO_TEST_DATABASE_URL").unwrap_or_else(|_| "mysql://magento:magento@127.0.0.1:3309/magento".to_string());
            sqlx::mysql::MySqlPoolOptions::new().acquire_timeout(std::time::Duration::from_secs(3)).connect(&url).await.ok()?
        };
        let product_code_map = repository::product_db::load_attribute_code_map(&pool).await.ok()?;
        let category_meta = repository::category_db::load_attribute_meta(&pool).await.ok()?;
        Some(router(GraphQLContext {
            pool,
            product_cache: std::sync::Arc::new(repository::FlatCache::new()),
            category_cache: std::sync::Arc::new(repository::FlatCache::new()),
            product_code_map: std::sync::Arc::new(product_code_map),
            category_meta: std::sync::Arc::new(category_meta),
            product_flat_cache_enabled: true,
        }))
    }

    async fn post_graphql(app: Router, uri: &str, headers: &[(&str, &str)], body: &str) -> serde_json::Value {
        let mut builder = Request::builder().method("POST").uri(uri).header("content-type", "application/json");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let response = app.oneshot(builder.body(Body::from(body.to_string())).unwrap()).await.unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn graphql_endpoint_executes_a_query_over_http() {
        let Some(app) = live_router().await else { return };
        let json = post_graphql(app, "/graphql", &[], r#"{"query":"query { categories { entity_id } }"}"#).await;
        assert!(json["errors"].is_null(), "errors: {:?}", json["errors"]);
        assert!(!json["data"]["categories"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn graphql_endpoint_resolves_store_id_from_header() {
        let Some(app) = live_router().await else { return };
        // Store ID doesn't change *what* categories() returns in this port
        // (store scoping is on attribute values, not row visibility), so
        // this just pins that the header is accepted without erroring --
        // full store-scoped-value assertions would need per-store seed data.
        let json = post_graphql(app, "/graphql", &[("Store", "1")], r#"{"query":"query { categories { entity_id } }"}"#).await;
        assert!(json["errors"].is_null(), "errors: {:?}", json["errors"]);
    }

    #[tokio::test]
    async fn graphql_endpoint_resolves_store_id_from_query_param() {
        let Some(app) = live_router().await else { return };
        let json = post_graphql(app, "/graphql?__Store=1", &[], r#"{"query":"query { categories { entity_id } }"}"#).await;
        assert!(json["errors"].is_null(), "errors: {:?}", json["errors"]);
    }

    #[tokio::test]
    async fn graphql_endpoint_resolves_store_id_from_body_variable() {
        let Some(app) = live_router().await else { return };
        let json = post_graphql(app, "/graphql", &[], r#"{"query":"query { categories { entity_id } }","variables":{"__Store":1}}"#).await;
        assert!(json["errors"].is_null(), "errors: {:?}", json["errors"]);
    }

    #[tokio::test]
    async fn playground_route_serves_html() {
        let Some(app) = live_router().await else { return };
        let response = app.oneshot(Request::builder().uri("/playground").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let content_type = response.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(content_type.contains("html"));
    }
}
