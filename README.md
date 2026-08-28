![RustGento](RustGento.jpeg)

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
- **`web`** — server-rendered storefront: a homepage (hero slider, live
  catalog stats), category listing with pagination, product detail with
  breadcrumbs/gallery/price-index table, and full-text product search
  (SKU + name) -- plus a desktop dropdown menu and mobile slide-out menu,
  both built from the same cached category tree. Built on `askama`
  (compile-time-checked templates -- a template referencing a field the
  page struct doesn't have fails `cargo build`, not a live request) plus
  an `/image/webp` resize-and-reencode proxy (jpeg/png/webp, letterboxed
  to an exact box when both dimensions are given, disk-cached)
- **`bin/import_cli.rs`** (`gogento-import`) — standalone CLI for
  benchmarking/running the bulk importer outside the server process
- **`src/main.rs`** (`gogento-server`) — the HTTP server: all four layers
  (REST/GraphQL/realtime/web) merged into one `axum::Router`, plus static
  asset serving

Verified end-to-end against a live MySQL instance: full REST CRUD lifecycle,
every query in the GraphQL schema, the HMAC-gated realtime endpoint
(signature cross-checked against an independent Python HMAC implementation,
not just self-consistently), and the category/product pages rendering real
data with working pagination, breadcrumbs, and image resizing.

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

If the dev MySQL instance was seeded via GoGento's `cmd/seed` (GORM
`AutoMigrate`), run `sql/mysql_restore_upsert_keys.sql` against it once
afterward — `AutoMigrate` doesn't create the `unq_entity_attr_store` /
`unq_product_stock` unique keys this project's `ON DUPLICATE KEY UPDATE`
upserts rely on to update in place instead of inserting a duplicate row on
re-import, and their absence only surfaces as a test failure or duplicate
data on a second import, not a missing-table error.

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

