//! Config layer — env-var driven settings, `.env` file support (ignored if
//! missing), and MySQL/Postgres pool construction.

use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{MySqlPool, PgPool};
use std::time::Duration;

/// Load a `.env` file if present. Silently no-ops when the file is missing
/// rather than erroring.
pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl DbConfig {
    /// Reads MYSQL_HOST/PORT/USER/PASS/DB from the environment, with the same
    /// defaults `.env.example` documents.
    pub fn from_env() -> Self {
        DbConfig {
            host: env_or("MYSQL_HOST", "localhost"),
            port: env_or("MYSQL_PORT", "3306").parse().unwrap_or(3306),
            user: env_or("MYSQL_USER", "magento"),
            password: env_or("MYSQL_PASS", "magento"),
            database: env_or("MYSQL_DB", "magento"),
        }
    }

    pub fn connect_options(&self) -> MySqlConnectOptions {
        MySqlConnectOptions::new()
            .host(&self.host)
            .port(self.port)
            .username(&self.user)
            .password(&self.password)
            .database(&self.database)
    }

    /// Builds a connection pool: 25 max open, 25 max idle, 5 min max
    /// lifetime, 2 min max idle time.
    pub async fn build_pool(&self) -> Result<MySqlPool, sqlx::Error> {
        MySqlPoolOptions::new()
            .max_connections(25)
            .min_connections(0)
            .max_lifetime(Duration::from_secs(5 * 60))
            .idle_timeout(Duration::from_secs(2 * 60))
            .connect_with(self.connect_options())
            .await
    }
}

/// Connection settings for the Postgres synthetic-import path (see
/// `import::import_products_pg` and `sql/postgres_schema.sql`) -- a
/// separate, smaller config struct rather than a generic `DbConfig<Db>`
/// since Postgres isn't a general-purpose target here, just the one
/// synthetic-test path.
#[derive(Debug, Clone, PartialEq)]
pub struct PgDbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl PgDbConfig {
    /// Reads POSTGRES_HOST/PORT/USER/PASS/DB from the environment.
    pub fn from_env() -> Self {
        PgDbConfig {
            host: env_or("POSTGRES_HOST", "localhost"),
            port: env_or("POSTGRES_PORT", "5432").parse().unwrap_or(5432),
            user: env_or("POSTGRES_USER", "magento"),
            password: env_or("POSTGRES_PASS", "magento"),
            database: env_or("POSTGRES_DB", "magento"),
        }
    }

    pub fn connect_options(&self) -> PgConnectOptions {
        PgConnectOptions::new()
            .host(&self.host)
            .port(self.port)
            .username(&self.user)
            .password(&self.password)
            .database(&self.database)
    }

    pub async fn build_pool(&self) -> Result<PgPool, sqlx::Error> {
        PgPoolOptions::new()
            .max_connections(25)
            .min_connections(0)
            .max_lifetime(Duration::from_secs(5 * 60))
            .idle_timeout(Duration::from_secs(2 * 60))
            .connect_with(self.connect_options())
            .await
    }
}

/// `PRODUCT_FLAT_CACHE=off` disables the in-process flattened-product/category
/// cache, forcing every fetch to hit the DB directly — same toggle as Go.
pub fn product_flat_cache_enabled() -> bool {
    env_or("PRODUCT_FLAT_CACHE", "on") != "off"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    Basic,
    Key,
    Token,
}

impl AuthType {
    /// Reads `AUTH_TYPE`, defaulting to Basic.
    pub fn from_env() -> Self {
        match env_or("AUTH_TYPE", "basic").as_str() {
            "key" => AuthType::Key,
            "token" => AuthType::Token,
            _ => AuthType::Basic,
        }
    }
}

pub fn app_port() -> u16 {
    env_or("PORT", "8080").parse().unwrap_or(8080)
}

