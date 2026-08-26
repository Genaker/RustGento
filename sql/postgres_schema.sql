-- Postgres "mimic" schema for the synthetic import test.
--
-- Mirrors the subset of the live MySQL Magento-CE schema this project's
-- import pipeline actually reads/writes (verified against `gogento-mysql`
-- via `SHOW CREATE TABLE`) -- not a full Magento Postgres port. Two real
-- differences from the MySQL originals, both forced by Postgres itself:
--
--   1. No unsigned integer types in Postgres -- every MySQL `... unsigned`
--      column becomes a plain signed INTEGER/BIGINT here. The Rust importer
--      casts its u16/u32/u64 values down to i32/i64 when binding against
--      this schema (see `crates/import/src/pg.rs`).
--   2. `ON DUPLICATE KEY UPDATE` has no Postgres equivalent; the importer
--      uses `INSERT ... ON CONFLICT (...) DO UPDATE` instead, which is why
--      every upsert-target table below has an explicit UNIQUE constraint on
--      its natural key (MySQL's `unq_entity_attr_store` / `unq_product_stock`
--      indexes serve the same role there).
--
-- Only columns the importer actually populates are included, same
-- philosophy as the `entity` crate's structs ("only the subset of columns
-- this port actually reads/writes is modeled").
--
-- Scope: entity + the 5 EAV value tables + stock + price index -- the
-- "core" path this project's README benchmarks. Categories, tier prices,
-- product links, custom options, downloadable, bundle, and configurable
-- products are not part of this Postgres mimic (see the top-level README's
-- Postgres section).

CREATE TABLE IF NOT EXISTS eav_attribute (
    attribute_id     SERIAL PRIMARY KEY,
    entity_type_id   INTEGER NOT NULL DEFAULT 0,
    attribute_code   VARCHAR(255) NOT NULL,
    attribute_model  VARCHAR(255),
    backend_model    VARCHAR(255),
    backend_type     VARCHAR(8) NOT NULL DEFAULT 'static',
    backend_table    VARCHAR(255),
    frontend_model   VARCHAR(255),
    frontend_input   VARCHAR(50),
    frontend_label   VARCHAR(255),
    frontend_class   VARCHAR(255),
    source_model     VARCHAR(255),
    is_required      INTEGER NOT NULL DEFAULT 0,
    is_user_defined  INTEGER NOT NULL DEFAULT 0,
    default_value    TEXT,
    is_unique        INTEGER NOT NULL DEFAULT 0,
    note             VARCHAR(255),
    UNIQUE (entity_type_id, attribute_code)
);

