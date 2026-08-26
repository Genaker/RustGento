# gogento-rust

A Rust-native Magento catalog API: a REST + GraphQL server backed by a
flattened EAV (entity-attribute-value) read model, a high-throughput bulk
CSV product importer, and an HMAC-gated realtime price/inventory API. Built
on `axum`, `sqlx`, and `async-graphql` against a MySQL-backed Magento schema.

## Why this exists

Magento's catalog is modeled as EAV: a product's attributes (name, price,
description, ...) live as rows in per-type value tables
(`catalog_product_entity_{varchar,int,decimal,text,datetime}`), keyed by
`(entity_id, attribute_id, store_id)`, rather than as columns on one row.
Reading a product means joining across five tables plus stock and category
links; writing a CSV of products means resolving SKUs to entity IDs and
fanning attribute values out across those same five tables in bulk. This
project implements that whole read/write path natively in Rust: a typed
entity layer over the real schema, an attribute-flattening layer that turns
those joins into one JSON map per product, a concurrent batched-upsert import
pipeline, and REST/GraphQL/realtime APIs on top.

## What's here

- **`entity`** — typed structs over the live MySQL schema (CE/`entity_id`
  schema; see [Known limitations](#known-limitations))
- **`config`** — env-driven configuration, MySQL pool construction
- **`repository`** — EAV attribute flattening, batched DB fetch/CRUD, an
  in-process flat-result cache, category tree construction
- **`import`** — CSV → EAV bulk import pipeline: SKU resolution, new-entity
  insertion (with its own `type_id` per row, so configurable/bundle/
  downloadable products aren't silently created as "simple"), per-backend-type
  value bucketing with validation, concurrent batched upserts across all five
  value tables plus stock, price-index, on-the-fly categories, tier/group
  pricing, related/up-sell/cross-sell/grouped product links, image gallery
  (with proper entity linking), custom options, downloadable links/samples,
  bundle options/selections, and configurable super-attributes/links
- **`api-rest`** — REST endpoints for products/categories/stock, HTTP Basic
  or API-key auth, per-request timing headers, gzip
- **`api-graphql`** — a full GraphQL schema: paginated product/category
  search, category tree, Venia-storefront-compatible queries
  (`magentoProducts`/`magentoCategories` with base64 entity UIDs),
  multi-source store-ID resolution (header, GraphQL variable, or query
  param)
- **`api-realtime`** — a small HMAC-signed API for price/inventory lookups
  meant for latency-sensitive callers (checkout, cart) that don't want the
  overhead of a full GraphQL round trip
- **`bin/import_cli.rs`** (`gogento-import`) — standalone CLI for
  benchmarking/running the bulk importer outside the server process
- **`src/main.rs`** (`gogento-server`) — the HTTP server: all three API
  layers merged into one `axum::Router`

Verified end-to-end against a live MySQL instance: full REST CRUD lifecycle,
every query in the GraphQL schema, and the HMAC-gated realtime endpoint
(signature cross-checked against an independent Python HMAC implementation,
not just self-consistently).

## Running the server

```bash
cp .env.example .env   # point MYSQL_HOST/PORT at your MySQL instance
cargo build --release --bin gogento-server
./target/release/gogento-server

# REST
curl -u admin:secret 'http://localhost:8080/api/products/flat?limit=5'

# GraphQL
curl -X POST -H 'Content-Type: application/json' \
  -d '{"query":"query { products(pageSize:5){ items { sku name } } }"}' \
  http://localhost:8080/graphql

# Realtime
curl 'http://localhost:8080/api/realtime/stock?sku=<sku>'
```

## Configuration

| Variable | Default | Purpose |
|---|---|---|
| `MYSQL_HOST` / `MYSQL_PORT` / `MYSQL_USER` / `MYSQL_PASS` / `MYSQL_DB` | `localhost` / `3306` / `magento` / `magento` / `magento` | MySQL connection |
| `PORT` | `8080` | HTTP listen port |
| `AUTH_TYPE` | `basic` | `basic` (uses `API_USER`/`API_PASS`) or `key` (uses `API_KEY`, checked via `X-API-Key` or `Authorization: Bearer`) |
| `PRODUCT_FLAT_CACHE` | `on` | Set to `off` to bypass the in-process flattened-product cache entirely (useful for benchmarking cold-path latency) |
| `MAGENTO_CRYPT_KEY` | unset | HMAC key for the realtime API's signed endpoint; when unset, that endpoint's signature check is skipped entirely |
| `RUST_LOG` | unset | Standard `tracing`/`env_logger`-style log-level filter |

## API overview

**REST** (under `/api`, Basic/key-authenticated except the two paths below):

| Method | Path | |
|---|---|---|
| GET | `/health` | unauthenticated |
| GET | `/api/products` | unauthenticated; `?limit=` |
| GET, POST | `/api/products`, `/api/products/{id}` | list/get unauthenticated by ID only; create/update/delete require auth |
| GET | `/api/products/flat`, `/api/products/full`, `/api/products/flat/{ids}` | flattened attribute view, auth required |
| GET | `/api/categories`, `/api/category/{id}`, `/api/category/{ids}/flat`, `/api/category/tree` | |
| GET | `/api/category/cache`, `/api/category/cache/{id}` | introspects the in-process category cache |
| POST | `/api/stock/import` | bulk JSON stock upsert |
| GET | `/api/realtime/price`, `/stock`, `/tier-prices`, `/price-inventory` | the last is HMAC-gated when `MAGENTO_CRYPT_KEY` is set |

**GraphQL** (`/graphql`, unauthenticated, playground at `/playground`):
`products`, `product`, `categories`, `category`, `categoryTree`,
`magentoCategories`, `magentoProducts`, plus stubbed `search` and
`_extension` fields kept for schema-shape completeness.

## Running the import benchmark

```bash
cargo build --release --bin gogento-import
./target/release/gogento-import --file path/to/products.csv --batch-size 500
```

Reports rows processed, EAV/stock/price row counts, and a
processing-time/DB-time/total-time breakdown.

## Testing

```bash
cargo test --workspace
```

Unit tests cover all pure logic (CSV parsing, EAV flattening, stock/price
value bucketing and validation, pagination math, HMAC signing). DB-touching
integration tests connect to a live MySQL instance — default
`mysql://magento:magento@127.0.0.1:3309/magento`, overridable via
`GOGENTO_TEST_DATABASE_URL` — and skip gracefully (not fail) if that database
isn't reachable, so `cargo test` still passes in an environment with no
MySQL available.

REST/GraphQL/realtime handlers are tested the same way but through their
actual routers: `tower::ServiceExt::oneshot` for REST/realtime, `Schema::execute`
for GraphQL — full request/response round trips against the live DB, not
mocks. Coverage includes CRUD lifecycles, auth skip-list behavior (including
once nested under the full app), cache warm/cold paths, pagination edge
cases, and the HMAC gate's accept/reject paths.

Coverage (`cargo llvm-cov --workspace`, with the dev database up): **96.0%
regions, 96.9% functions, 97.9% lines** across 238 tests. The remainder is
almost entirely `?`-propagated `sqlx::Error` branches inside DB calls that
only trigger on an actual connection/query failure mid-operation — not
reachable without deliberately breaking the database mid-test, and not
mocked here since mocking sqlx's wire protocol wouldn't meaningfully test
anything beyond what the pure-logic tests already cover.

## Benchmark: bulk product import, Rust vs. an equivalent Go/Echo/GORM service

Same MySQL instance, same 1000-row/13-attribute-column CSV
(`sku,name,meta_title,url_key,description,short_description,color,size,status,price,weight,special_price,special_from_date,special_to_date`),
`--batch-size 500`, 10 runs each with the target rows deleted between runs so
every run is a fresh insert rather than an update.

| | Go service | This project |
|---|---|---|
| Min | 208ms | 188ms |
| Median | 247ms | 243ms |
| Max | 428ms | 427ms |
| Rate (median) | ~4,050 products/sec | ~4,110 products/sec |

Effectively tied, with this project a hair ahead. It wasn't originally: an
earlier version of this benchmark had this project's median at 386ms against
Go's 234ms, because every batched upsert was issued as its own
auto-committed statement — each chunk was a separate implicit transaction,
so a 1000-row/7-table import paid for several transaction commits (and their
fsyncs) per table instead of one. Wrapping each flush function's chunks in a
single explicit transaction (`pool.begin()` / `tx.commit()` around the whole
batch, instead of `execute()` straight against the pool per chunk) removed
that overhead and roughly halved this project's DB time.

The same fix was then found to be missing on the Go reference side too — its
raw-SQL EAV upsert and its GORM `CreateInBatches` calls for stock/price/
gallery all had the identical one-transaction-per-batch pattern — and
applying it there dropped Go's median from 288ms to 247ms. With the same
optimization on both sides, the two are within noise of each other; both
implementations remain almost entirely DB-round-trip-bound (this project's
own breakdown: 1.5-3ms in-memory processing vs. the rest in DB calls, for
1000 products / 13,000 EAV rows across 5 tables + attribute lookup + SKU
resolution + entity insert), and the wide per-run spread (188-428ms on
either side) reflects that round-trip variance far more than any remaining
algorithmic difference between the two.

## Known limitations

- **CE schema only.** Magento Enterprise's staging/versioning schema
  (`row_id`-keyed EAV tables) isn't supported; there's no runtime
  CE/EE detection, unlike a typical Magento-adjacent Go service.
- **Tier pricing is import-only.** The bulk importer writes real tier/group
  pricing to `catalog_product_entity_tier_price`, but the realtime
  `/tier-prices` endpoint hasn't been wired up to read it yet and still
  always returns an empty list.
- **No full-text/Elasticsearch search.** The GraphQL `search` field is
  present for schema-shape completeness but always returns an empty result.
- **No cron, extension registry, or RBAC enforcement.** These exist as
  scaffolding elsewhere but aren't part of this project's scope.
- **No image file download.** Gallery import writes and links the DB rows
  correctly, but doesn't fetch/store the actual image files a CSV's
  `image`/`small_image`/`thumbnail` URLs point at.
- **No sales module or Redis-backed caching.** Order management and the
  Redis cache layer some Magento-adjacent services use aren't implemented
  here — caching is a simple in-process, per-store map with no TTL or
  eviction.
- **Custom options, downloadable, bundle, and configurable products are
  import-only**, same caveat as tier pricing: the CSV importer writes all
  of them correctly, but the REST/GraphQL read APIs haven't been extended
  to surface bundle selections, custom option choices, downloadable links,
  or configurable variations in their responses yet -- they still return
  the same flat product shape as a simple product.
