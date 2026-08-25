use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;

/// In-process, store-id-keyed cache of flattened product/category maps.
/// Mirrors Go's package-level `flatProductsCache map[uint16]map[uint]map[string]interface{}`:
/// no TTL, no automatic invalidation. Whether that cache emptiness is ever
/// refreshed is entirely up to the caller (`invalidate`/`invalidate_store`) —
/// same as Go, where nothing calls the equivalent reset today. This is
/// replicated deliberately (see plan) since it's load-bearing for the
/// benchmark comparison, not a bug to "fix".
#[derive(Debug, Default)]
pub struct FlatCache {
    inner: RwLock<HashMap<u16, HashMap<u64, Value>>>,
}

impl FlatCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks up a cached flattened entity by (store_id, entity_id). Returns
    /// `None` unconditionally when `bypass` is true — the caller passes in
    /// `!config::product_flat_cache_enabled()` (i.e. `PRODUCT_FLAT_CACHE=off`),
    /// keeping this crate decoupled from the `config` crate.
    pub fn get(&self, bypass: bool, store_id: u16, entity_id: u64) -> Option<Value> {
        if bypass {
            return None;
        }
        self.inner
            .read()
            .unwrap()
            .get(&store_id)
            .and_then(|m| m.get(&entity_id))
            .cloned()
    }

    /// Populates the cache for one (store_id, entity_id). A no-op when
    /// `bypass` is true, matching Go's behavior of never populating the
    /// cache while `PRODUCT_FLAT_CACHE=off`.
    pub fn put(&self, bypass: bool, store_id: u16, entity_id: u64, value: Value) {
        if bypass {
            return;
        }
        self.inner
            .write()
            .unwrap()
            .entry(store_id)
            .or_default()
            .insert(entity_id, value);
    }

    pub fn invalidate_store(&self, store_id: u16) {
        self.inner.write().unwrap().remove(&store_id);
    }

    pub fn invalidate_all(&self) {
        self.inner.write().unwrap().clear();
    }

    pub fn len_for_store(&self, store_id: u16) -> usize {
        self.inner
            .read()
            .unwrap()
            .get(&store_id)
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn miss_on_empty_cache() {
        let cache = FlatCache::new();
        assert_eq!(cache.get(false, 1, 42), None);
    }

    #[test]
    fn put_then_get_hits() {
        let cache = FlatCache::new();
        cache.put(false, 1, 42, json!({"sku": "A"}));
        assert_eq!(cache.get(false, 1, 42), Some(json!({"sku": "A"})));
        assert_eq!(cache.len_for_store(1), 1);
    }

    #[test]
    fn different_store_ids_are_isolated() {
        let cache = FlatCache::new();
        cache.put(false, 1, 42, json!({"store": 1}));
        cache.put(false, 2, 42, json!({"store": 2}));
        assert_eq!(cache.get(false, 1, 42), Some(json!({"store": 1})));
        assert_eq!(cache.get(false, 2, 42), Some(json!({"store": 2})));
    }

    #[test]
    fn bypass_true_never_reads_or_writes() {
        let cache = FlatCache::new();
        cache.put(false, 1, 42, json!({"sku": "A"}));
        assert_eq!(cache.get(true, 1, 42), None, "bypass must ignore existing entries");
        cache.put(true, 1, 99, json!({"sku": "B"}));
        assert_eq!(cache.get(false, 1, 99), None, "bypass=true put must not populate the cache");
    }

    #[test]
    fn invalidate_store_clears_only_that_store() {
        let cache = FlatCache::new();
        cache.put(false, 1, 42, json!({}));
        cache.put(false, 2, 42, json!({}));
        cache.invalidate_store(1);
        assert_eq!(cache.get(false, 1, 42), None);
        assert_eq!(cache.get(false, 2, 42), Some(json!({})));
    }

    #[test]
    fn invalidate_all_clears_everything() {
        let cache = FlatCache::new();
        cache.put(false, 1, 42, json!({}));
        cache.put(false, 2, 7, json!({}));
        cache.invalidate_all();
        assert_eq!(cache.get(false, 1, 42), None);
        assert_eq!(cache.get(false, 2, 7), None);
    }

    #[test]
    fn len_for_store_reports_zero_for_unknown_store() {
        let cache = FlatCache::new();
        assert_eq!(cache.len_for_store(99), 0);
    }
}