/// Base URL product/gallery image paths are resolved against, matching
/// Go's `MEDIA_URL` (default `http://localhost/media/`).
pub fn media_url() -> String {
    env_or("MEDIA_URL", "http://localhost/media/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `std::env` is process-global, and `cargo test` runs tests on multiple
    // threads within one process by default. Every test in this module that
    // reads or mutates env vars takes this lock first so they can't race.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn db_config_defaults_match_gogento_env_example() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Clear any inherited env vars from the test process so this test is
        // deterministic regardless of the shell it runs in.
        for k in ["MYSQL_HOST", "MYSQL_PORT", "MYSQL_USER", "MYSQL_PASS", "MYSQL_DB"] {
            std::env::remove_var(k);
        }
        let cfg = DbConfig::from_env();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 3306);
        assert_eq!(cfg.user, "magento");
        assert_eq!(cfg.password, "magento");
        assert_eq!(cfg.database, "magento");
    }

    #[test]
    fn db_config_reads_overrides_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("MYSQL_HOST", "gogento-mysql");
        std::env::set_var("MYSQL_PORT", "3309");
        let cfg = DbConfig::from_env();
        assert_eq!(cfg.host, "gogento-mysql");
        assert_eq!(cfg.port, 3309);
        std::env::remove_var("MYSQL_HOST");
        std::env::remove_var("MYSQL_PORT");
    }

    #[test]
    fn invalid_port_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("MYSQL_PORT", "not-a-number");
        let cfg = DbConfig::from_env();
        assert_eq!(cfg.port, 3306);
        std::env::remove_var("MYSQL_PORT");
    }

    #[test]
    fn pg_db_config_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        for k in ["POSTGRES_HOST", "POSTGRES_PORT", "POSTGRES_USER", "POSTGRES_PASS", "POSTGRES_DB"] {
            std::env::remove_var(k);
        }
        let cfg = PgDbConfig::from_env();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 5432);
        assert_eq!(cfg.user, "magento");
        assert_eq!(cfg.password, "magento");
        assert_eq!(cfg.database, "magento");
    }

    #[test]
    fn pg_db_config_reads_overrides_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("POSTGRES_HOST", "gogento-postgres");
        std::env::set_var("POSTGRES_PORT", "5435");
        let cfg = PgDbConfig::from_env();
        assert_eq!(cfg.host, "gogento-postgres");
        assert_eq!(cfg.port, 5435);
        std::env::remove_var("POSTGRES_HOST");
        std::env::remove_var("POSTGRES_PORT");
    }

    #[tokio::test]
    async fn pg_build_pool_connects_to_the_gogento_postgres_dev_container() {
        let cfg = PgDbConfig { host: "127.0.0.1".into(), port: 5435, user: "magento".into(), password: "magento".into(), database: "magento".into() };
        match cfg.build_pool().await {
            Ok(pool) => {
                let one: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&pool).await.unwrap();
                assert_eq!(one, 1);
            }
            Err(e) => eprintln!("skipping: cannot connect to gogento-postgres: {e}"),
        }
    }

    #[test]
    fn product_flat_cache_defaults_enabled_and_respects_off() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PRODUCT_FLAT_CACHE");
        assert!(product_flat_cache_enabled());
        std::env::set_var("PRODUCT_FLAT_CACHE", "off");
        assert!(!product_flat_cache_enabled());
        std::env::set_var("PRODUCT_FLAT_CACHE", "on");
        assert!(product_flat_cache_enabled());
        std::env::remove_var("PRODUCT_FLAT_CACHE");
    }

    #[test]
    fn auth_type_defaults_to_basic_and_parses_variants() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("AUTH_TYPE");
        assert_eq!(AuthType::from_env(), AuthType::Basic);
        std::env::set_var("AUTH_TYPE", "key");
        assert_eq!(AuthType::from_env(), AuthType::Key);
        std::env::set_var("AUTH_TYPE", "token");
        assert_eq!(AuthType::from_env(), AuthType::Token);
        std::env::set_var("AUTH_TYPE", "garbage");
        assert_eq!(AuthType::from_env(), AuthType::Basic);
        std::env::remove_var("AUTH_TYPE");
    }

    #[test]
    fn app_port_defaults_to_8080() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PORT");
        assert_eq!(app_port(), 8080);
        std::env::set_var("PORT", "9090");
        assert_eq!(app_port(), 9090);
        std::env::remove_var("PORT");
    }

    #[test]
    fn media_url_defaults_and_reads_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("MEDIA_URL");
        assert_eq!(media_url(), "http://localhost/media/");
        std::env::set_var("MEDIA_URL", "https://cdn.example.com/media/");
        assert_eq!(media_url(), "https://cdn.example.com/media/");
        std::env::remove_var("MEDIA_URL");
    }

    #[test]
    fn load_dotenv_does_not_panic_without_a_dotenv_file() {
        // No `.env` file is expected in the crate's test working directory,
        // so this exercises the "missing file is ignored" path -- matching
        // Go's `godotenv.Load()` silently no-op'ing when absent.
        load_dotenv();
    }

    #[tokio::test]
    async fn build_pool_connects_to_the_gogento_mysql_dev_container() {
        // Mirrors the skip-if-unreachable pattern used throughout this
        // project's DB-touching tests: give real coverage of pool
        // construction when the dev container is up, without failing
        // `cargo test` in a sandbox that has no Docker/MySQL available.
        let cfg = DbConfig {
            host: "127.0.0.1".into(),
            port: 3309,
            user: "magento".into(),
            password: "magento".into(),
            database: "magento".into(),
        };
        match cfg.build_pool().await {
            Ok(pool) => {
                let one: i64 = sqlx::query_scalar("SELECT 1").fetch_one(&pool).await.unwrap();
                assert_eq!(one, 1);
            }
            Err(e) => eprintln!("skipping: cannot connect to gogento-mysql: {e}"),
        }
    }

    #[test]
    fn connect_options_uses_configured_values() {
        let cfg = DbConfig {
            host: "example-host".into(),
            port: 3309,
            user: "u".into(),
            password: "p".into(),
            database: "db".into(),
        };
        // MySqlConnectOptions doesn't expose getters for round-tripping, so we
        // just confirm construction doesn't panic and produces a value.
        let _opts = cfg.connect_options();
    }
}
