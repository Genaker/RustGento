-- Seeds the 40 `catalog_product` (entity_type_id=4) EAV attributes the
-- "100k products / 40 attributes" performance fixture uses. Idempotent
-- (guarded by WHERE NOT EXISTS, since this project's dev MySQL schema has
-- no unique key on (entity_type_id, attribute_code) to key an
-- INSERT ... ON DUPLICATE KEY UPDATE off of) -- safe to re-run against a
-- fresh `gogento-mysql` or one that already has the original 13-attribute
-- seed from GoGento's seeder. The Postgres equivalent lives in
-- `sql/postgres_schema.sql`.


INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'name' AS attribute_code, 'varchar' AS backend_type, 1 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='name');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'meta_title' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='meta_title');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'url_key' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='url_key');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'meta_keywords' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='meta_keywords');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'brand' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='brand');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'manufacturer' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='manufacturer');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'model_number' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='model_number');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'color_family' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='color_family');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'material' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='material');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'country_of_origin' AS attribute_code, 'varchar' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='country_of_origin');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'description' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='description');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'short_description' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='short_description');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'features' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='features');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'specifications' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='specifications');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'care_instructions' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='care_instructions');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'warranty_info' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='warranty_info');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'ingredients' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='ingredients');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'additional_info' AS attribute_code, 'text' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='additional_info');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'color' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='color');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'size' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='size');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'status' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='status');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'visibility' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='visibility');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'is_featured' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='is_featured');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'warranty_months' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='warranty_months');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'min_order_qty' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='min_order_qty');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'stock_threshold' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='stock_threshold');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'package_count' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='package_count');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'tax_class' AS attribute_code, 'int' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='tax_class');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'price' AS attribute_code, 'decimal' AS backend_type, 1 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='price');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'weight' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='weight');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'special_price' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='special_price');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'cost' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='cost');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'msrp' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='msrp');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'shipping_weight' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='shipping_weight');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'length' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='length');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'width' AS attribute_code, 'decimal' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='width');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'special_from_date' AS attribute_code, 'datetime' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='special_from_date');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'special_to_date' AS attribute_code, 'datetime' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='special_to_date');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'news_from_date' AS attribute_code, 'datetime' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='news_from_date');

INSERT INTO eav_attribute (entity_type_id, attribute_code, backend_type, is_required, is_user_defined, is_unique)
SELECT * FROM (SELECT 4 AS entity_type_id, 'news_to_date' AS attribute_code, 'datetime' AS backend_type, 0 AS is_required, 0 AS is_user_defined, 0 AS is_unique) t
WHERE NOT EXISTS (SELECT 1 FROM eav_attribute WHERE entity_type_id=4 AND attribute_code='news_to_date');
