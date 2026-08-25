use crate::category_repo::{flatten_category, CategoryAttributeMeta, CategoryEavRows};
use crate::flat_cache::FlatCache;
use crate::product_db::batch_by_ids;
use entity::{Category, CategoryInt, CategoryText, CategoryVarchar, EavAttribute};
use serde_json::{Map, Value};
use sqlx::MySqlPool;
use std::collections::HashMap;

/// `entity_type_id` for Magento's `catalog_category` EAV entity type.
const CATEGORY_ENTITY_TYPE_ID: u16 = 3;

/// Static (non-EAV) category fields that go at the top level of the response
/// rather than nested under `"attributes"` -- mirrors Go's `CategoryWithAttributes`,
/// which embeds the raw `Category` struct fields alongside an `attributes` map.
const STATIC_FIELDS: [&str; 4] = ["entity_id", "parent_id", "path", "level"];

pub async fn load_attribute_meta(pool: &MySqlPool) -> Result<CategoryAttributeMeta, sqlx::Error> {
    let attrs: Vec<EavAttribute> = sqlx::query_as("SELECT * FROM eav_attribute WHERE entity_type_id = ?")
        .bind(CATEGORY_ENTITY_TYPE_ID)
        .fetch_all(pool)
        .await?;
    Ok(CategoryAttributeMeta::build(&attrs))
}

pub async fn find_all(pool: &MySqlPool) -> Result<Vec<Category>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM catalog_category_entity ORDER BY entity_id").fetch_all(pool).await
}

pub async fn find_by_id(pool: &MySqlPool, id: u64) -> Result<Option<Category>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM catalog_category_entity WHERE entity_id = ?").bind(id).fetch_optional(pool).await
}

/// Splits a flattened category map into `(top-level fields, attributes map)`,
/// matching the REST/GraphQL response shape.
fn split_static_and_attributes(flat: Map<String, Value>) -> Value {
    let mut top = Map::new();
    let mut attributes = Map::new();
    for (k, v) in flat {
        if STATIC_FIELDS.contains(&k.as_str()) {
            top.insert(k, v);
        } else {
            attributes.insert(k, v);
        }
    }
    top.insert("attributes".to_string(), Value::Object(attributes));
    Value::Object(top)
}

async fn batch_flatten(pool: &MySqlPool, meta: &CategoryAttributeMeta, categories: &[Category]) -> Result<HashMap<u64, Value>, sqlx::Error> {
    let ids: Vec<u64> = categories.iter().map(|c| c.entity_id).collect();

    let int = batch_by_ids::<CategoryInt, _, _>(pool, "SELECT * FROM catalog_category_entity_int WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let varchar = batch_by_ids::<CategoryVarchar, _, _>(pool, "SELECT * FROM catalog_category_entity_varchar WHERE entity_id IN", &ids, |r| r.entity_id).await?;
    let text = batch_by_ids::<CategoryText, _, _>(pool, "SELECT * FROM catalog_category_entity_text WHERE entity_id IN", &ids, |r| r.entity_id).await?;

    let mut out = HashMap::with_capacity(categories.len());
    for cat in categories {
        let id = cat.entity_id;
        let rows = CategoryEavRows {
            int: int.get(&id).cloned().unwrap_or_default(),
            varchar: varchar.get(&id).cloned().unwrap_or_default(),
            text: text.get(&id).cloned().unwrap_or_default(),
        };
        let flat = flatten_category(cat, &rows, meta);
        out.insert(id, split_static_and_attributes(flat));
    }
    Ok(out)
}

pub async fn fetch_flat_by_id(
    pool: &MySqlPool,
    cache: &FlatCache,
    meta: &CategoryAttributeMeta,
    store_id: u16,
    entity_id: u64,
) -> Result<Option<Value>, sqlx::Error> {
    if let Some(cached) = cache.get(false, store_id, entity_id) {
        return Ok(Some(cached));
    }
    let Some(category) = find_by_id(pool, entity_id).await? else { return Ok(None) };
    let flattened = batch_flatten(pool, meta, std::slice::from_ref(&category)).await?;
    let Some(flat) = flattened.into_values().next() else { return Ok(None) };
    cache.put(false, store_id, entity_id, flat.clone());
    Ok(Some(flat))
}

pub async fn fetch_flat_list(pool: &MySqlPool, cache: &FlatCache, meta: &CategoryAttributeMeta, store_id: u16) -> Result<Vec<Value>, sqlx::Error> {
    let categories = find_all(pool).await?;
    if categories.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = HashMap::new();
    let mut missing = Vec::new();
    for c in &categories {
        match cache.get(false, store_id, c.entity_id) {
            Some(cached) => {
                results.insert(c.entity_id, cached);
            }
            None => missing.push(c.clone()),
        }
    }
    if !missing.is_empty() {
        let fetched = batch_flatten(pool, meta, &missing).await?;
        for (id, flat) in fetched {
            cache.put(false, store_id, id, flat.clone());
            results.insert(id, flat);
        }
    }

    Ok(categories.iter().filter_map(|c| results.get(&c.entity_id).cloned()).collect())
}

pub async fn fetch_flat_by_ids(pool: &MySqlPool, cache: &FlatCache, meta: &CategoryAttributeMeta, store_id: u16, ids: &[u64]) -> Result<Vec<Value>, sqlx::Error> {
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some(flat) = fetch_flat_by_id(pool, cache, meta, store_id, id).await? {
            out.push(flat);
        }
    }
    Ok(out)
}

