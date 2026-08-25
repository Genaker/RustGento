//! REST/GraphQL server binary: connect to MySQL, build the router (REST
//! under `/api`, GraphQL at `/graphql`, both timing-instrumented), and serve.

use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    config::load_dotenv();

    let db_config = config::DbConfig::from_env();
    let pool = db_config.build_pool().await?;
    tracing::info!("connected to database at {}:{}", db_config.host, db_config.port);

    let rest_state = api_rest::state::AppState::new(pool.clone()).await?;

    let graphql_context = api_graphql::context::GraphQLContext {
        pool,
        product_cache: Arc::new(repository::FlatCache::new()),
        category_cache: Arc::new(repository::FlatCache::new()),
        product_code_map: Arc::new(repository::product_db::load_attribute_code_map(&rest_state.pool).await?),
        category_meta: Arc::new(repository::category_db::load_attribute_meta(&rest_state.pool).await?),
        product_flat_cache_enabled: rest_state.product_flat_cache_enabled,
    };

    // Realtime endpoints: mounted at /api/realtime but, as a deliberate
    // simplification, outside the standard basic/key `/api` auth layer --
    // wiring one shared auth middleware across independent crates with
    // different `State` types isn't worth it here, since the endpoints'
    // actual distinguishing feature is their own HMAC gate (skipped
    // entirely when MAGENTO_CRYPT_KEY is unset).
    let realtime_state = api_realtime::RealtimeState { pool: rest_state.pool.clone(), crypt_key: std::env::var("MAGENTO_CRYPT_KEY").unwrap_or_default() };
    let realtime_router = axum::Router::new().nest("/api/realtime", api_realtime::router(realtime_state));

    // GraphQL is mounted unauthenticated at the root, sitting outside the
    // `/api` auth group. All these routers are already fully-stated
    // (`Router<()>`), so they merge cleanly despite coming from independent
    // crates with their own state types.
    let app = api_rest::build_router(rest_state).merge(api_graphql::router(graphql_context)).merge(realtime_router);

    let port = config::app_port();
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("gogento-rust server listening on :{port}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