CREATE TABLE IF NOT EXISTS catalog_product_entity (
    entity_id         BIGSERIAL PRIMARY KEY,
    attribute_set_id  INTEGER NOT NULL DEFAULT 0,
    type_id           VARCHAR(32) NOT NULL DEFAULT 'simple',
    sku               VARCHAR(64) NOT NULL UNIQUE,
    has_options       SMALLINT NOT NULL DEFAULT 0,
    required_options  INTEGER NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS catalog_product_entity_varchar (
    value_id      BIGSERIAL PRIMARY KEY,
    attribute_id  INTEGER NOT NULL DEFAULT 0,
    store_id      INTEGER NOT NULL DEFAULT 0,
    entity_id     BIGINT NOT NULL DEFAULT 0,
    value         VARCHAR(255),
    UNIQUE (entity_id, attribute_id, store_id)
);

CREATE TABLE IF NOT EXISTS catalog_product_entity_int (
    value_id      BIGSERIAL PRIMARY KEY,
    attribute_id  INTEGER NOT NULL DEFAULT 0,
    store_id      INTEGER NOT NULL DEFAULT 0,
    entity_id     BIGINT NOT NULL DEFAULT 0,
    value         INTEGER,
    UNIQUE (entity_id, attribute_id, store_id)
);

CREATE TABLE IF NOT EXISTS catalog_product_entity_decimal (
    value_id      BIGSERIAL PRIMARY KEY,
    attribute_id  INTEGER NOT NULL DEFAULT 0,
    store_id      INTEGER NOT NULL DEFAULT 0,
    entity_id     BIGINT NOT NULL DEFAULT 0,
    value         DOUBLE PRECISION,
    UNIQUE (entity_id, attribute_id, store_id)
);

CREATE TABLE IF NOT EXISTS catalog_product_entity_text (
    value_id      BIGSERIAL PRIMARY KEY,
    attribute_id  INTEGER NOT NULL DEFAULT 0,
    store_id      INTEGER NOT NULL DEFAULT 0,
    entity_id     BIGINT NOT NULL DEFAULT 0,
    value         TEXT,
    UNIQUE (entity_id, attribute_id, store_id)
);

CREATE TABLE IF NOT EXISTS catalog_product_entity_datetime (
    value_id      BIGSERIAL PRIMARY KEY,
    attribute_id  INTEGER NOT NULL DEFAULT 0,
    store_id      INTEGER NOT NULL DEFAULT 0,
    entity_id     BIGINT NOT NULL DEFAULT 0,
    value         TIMESTAMP,
    UNIQUE (entity_id, attribute_id, store_id)
);

CREATE TABLE IF NOT EXISTS cataloginventory_stock_item (
    item_id        BIGSERIAL PRIMARY KEY,
    product_id     BIGINT NOT NULL,
    stock_id       INTEGER NOT NULL,
    qty            DOUBLE PRECISION,
    min_qty        DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_qty_decimal INTEGER NOT NULL DEFAULT 0,
    backorders     INTEGER NOT NULL DEFAULT 0,
    min_sale_qty   DOUBLE PRECISION NOT NULL DEFAULT 1,
    max_sale_qty   DOUBLE PRECISION NOT NULL DEFAULT 0,
    is_in_stock    INTEGER NOT NULL DEFAULT 0,
    manage_stock   INTEGER NOT NULL DEFAULT 0,
    website_id     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (product_id, stock_id)
);

CREATE TABLE IF NOT EXISTS catalog_product_index_price (
    entity_id          BIGINT NOT NULL,
    customer_group_id  BIGINT NOT NULL,
    website_id         INTEGER NOT NULL,
    tax_class_id       INTEGER DEFAULT 0,
    price              DOUBLE PRECISION,
    final_price        DOUBLE PRECISION,
    min_price          DOUBLE PRECISION,
    max_price          DOUBLE PRECISION,
    tier_price         DOUBLE PRECISION,
    PRIMARY KEY (entity_id, customer_group_id, website_id)
);

-- Seed the 40 EAV attributes the synthetic-import fixtures use (the
-- 13-column benchmark CSV plus the 27 extra columns the 100k-row/
-- 40-attribute performance fixture adds), matching the same
-- entity_type_id=4 (catalog_product) / attribute_code / backend_type shape
-- seeded into the MySQL side (see the top-level README's "Performance:
-- 100k products / 40 attributes" section for how the MySQL side was
-- seeded) -- without this, bucket_rows() has no attribute to resolve any
-- of a CSV's non-static columns against.
INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
VALUES
    (4, 'name', 'varchar', 1, 0, 0),
    (4, 'meta_title', 'varchar', 0, 0, 0),
    (4, 'url_key', 'varchar', 0, 0, 0),
    (4, 'meta_keywords', 'varchar', 0, 0, 0),
    (4, 'brand', 'varchar', 0, 0, 0),
    (4, 'manufacturer', 'varchar', 0, 0, 0),
    (4, 'model_number', 'varchar', 0, 0, 0),
    (4, 'color_family', 'varchar', 0, 0, 0),
    (4, 'material', 'varchar', 0, 0, 0),
    (4, 'country_of_origin', 'varchar', 0, 0, 0),
    (4, 'description', 'text', 0, 0, 0),
    (4, 'short_description', 'text', 0, 0, 0),
    (4, 'features', 'text', 0, 0, 0),
    (4, 'specifications', 'text', 0, 0, 0),
    (4, 'care_instructions', 'text', 0, 0, 0),
    (4, 'warranty_info', 'text', 0, 0, 0),
    (4, 'ingredients', 'text', 0, 0, 0),
    (4, 'additional_info', 'text', 0, 0, 0),
    (4, 'color', 'int', 0, 0, 0),
    (4, 'size', 'int', 0, 0, 0),
    (4, 'status', 'int', 0, 0, 0),
    (4, 'visibility', 'int', 0, 0, 0),
    (4, 'is_featured', 'int', 0, 0, 0),
    (4, 'warranty_months', 'int', 0, 0, 0),
    (4, 'min_order_qty', 'int', 0, 0, 0),
    (4, 'stock_threshold', 'int', 0, 0, 0),
    (4, 'package_count', 'int', 0, 0, 0),
    (4, 'tax_class', 'int', 0, 0, 0),
    (4, 'price', 'decimal', 1, 0, 0),
    (4, 'weight', 'decimal', 0, 0, 0),
    (4, 'special_price', 'decimal', 0, 0, 0),
    (4, 'cost', 'decimal', 0, 0, 0),
    (4, 'msrp', 'decimal', 0, 0, 0),
    (4, 'shipping_weight', 'decimal', 0, 0, 0),
    (4, 'length', 'decimal', 0, 0, 0),
    (4, 'width', 'decimal', 0, 0, 0),
    (4, 'special_from_date', 'datetime', 0, 0, 0),
    (4, 'special_to_date', 'datetime', 0, 0, 0),
    (4, 'news_from_date', 'datetime', 0, 0, 0),
    (4, 'news_to_date', 'datetime', 0, 0, 0)
ON CONFLICT (entity_type_id, attribute_code) DO NOTHING;
