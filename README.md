# gogento-rust

A Rust reimplementation of [GoGento](https://github.com/Genaker/GoGento) (a Go/Echo/GORM
Magento REST+GraphQL API layer), built to compare Go and Rust head-to-head on the same
operations against the same MySQL data. Independent project/repo — not a fork or a
modification of GoGento.

See the implementation plan for full scope, architecture, and phasing.

## Status: Phases A–D complete

- `entity` — EAV data model (CE schema only; see non-goals)
- `config` — env-driven config, MySQL pool construction
- `repository` — EAV flattening logic, batched DB fetch/CRUD, in-process flat cache, category tree
- `import` — CSV → EAV bulk import pipeline (the benchmarked path)
- `api-rest` — REST API: products/categories/stock endpoints, basic/key auth with
  Go's exact skip-list, timing headers, gzip
- `api-graphql` — GraphQL: full schema (products, categories, category tree,
  Magento/Venia-compatible `magentoProducts`/`magentoCategories`, store-ID
  resolution from header/variable/query-param), `search`/`_extension` stubbed
- `api-realtime` — HMAC-gated realtime price/stock API (Phase D, stretch)
- `bin/import_cli.rs` (`gogento-import`) — standalone benchmark CLI
- `src/main.rs` (`gogento-server`) — the actual HTTP server, all three API
  layers merged into one `axum::Router` and served together

Verified end-to-end against a live MySQL instance: full REST CRUD lifecycle,
every GraphQL query in the schema, and the HMAC-gated realtime endpoint
(cross-verified with an independent Python HMAC implementation, not just
self-consistently).

## Running the server

```bash
cp .env.example .env   # point MYSQL_HOST/PORT at your MySQL instance
cargo build --release --bin gogento-server
./target/release/gogento-server
# REST:      curl -u admin:secret http://localhost:8080/api/products/flat?limit=5
# GraphQL:   curl -X POST -H 'Content-Type: application/json' \
#              -d '{"query":"query { products(pageSize:5){ items { sku name } } }"}' \
#              http://localhost:8080/graphql
# Realtime:  curl http://localhost:8080/api/realtime/stock?sku=<sku>
```

## Running the import benchmark

```bash
cargo build --release --bin gogento-import
./target/release/gogento-import --file path/to/products.csv --batch-size 500
```

## Testing

```bash
cargo test --workspace
```

Unit tests cover all pure logic (CSV parsing, EAV flattening, stock/price
bucketing, validation, HMAC-adjacent config). DB-touching integration tests
(`sku_lookup`, `entities`, `flush`, `run`) connect to a live MySQL instance —
default `mysql://magento:magento@127.0.0.1:3309/magento` (this project's dev
`gogento-mysql` Docker container), overridable via `GOGENTO_TEST_DATABASE_URL`.
They skip gracefully (not fail) if that database isn't reachable, mirroring
GoGento's own `t.Skip`-on-no-DB test pattern.

REST/GraphQL/realtime handlers are tested the same way, but through their
actual axum routers via `tower::ServiceExt::oneshot` (REST/realtime) or
`Schema::execute` (GraphQL) — full HTTP-level request/response round trips
against the live DB, not mocks: CRUD lifecycles, auth skip-list behavior
(including once nested under the full app), cache warm/cold paths, pagination
edge cases, and the HMAC gate's accept/reject paths.

Coverage (`cargo llvm-cov --workspace`, with the dev database up): **96.0%
regions, 96.9% functions, 97.9% lines** across 238 tests. The remaining ~2-4%
is almost entirely `?`-propagated `sqlx::Error` branches inside DB calls that
only trigger on an actual connection/query failure mid-operation — not
reachable without deliberately breaking the database, and not mocked here
since mocking sqlx's wire protocol wouldn't meaningfully test anything beyond
what the pure logic tests already cover.

## Go vs Rust benchmark: product import

Same operation, same MySQL container (`gogento-mysql`, un-networked from any
other project), same 1000-row/13-attribute-column CSV
(`sku,name,meta_title,url_key,description,short_description,color,size,status,price,weight,special_price,special_from_date,special_to_date`),
`--batch-size 500`, 3 runs each with the target rows deleted between runs so
every run is a fresh insert (not an update):

```
# Go (GORM_LOG=off for a fair comparison -- Rust has no equivalent verbose
# per-query logging enabled by default, so Go's must be disabled too):
cd ~/GoGento
GORM_LOG=off go run -tags cli . products:import -f bench-1000.csv --batch-size 500 --raw-sql

# Rust:
cd ~/gogento-rust
./target/release/gogento-import --file bench-1000.csv --batch-size 500
```

| Run | Go total time | Rust total time |
|---|---|---|
| 1 | 234ms | 382ms |
| 2 | 200ms | 386ms |
| 3 | 296ms | 430ms |
| **Median** | **234ms** | **386ms** |
| Rate (median) | ~4,270 products/sec | ~2,590 products/sec |

**Go was faster on this specific benchmark.** Both implementations are almost
entirely DB-round-trip-bound here (Rust's own breakdown: ~1.3ms in-memory
processing vs ~380ms in DB calls, for 1000 products / 13,000 EAV rows across
5 tables + attribute lookup + SKU resolution + entity insert). The most
likely explanation is transaction scope: this Rust implementation issues each
batched upsert as its own auto-committed statement (no explicit transaction
wrapping the concurrent per-table flush), while GORM's `CreateInBatches` may
be batching differently under the hood. Wrapping each flush's batches in one
explicit transaction is a plausible next optimization, not yet done — this
result is reported as-is rather than tuned until Rust wins, since the point
of this project is an honest comparison, not a foregone conclusion.

## Non-goals (this port, v1)

Cron jobs, the extension/registry mechanism, RBAC/ACL enforcement, the
Redis-backed sales-grid cache and sales module generally, gallery import,
EE (`row_id`) schema support, Elasticsearch-backed search, static asset/template
serving. See the implementation plan for the full reasoning.