/// One node of the category tree: enough fields for a storefront nav menu
/// (id, name if the category has one, and its children).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryTreeNode {
    pub entity_id: u64,
    pub parent_id: u64,
    pub name: Option<String>,
    pub path: String,
    pub level: i64,
    pub children: Vec<CategoryTreeNode>,
}

/// Builds the full category tree rooted at `root_parent_id` (Magento's real
/// root category typically has `entity_id=1, parent_id=0`; pass `0` for the
/// conventional root).
///
/// Looks children up by `entity_id`, widened to match `parent_id`'s `u64`
/// tree-node type even though the live schema declares `parent_id` as the
/// narrower `int unsigned` (`u32`) -- both columns hold the same category-ID
/// value space, just with an inconsistent width inherited from Go's own
/// struct tags, so the widen is a value-preserving cast, not a truncation.
pub async fn build_tree(pool: &MySqlPool, meta: &CategoryAttributeMeta, root_parent_id: u64) -> Result<Vec<CategoryTreeNode>, sqlx::Error> {
    let categories = find_all(pool).await?;
    let flattened = batch_flatten(pool, meta, &categories).await?;

    let mut children_of: HashMap<u64, Vec<&Category>> = HashMap::new();
    for c in &categories {
        children_of.entry(c.parent_id as u64).or_default().push(c);
    }

    fn build_node(id: u64, categories_by_parent: &HashMap<u64, Vec<&Category>>, flattened: &HashMap<u64, Value>) -> Vec<CategoryTreeNode> {
        let Some(siblings) = categories_by_parent.get(&id) else { return Vec::new() };
        siblings
            .iter()
            .map(|c| {
                let name = flattened
                    .get(&c.entity_id)
                    .and_then(|v| v["attributes"]["name"]["value"].as_str())
                    .map(str::to_string);
                CategoryTreeNode {
                    entity_id: c.entity_id,
                    parent_id: c.parent_id as u64,
                    name,
                    path: c.path.clone(),
                    level: c.level,
                    children: build_node(c.entity_id, categories_by_parent, flattened),
                }
            })
            .collect()
    }

    Ok(build_node(root_parent_id, &children_of, &flattened))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_flat_list_and_by_id_agree_and_populate_cache() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let cache = FlatCache::new();
        let meta = load_attribute_meta(&pool).await.unwrap();

        let list = fetch_flat_list(&pool, &cache, &meta, 0).await.unwrap();
        assert!(!list.is_empty(), "seed data must have at least one category");
        assert_eq!(cache.len_for_store(0), list.len());

        let first_id = list[0]["entity_id"].as_u64().unwrap();
        let single = fetch_flat_by_id(&pool, &cache, &meta, 0, first_id).await.unwrap().unwrap();
        assert_eq!(single["entity_id"], list[0]["entity_id"]);
        assert!(single["attributes"].is_object());
    }

    #[tokio::test]
    async fn fetch_flat_by_id_returns_none_for_unknown_id() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let cache = FlatCache::new();
        let meta = load_attribute_meta(&pool).await.unwrap();
        assert!(fetch_flat_by_id(&pool, &cache, &meta, 0, 999_999_999).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn build_tree_produces_a_rooted_hierarchy() {
        let Some(pool) = crate::test_support::test_pool().await else { return };
        let meta = load_attribute_meta(&pool).await.unwrap();
        let tree = build_tree(&pool, &meta, 0).await.unwrap();
        // The seed data creates a root category (parent_id=0) with one child.
        assert!(!tree.is_empty());
        assert_eq!(tree[0].parent_id, 0);
    }
}
