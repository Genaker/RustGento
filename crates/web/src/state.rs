use repository::{AttributeCodeMap, CategoryAttributeMeta, CategoryTreeNode, FlatCache};
use sqlx::MySqlPool;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// How long a rendered category-tree nav fragment is reused before being
/// rebuilt from the database -- mirrors Go's 30-minute in-process cache for
/// the same fragment (`RenderCategoryTreeCached`).
const CATEGORY_TREE_TTL: Duration = Duration::from_secs(30 * 60);

/// The two cached nav fragments built from the same category tree fetch:
/// the full recursive tree (mobile slide-out menu) and a shallower
/// top-level-with-hover-dropdown bar (desktop menu).
#[derive(Clone)]
struct NavHtml {
    full_tree: String,
    top_nav: String,
}

#[derive(Clone)]
pub struct WebState {
    pub pool: MySqlPool,
    pub product_cache: Arc<FlatCache>,
    pub category_cache: Arc<FlatCache>,
    pub product_code_map: Arc<AttributeCodeMap>,
    pub category_meta: Arc<CategoryAttributeMeta>,
    pub media_url: String,
    /// Resolved once at startup so a search request doesn't need its own
    /// `eav_attribute` lookup on every call.
    pub name_attribute_id: u16,
    nav_html: Arc<RwLock<Option<(Instant, NavHtml)>>>,
}

impl WebState {
    pub async fn new(pool: MySqlPool) -> Result<Self, sqlx::Error> {
        let product_code_map = repository::product_db::load_attribute_code_map(&pool).await?;
        let category_meta = repository::category_db::load_attribute_meta(&pool).await?;
        let name_attribute_id: u16 = sqlx::query_scalar("SELECT attribute_id FROM eav_attribute WHERE entity_type_id = 4 AND attribute_code = 'name'")
            .fetch_optional(&pool)
            .await?
            .unwrap_or(0);
        Ok(WebState {
            pool,
            product_cache: Arc::new(FlatCache::new()),
            category_cache: Arc::new(FlatCache::new()),
            product_code_map: Arc::new(product_code_map),
            category_meta: Arc::new(category_meta),
            media_url: config::media_url(),
            name_attribute_id,
            nav_html: Arc::new(RwLock::new(None)),
        })
    }

    /// Both nav fragments at once (full recursive tree for the mobile
    /// slide-out, top-level-with-dropdown for the desktop menu bar),
    /// rebuilding from the database if missing or older than
    /// [`CATEGORY_TREE_TTL`], and falling
    /// back to empty strings (rather than failing the whole page) if the
    /// tree can't be rebuilt -- every page still renders without its nav
    /// menus rather than not at all.
    pub async fn nav_fragments(&self) -> (String, String) {
        match self.nav_html().await {
            Ok(html) => (html.full_tree, html.top_nav),
            Err(e) => {
                tracing::warn!("category tree render failed: {e}");
                (String::new(), String::new())
            }
        }
    }

    async fn nav_html(&self) -> Result<NavHtml, sqlx::Error> {
        if let Some((built_at, html)) = self.nav_html.read().unwrap().as_ref() {
            if built_at.elapsed() < CATEGORY_TREE_TTL {
                return Ok(html.clone());
            }
        }

        let tree = repository::build_tree(&self.pool, &self.category_meta, 0).await?;
        let html = NavHtml { full_tree: render_category_tree(&tree), top_nav: render_top_nav(&tree) };
        *self.nav_html.write().unwrap() = Some((Instant::now(), html.clone()));
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

/// Renders the desktop menu bar: one entry per top-level category, each
/// with a hover-revealed dropdown listing its (recursively rendered)
/// children -- unlike the mobile slide-out, this only shows a flyout for
/// categories that actually have children, not the whole tree at once.
fn render_top_nav(nodes: &[CategoryTreeNode]) -> String {
    let mut out = String::new();
    for node in nodes {
        let label = node.name.as_deref().unwrap_or(&node.path);
        out.push_str(&format!(
            r#"<li class="relative group"><a href="/category/{id}" class="px-3 py-2 inline-block hover:text-yellow-300">{label}</a>"#,
            id = node.entity_id,
            label = html_escape(label),
        ));
        if !node.children.is_empty() {
            out.push_str(r#"<ul class="absolute left-0 top-full hidden group-hover:block bg-white text-gray-800 rounded shadow-lg py-2 min-w-[180px] z-50">"#);
            render_category_tree_into(&node.children, &mut out);
            out.push_str("</ul>");
        }
        out.push_str("</li>");
    }
    out
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
    fn top_nav_renders_a_dropdown_only_for_categories_with_children() {
        let tree = vec![node(2, "Shoes", vec![node(5, "Running", vec![])]), node(3, "Hats", vec![])];
        let html = render_top_nav(&tree);
        assert!(html.contains(r#"href="/category/2""#));
        assert!(html.contains("group-hover:block"));
        assert!(html.contains(r#"href="/category/5""#), "Shoes' dropdown must list its child");
        // Hats has no children, so it shouldn't get a dropdown <ul> of its own.
        let hats_onward = &html[html.find("Hats").unwrap()..];
        assert!(!hats_onward.starts_with("Hats</a><ul"));
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