**Memory usage on the same benchmark**: `/usr/bin/time -l` around each
binary/process, same 1000-row/13-attribute CSV. PHP has three variants in
`bench/`: plain PDO (no framework), Magento bootstrapped but still writing
via this project's own PDO code, and Magento's own `Model::save()` per row
(no bulk API — the way someone scripting against Magento directly would
write it). Three more languages joined later, each targeting the same real
Magento database the Magento-targeting PHP rows use: Laravel, in the
sibling [laragento](https://github.com/Genaker/laragento) repo — a real
Laravel 11 app whose `magmi:import` Artisan command batch-upserts via
Eloquent's own `DB::table(...)->upsert(...)`; Python, in the sibling
[PyGento](https://github.com/Genaker/PyGento) repo — `import_products.py`,
batch-upserting via SQLAlchemy Core's `insert().on_duplicate_key_update()`;
and Node.js, in the sibling [nodejento](https://github.com/Genaker/nodejento)
repo — `import_products.js`, batch-upserting via Sequelize's
`Model.bulkCreate(rows, {updateOnDuplicate: [...]})`, which compiles to the
same one-multi-row-`INSERT ... ON DUPLICATE KEY UPDATE`-per-chunk shape as
everywhere else in this family.

**All eight rows below now target the same database.** Go, Rust, and
plain PDO originally ran against a separate, leaner synthetic schema
(`gogento-mysql`: no FK constraints, no pre-existing rows) rather than the
real Magento sample-data DB (`mage-postgres_mysql_1`: FK constraints on
every EAV table, 17,500+ existing rows) the other five already used —
an apples-to-oranges gap in the original table. All three were re-pointed
at the real DB (`MYSQL_HOST=127.0.0.1 MYSQL_PORT=3308`, same `.env`
convention every implementation already shares) and re-measured there
(5 runs each, median reported) — both Go's and Rust's importers already
resolved every `attribute_id` dynamically via `eav_attribute` rather than
hardcoding one, so no code changes were needed, just a different
connection target. This is a disposable, test-only database, so pointing
more benchmarks at it and writing/deleting `BENCH-STD-*` rows freely is
fine:

| | Bootstrap | Import (1000 rows) | Max RSS |
|---|---|---|---|
| Go, raw SQL (`--raw-sql`) | — | ~2.4s | ~15.2 MB |
| Go, default (GORM `CreateInBatches`) | — | ~2.4s | ~15.4 MB |
| Rust | — | ~2.3s | ~8.4 MB |
| PHP, plain PDO | — | ~4.3s | 24.2 MB |
| PHP, Magento bootstrap + PDO | ~0.38s | ~5.4s | 37.3 MB |
| Laravel, Eloquent `upsert()` (laragento) | — | ~6.9s | 47.5 MB |
| Node.js, Sequelize `bulkCreate` upsert (nodejento) | — | ~6.9s | ~90 MB |
| Python, SQLAlchemy Core `upsert` (PyGento) | — | ~7.7s | 46.9 MB |
| PHP, Magento model (`::save()`) | ~1.5s | ~135.4s | 70.0 MB |

**The two Magento-bootstrap rows above were re-measured, and the
original numbers turned out to be wrong in a more serious way than
session-to-session noise.** This Magento install's `app/etc/env.php` had
its actual **database connection** (not just its Redis cache) pointed at
a Postgres container via a custom `Morozov\PgCompat` compatibility
adapter — a `di.xml` preference that unconditionally substitutes
`Magento\Framework\DB\Adapter\Pdo\Mysql` regardless of what `env.php`
says. The Postgres container had been stopped for hours, so the
bootstrap benchmark was silently failing before this pass; whatever
session originally produced "0.32s / 2.90s" and "0.25s / 120.6s" was
running Magento's bootstrap against Postgres, not the MySQL instance
every other row in this table uses — never a valid comparison, not even
before accounting for noise. Fixed for this measurement by temporarily
disabling `Morozov_PgCompat` and pointing `env.php` at
`mage-postgres_mysql_1` directly, taking the numbers, then restoring
both files from a pre-edit backup and re-enabling the module — this
project's actual active work (per its own most recent commit) is
building that Postgres adapter, and this benchmark has no business
leaving it altered. Bootstrap cost roughly doubled (0.32s → 0.38s
bootstrap alone is close, but total import time moved 2.90s → 5.4s and
120.6s → 135.4s) — plausibly just this session's generally heavier DB
load (see the correction above), but the two numbers are at least now
verified to be measuring the same database engine as everything else.

With everything on one database engine, the real shape is: Rust, both
Go modes, and plain PDO (~2.3-4.3s) — raw SQL or a thin GORM/sqlx layer —
form one tier; Magento's own bootstrap+PDO joins closer to the second
tier at ~5.4s; the three ORM-tier batched-upsert implementations
(Laravel, Node, PyGento, ~6.9-7.7s) cluster together regardless of
language; and `Model::save()`'s per-row full-object-lifecycle path
(~135.4s) is in a class of its own, ~25-60x slower than anything batched.

The interesting result isn't a clean "ORMs are slow" story, though: Go's
own ORM (GORM) shows **no tax at all** — `db.Clauses(clause.OnConflict{...}).CreateInBatches(...)`
lands within noise of hand-written raw SQL on both time and memory,
confirmed with 5 repeated runs. But Laravel, Node, and PyGento's ORMs
are all ~3x slower doing the same logical operation, despite each one
independently confirmed (via query logs, not assumption) to compile down
to the same single batched `INSERT ... ON DUPLICATE KEY UPDATE`
per chunk that GORM and raw SQL use — no N+1 queries hiding anywhere.
So the gap isn't "has an ORM" vs. "doesn't" — it's something specific to
how Sequelize/Eloquent/SQLAlchemy execute a batch upsert that GORM's
implementation doesn't share. That's not yet isolated to a root cause,
and it's called out here as an open question rather than a tidy
explanation forced onto data that doesn't support one.

Node originally showed ~155MB here: `Models/init-models.js` eagerly
`require()`s and `sequelize.define()`s all 347 sequelize-auto-generated
model files (every Magento core table, not just the 7 this importer
touches) on every CLI invocation, a cost Laravel and PyGento don't pay
since they only load the handful of models their importer actually
touches. Fixed in nodejento via a `getLiteModels()` path that defines only
the needed tables directly from their individual model files, skipping
`init-models.js`'s full graph and association wiring — measured at ~20ms
vs ~1,330ms in isolation (~65x), which is what dropped Max RSS from ~155MB
to ~90MB. Full detail and the isolated-timing table are in nodejento's own
README. (nodejento's web storefront, by contrast, is a long-lived process
that genuinely benefits from loading the full model graph once and reusing
it — it still uses the original `getModels()`, deliberately.)

The Magento-model row is the odd one out on purpose: it isn't "Magento is
slow," it's what one full load/validate/persist per row costs relative to
bypassing that abstraction with batched raw SQL — the exact tradeoff this
project's Go and Rust reimplementations exist to explore. The Laravel and
PyGento rows sit between the two PHP-on-real-Magento numbers for the same
reason: both are properly batched (unlike `Model::save()`), so they avoid
that 41x-scale penalty almost entirely, but each ORM's query-builder
layer is still measurably heavier than hand-rolled PDO on the identical
statement shape against the identical database.

**A correction, left in deliberately**: an earlier version of this table
had PyGento at 8.68s — nearly 2x Laravel's number. That comparison was
wrong, not because of a code bug, but a measurement one: the two numbers
were captured in separate sessions against the same shared, real,
actively-used MySQL instance, and this instance's load varies a lot
run to run (confirmed directly: three back-to-back Laravel/PyGento pairs,
run immediately after each other with the host otherwise idle, landed at
4.29s/4.55s median — within ~6%, not 2x). A `general_log`-driven check
of the actual SQL also confirmed PyGento's batching was never the
problem: `insert(table).values(chunk).on_duplicate_key_update(...)`
compiles to one genuine multi-row `INSERT ... ON DUPLICATE KEY UPDATE`
per chunk, same as everywhere else in this benchmark family, not N
separate statements. The lesson: on a shared database, "measure once,
each language on its own turn" produces numbers that *look* precise but
aren't comparable — only back-to-back, same-session runs are. See each
script's own header comment in `bench/` (and laragento's/PyGento's own
READMEs) for full methodology and caveats (schema differences, indexer
mode, run counts).

**The table above reflects that lesson applied a second time.** When
nodejento was added (2026-08-27), Laravel and PyGento were re-measured
in the same session immediately before and after nodejento's runs (3
runs each, host otherwise idle, no Time Machine/Spotlight activity),
rather than reusing the ~4.4s/~4.5s numbers from the earlier session —
and this session's host/DB load was simply heavier: all three landed at
6.9-7.7s, roughly 50-70% slower across the board than the original
measurement, with their *relative order* (Laravel and Node effectively
tied, PyGento a bit behind) the only thing that held up. The ~4.4s/~4.5s
numbers were real for their session, just not for this one — which is
the whole point: absolute import times on this shared real-Magento
database are only meaningful compared against numbers from the same
sitting, never against a table from a different day.

## Performance: MySQL vs. Postgres, 10k products / 40 attributes

A larger, wider-schema run of the same importer against both drivers: a
10,000-row CSV across 40 attribute columns (10 varchar, 8 text, 10 int, 8
decimal, 4 datetime — roughly double the original benchmark's column count),
`--batch-size 500`, 3 runs each with every `PERF10K-*` row deleted across all
seven core tables between runs so every run is a fresh insert. Same host,
same importer binary, one driver flag different.

| | MySQL | Postgres (batched `INSERT ... ON CONFLICT`) | Postgres (`COPY` + merge) |
|---|---|---|---|
| Run 1 | 3.31s | 5.90s | 4.39s |
| Run 2 | 2.90s | 5.71s | 3.89s |
| Run 3 | 3.33s | 8.87s | 4.15s |
| Median | 3.31s | 5.90s | 4.15s |
| Rate (median) | ~3,020 products/sec | ~1,695 products/sec | ~2,410 products/sec |

Both databases end up with byte-identical data in every column (spot-checked
directly: matching `name`/`price` for the same SKUs in both), and produce
the exact same row counts every run — 10,000 entities, 369,000 EAV rows
split identically across backend types (100k varchar, 100k int, 75k decimal,
80k text, 14k datetime).

**Why the first Postgres number was slow, and how `pg.rs` fixes it**: the
initial Postgres path used the same strategy as the MySQL path — batched
multi-row `INSERT ... ON CONFLICT DO UPDATE`, chunked at `--batch-size`.
Isolating that specific clause (`EXPLAIN ANALYZE` on 5,000 fresh rows, zero
actual conflicts) showed it costing **57% more than a plain `INSERT`** in
Postgres (166ms → 260ms), against roughly **0% overhead** for MySQL's
`ON DUPLICATE KEY UPDATE` on the same rows (21ms → 13ms, within noise).
Postgres implements `ON CONFLICT` via *speculative insertion* — every row
optimistically inserts into the unique index, then checks whether that just
collided, backing out to an `UPDATE` only if it did — so you pay for the
conflict-arbiter machinery on every row even when nothing ever conflicts,
which is exactly this benchmark's shape (a deliberate fresh-insert test).

`crates/import/src/pg.rs` implements both strategies side by side as
`PgWriteMode::Insert` (the batched `ON CONFLICT` approach just described)
and `PgWriteMode::Copy`: `COPY FROM STDIN` streams every row (no chunking
limit — one `COPY` per table, not one per `--batch-size` chunk) into a
per-transaction, `ON COMMIT DROP` temporary table with no indexes or
constraints to check at all, then a single set-based
`INSERT ... SELECT ... ON CONFLICT` merges it into the real table. The
conflict-arbiter cost still applies to that one merge statement, but only
once, instead of once per chunked round trip — which is most of why this
beats even the plain-`INSERT` baseline from the `EXPLAIN ANALYZE` test
above. Net effect end-to-end: median time dropped from 5.90s to 4.15s
(**~30% faster**), cutting MySQL's lead from ~1.8x to **~1.25x**.

**`Insert` is the default**, not `Copy` — despite being slower. It's the
simpler, longer-exercised code path (no temporary tables, no `COPY`
protocol handshake to get right), so it's the safer choice whenever
correctness matters more than the last ~30% of throughput; `Copy` is an
explicit opt-in for when it doesn't. Select it via `gogento-import
--driver postgres --pg-write-mode copy` (or `PgWriteMode::Copy` when
calling `import_products_pg` directly) — both modes have their own
create/reimport/upsert-correctness test in `pg.rs`, not just the default.

**Fairness caveat, stated plainly**: this comparison is *not* apples-to-apples
on durability. `gogento-postgres` was created with `fsync=off`,
`full_page_writes=off`, and `synchronous_commit=off` — durability-relaxed
settings — while `gogento-mysql` runs with MySQL's out-of-the-box InnoDB
durability (`fsync` on, `innodb_flush_log_at_trx_commit=1`, binary logging
on). That's the opposite of a thumb on the scale for MySQL: even with
Postgres's durability guarantees turned down, MySQL was still faster on this
workload. A true apples-to-apples run would need both engines at matching
durability levels; take the ~1.25x figure as directional, not precise.

Also note the `COPY` path only covers the 5 EAV value-table flushes (369,000
of this benchmark's ~379,000 total rows) — entity creation still uses
`INSERT ... RETURNING sku, entity_id` (10,000 rows; RETURNING is how this
path gets IDs back without MySQL's `LAST_INSERT_ID()`), and stock/price
still use batched `INSERT ... ON CONFLICT` (0 rows in this fixture, so
untested at this scale either way). The remaining ~1.25x gap is plausibly
still partly attributable to those two paths, plus Postgres's inherently
larger per-tuple MVCC overhead (heap tuples carry xmin/xmax/ctid/infomask
bookkeeping InnoDB's row format doesn't) — neither was isolated with its own
`EXPLAIN ANALYZE` test the way the `ON CONFLICT` cost was.

Why Postgres runs with fsync off at all: this Docker Desktop environment
(an old 20.10.2 install) hit a real `PANIC: could not fsync file ... I/O
error` crash in `gogento-postgres` under sustained WAL-checkpoint write
pressure during an earlier 100,000-row/40-attribute attempt at this same
benchmark — reproduced twice, including once against a freshly created named
volume, so it wasn't specific to the container's writable layer. Disabling
fsync on this disposable, no-real-data benchmark container was the practical
workaround. `gogento-mysql` hit its own unrelated crash during that same
100k-row attempt (`InnoDB: [FATAL] fsync() returned EIO`) triggered by the
*host* disk actually filling up (Docker Desktop's VM disk had grown to 28GB
against a nearly-full host disk) mid-write — a genuine host resource issue,
not a Postgres- or MySQL-specific flaw. That combination of crashes is the
direct reason this benchmark uses 10k products rather than the originally
attempted 100k: the smaller size stays well clear of both failure modes on
this particular machine.

Reproducing this needs 40 seeded `eav_attribute` rows in both databases
(not just the 13 `fixtures/synthetic_products.csv` seeds) and a wider CSV
fixture — neither is checked in, same as the original 1000-row Go-vs-Rust
benchmark's CSV isn't:

```bash
# Seed the 40 attributes both drivers' import runs need (idempotent, safe
# to re-run against a container that already has some or all of them):
docker exec -i gogento-mysql mysql -umagento -pmagento magento < sql/mysql_seed_attributes.sql
# sql/postgres_schema.sql already seeds all 40 for a fresh Postgres instance.

# Generate a 10k-row/40-column CSV (same shape as
# fixtures/synthetic_products.csv, just wider and taller) and run both:
POSTGRES_HOST=127.0.0.1 POSTGRES_PORT=5435 \
  cargo run --release --bin gogento-import -- --driver postgres --file perf_10k_products.csv --batch-size 500
cargo run --release --bin gogento-import -- --driver mysql --file perf_10k_products.csv --batch-size 500
```

## Postgres synthetic import

The importer's primary target is MySQL (see [Known limitations](#known-limitations)),
but `gogento-import` also has a `--driver postgres` mode: a parallel,
Postgres-native write path for the "core" import tables (product entity + the
5 EAV value tables + stock + price index -- the same subset the benchmark
above exercises). It's a synthetic-data smoke test proving the import logic
itself (CSV parsing, EAV bucketing, upsert-not-duplicate semantics) isn't
accidentally MySQL-specific, not a second production target: categories,
tier pricing, product links, custom options, downloadable, bundle, and
configurable products aren't part of it.

Postgres has no unsigned integer types and no `ON DUPLICATE KEY UPDATE`/
`LAST_INSERT_ID()`, so this isn't `sqlx::Database`-generic code shared with
the MySQL path -- it's `crates/import/src/pg.rs`, a hand-mirrored write path
using `INSERT ... ON CONFLICT ... DO UPDATE` and `RETURNING sku, entity_id`
(binding entity/attribute/store IDs down to `i32`/`i64` on the way in). All
of the DB-free logic -- CSV parsing, EAV bucketing, stock/price collection --
is reused unchanged from the MySQL path.

```bash
# Mimic the core tables in a fresh Postgres instance (only the columns this
# importer actually reads/writes -- see the file's header for the exact
# MySQL-vs-Postgres differences and what's out of scope):
docker run -d --name gogento-postgres \
  -e POSTGRES_USER=magento -e POSTGRES_PASSWORD=magento -e POSTGRES_DB=magento \
  -p 5435:5432 postgres:16-alpine
docker exec -i gogento-postgres psql -U magento -d magento < sql/postgres_schema.sql

# Import the same synthetic fixture used in the dual-DB test:
POSTGRES_HOST=127.0.0.1 POSTGRES_PORT=5435 \
  cargo run --bin gogento-import -- --driver postgres --file fixtures/synthetic_products.csv

# The MySQL path, unchanged, against the same fixture:
cargo run --bin gogento-import -- --driver mysql --file fixtures/synthetic_products.csv
```

`crates/import/src/pg.rs`'s test module includes
`same_synthetic_csv_imports_into_both_mysql_and_postgres`: the same synthetic
CSV run through both drivers, asserting identical created/EAV counts --
skipped gracefully if either `GOGENTO_TEST_DATABASE_URL` or
`GOGENTO_TEST_POSTGRES_URL` (default `postgres://magento:magento@127.0.0.1:5435/magento`)
isn't reachable.

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
