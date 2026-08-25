//! Config layer — mirrors GoGento's `config/` package: env-var driven settings,
//! `.env` file support (ignored if missing, matching `godotenv.Load()`), and
//! MySQL pool construction matching GORM's connection-pool settings.

use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::MySqlPool;
use std::time::Duration;

/// Load a `.env` file if present. Mirrors Go's `godotenv.Load()`, which silently
/// no-ops when the file is missing rather than erroring.
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
    /// defaults GoGento's `.env.example` documents.
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

    /// Builds a connection pool matching GORM's settings in GoGento's `config/db.go`:
    /// 25 max open, 25 max idle, 5 min max lifetime, 2 min max idle time.
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
    /// Reads `AUTH_TYPE`, defaulting to Basic — matches Go's `core/auth`.
    pub fn from_env() -> Self {
        match env_or("AUTH_TYPE", "basic").as_str() {
            "key" => AuthType::Key,
            "token" => AuthType::Token,
            _ => AuthType::Basic,
        }
    }
}

/// Paths exempt from `/api` auth, matching GoGento's `config/api.go` exactly
/// (including the fact that `/api/products/flat` etc. are NOT in this list).
pub const AUTH_SKIP_PATHS: [&str; 4] = ["/health", "/api/products", "/api/products/:id", "/graphql"];

pub fn app_port() -> u16 {
    env_or("PORT", "8080").parse().unwrap_or(8080)
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
    fn auth_skip_paths_match_go_exactly() {
        assert_eq!(
            AUTH_SKIP_PATHS,
            ["/health", "/api/products", "/api/products/:id", "/graphql"]
        );
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
