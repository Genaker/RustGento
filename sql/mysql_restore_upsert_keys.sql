-- Restores the unique keys real Magento (and this project's original
-- dev seed) has on these tables but GORM's AutoMigrate (GoGento's
-- cmd/seed) does not create from the Go struct tags alone. Required for
-- this project's "ON DUPLICATE KEY UPDATE ... value = VALUES(value)"
-- upsert pattern to update in place instead of inserting a duplicate row
-- on every re-import of an existing SKU. Idempotent-ish: re-running
-- against a database that already has these keys errors with "Duplicate
-- key name", which is safe to ignore.

ALTER TABLE catalog_product_entity_varchar ADD UNIQUE KEY unq_entity_attr_store (entity_id, attribute_id, store_id);
ALTER TABLE catalog_product_entity_int ADD UNIQUE KEY unq_entity_attr_store (entity_id, attribute_id, store_id);
ALTER TABLE catalog_product_entity_decimal ADD UNIQUE KEY unq_entity_attr_store (entity_id, attribute_id, store_id);
ALTER TABLE catalog_product_entity_text ADD UNIQUE KEY unq_entity_attr_store (entity_id, attribute_id, store_id);
ALTER TABLE catalog_product_entity_datetime ADD UNIQUE KEY unq_entity_attr_store (entity_id, attribute_id, store_id);
ALTER TABLE cataloginventory_stock_item ADD UNIQUE KEY unq_product_stock (product_id, stock_id);
