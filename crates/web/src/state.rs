use repository::{AttributeCodeMap, CategoryAttributeMeta, CategoryTreeNode, FlatCache};
use sqlx::MySqlPool;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// How long a rendered category-tree nav fragment is reused before being
/// rebuilt from the database -- mirrors Go's 30-minute in-process cache for
/// the same fragment (`RenderCategoryTreeCached`).
const CATEGORY_TREE_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub struct WebState {
    pub pool: MySqlPool,
    pub product_cache: Arc<FlatCache>,
    pub category_cache: Arc<FlatCache>,
    pub product_code_map: Arc<AttributeCodeMap>,
    pub category_meta: Arc<CategoryAttributeMeta>,
    pub media_url: String,
    category_tree_html: Arc<RwLock<Option<(Instant, String)>>>,
}

impl WebState {
    pub async fn new(pool: MySqlPool) -> Result<Self, sqlx::Error> {
        let product_code_map = repository::product_db::load_attribute_code_map(&pool).await?;
        let category_meta = repository::category_db::load_attribute_meta(&pool).await?;
        Ok(WebState {
            pool,
            product_cache: Arc::new(FlatCache::new()),
            category_cache: Arc::new(FlatCache::new()),
            product_code_map: Arc::new(product_code_map),
            category_meta: Arc::new(category_meta),
            media_url: config::media_url(),
            category_tree_html: Arc::new(RwLock::new(None)),
        })
    }

    /// Returns the cached nav-menu HTML fragment, rebuilding it from the
    /// database if it's missing or older than [`CATEGORY_TREE_TTL`].
    pub async fn category_tree_html(&self) -> Result<String, sqlx::Error> {
        if let Some((built_at, html)) = self.category_tree_html.read().unwrap().as_ref() {
            if built_at.elapsed() < CATEGORY_TREE_TTL {
                return Ok(html.clone());
            }
        }

        let tree = repository::build_tree(&self.pool, &self.category_meta, 0).await?;
        let html = render_category_tree(&tree);
        *self.category_tree_html.write().unwrap() = Some((Instant::now(), html.clone()));
        Ok(html)
    }
}

/// Recursively renders a category tree into the same nested `<li><a><ul>`
/// shape Go's `category_tree` template produces. Rendered once and cached
/// as a plain HTML string (see [`WebState::category_tree_html`]) rather
/// than re-walked on every request, since the tree rarely changes.
fn render_category_tree(nodes: &[CategoryTreeNode]) -> String {
    let mut out = String::new();
    render_category_tree_into(nodes, &mut out);
    out
}

fn render_category_tree_into(nodes: &[CategoryTreeNode], out: &mut String) {
    for node in nodes {
        let label = node.name.as_deref().unwrap_or(&node.path);
        out.push_str(&format!(
            r#"<li><a href="/category/{id}" class="block px-2 py-2 hover:bg-blue-50">{label}</a>"#,
            id = node.entity_id,
            label = html_escape(label),
        ));
        if !node.children.is_empty() {
            out.push_str(r#"<ul class="ml-4 border-l border-blue-100">"#);
            render_category_tree_into(&node.children, out);
            out.push_str("</ul>");
        }
        out.push_str("</li>");
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, name: &str, children: Vec<CategoryTreeNode>) -> CategoryTreeNode {
        CategoryTreeNode { entity_id: id, parent_id: 0, name: Some(name.to_string()), path: String::new(), level: 0, children }
    }

    #[test]
    fn renders_flat_list() {
        let tree = vec![node(2, "Shoes", vec![]), node(3, "Hats", vec![])];
        let html = render_category_tree(&tree);
        assert!(html.contains(r#"href="/category/2""#));
        assert!(html.contains("Shoes"));
        assert!(html.contains(r#"href="/category/3""#));
        assert!(html.contains("Hats"));
    }

    #[test]
    fn renders_nested_children() {
        let tree = vec![node(2, "Shoes", vec![node(5, "Running", vec![])])];
        let html = render_category_tree(&tree);
        assert!(html.contains("<ul"));
        assert!(html.contains(r#"href="/category/5""#));
        assert!(html.contains("Running"));
    }

    #[test]
    fn falls_back_to_path_when_name_is_missing() {
        let tree = vec![CategoryTreeNode { entity_id: 9, parent_id: 0, name: None, path: "1/9".to_string(), level: 1, children: vec![] }];
        let html = render_category_tree(&tree);
        assert!(html.contains("1/9"));
    }

    #[test]
    fn escapes_category_names() {
        let tree = vec![node(2, "Foo & <Bar>", vec![])];
        let html = render_category_tree(&tree);
        assert!(html.contains("Foo &amp; &lt;Bar&gt;"));
        assert!(!html.contains("<Bar>"));
    }
}
