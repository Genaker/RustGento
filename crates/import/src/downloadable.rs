use crate::csv_parse::ParsedCsv;
use entity::{DownloadableLink, DownloadableSample};
use sqlx::MySqlPool;
use std::collections::HashMap;

pub const DOWNLOADABLE_LINK_COLUMN: &str = "downloadable_links";
pub const DOWNLOADABLE_SAMPLE_COLUMN: &str = "downloadable_samples";

/// Collects the "downloadable_links" and "downloadable_samples" columns.
///
///   downloadable_links:   "title:price:number_of_downloads:url" entries, ";"-separated
///   downloadable_samples: "title:url" entries, ";"-separated
///
/// Example: "Album MP3:9.99:0:https://example.com/album.zip;Bonus Track:1.99:5:https://example.com/bonus.mp3"
///
/// Returns (links, samples, touched product ids, warnings). "touched"
/// records every product id this import provides a link/sample set for,
/// so [`flush_downloadable`] knows which products to fully replace even
/// if a product's set becomes empty (e.g. every entry warned and was
/// skipped).
pub fn collect_downloadable(
    csv: &ParsedCsv,
    sku_to_id: &HashMap<String, u64>,
) -> (Vec<DownloadableLink>, Vec<DownloadableSample>, Vec<u64>, Vec<String>) {
    let mut links = Vec::new();
    let mut samples = Vec::new();
    let mut touched = Vec::new();
    let mut warnings = Vec::new();

    let link_col = csv.col_index(DOWNLOADABLE_LINK_COLUMN);
    let sample_col = csv.col_index(DOWNLOADABLE_SAMPLE_COLUMN);
    if link_col.is_none() && sample_col.is_none() {
        return (links, samples, touched, warnings);
    }

    for row in &csv.rows {
        let Some(sku) = csv.sku(row) else { continue };
        let Some(&product_id) = sku_to_id.get(sku) else { continue };

        if let Some(col) = link_col {
            if let Some(val) = csv.field(row, col) {
                touched.push(product_id);
                for entry in val.split(';') {
                    let entry = entry.trim();
                    if entry.is_empty() {
                        continue;
                    }
                    match parse_link(sku, product_id, entry) {
                        Ok(link) => links.push(link),
                        Err(w) => warnings.push(w),
                    }
                }
            }
        }
        if let Some(col) = sample_col {
            if let Some(val) = csv.field(row, col) {
                touched.push(product_id);
                for entry in val.split(';') {
                    let entry = entry.trim();
                    if entry.is_empty() {
                        continue;
                    }
                    match parse_sample(sku, product_id, entry) {
                        Ok(sample) => samples.push(sample),
                        Err(w) => warnings.push(w),
                    }
                }
            }
        }
    }

    (links, samples, touched, warnings)
}

fn parse_link(sku: &str, product_id: u64, entry: &str) -> Result<DownloadableLink, String> {
    // splitn(4, ':'): the URL is always last and may itself contain colons
    // (e.g. "https://..."), so it must not be split further.
    let fields: Vec<&str> = entry.splitn(4, ':').collect();
    let title = fields[0].trim();
    if title.is_empty() {
        return Err(format!("sku={sku}: downloadable link entry {entry:?} has no title"));
    }

    let mut link = DownloadableLink { link_id: 0, product_id: product_id as u32, title: title.to_string(), price: 0.0, number_of_downloads: 0, link_url: None, sort_order: 0 };
    if let Some(f) = fields.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(v) = f.parse::<f64>() {
            link.price = v;
        }
    }
    if let Some(f) = fields.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(v) = f.parse::<i64>() {
            link.number_of_downloads = v;
        }
    }
    if let Some(f) = fields.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        link.link_url = Some(f.to_string());
    }
    Ok(link)
}

fn parse_sample(sku: &str, product_id: u64, entry: &str) -> Result<DownloadableSample, String> {
    // splitn(2, ':'): the URL is always last and may itself contain colons.
    let fields: Vec<&str> = entry.splitn(2, ':').collect();
    let title = fields[0].trim();
    if title.is_empty() {
        return Err(format!("sku={sku}: downloadable sample entry {entry:?} has no title"));
    }

    let mut sample = DownloadableSample { sample_id: 0, product_id: product_id as u32, title: title.to_string(), sample_url: None, sort_order: 0 };
    if let Some(f) = fields.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sample.sample_url = Some(f.to_string());
    }
    Ok(sample)
}

/// Replaces each touched product's full link/sample set -- same
/// full-replace-on-reimport approach as custom options, so reimporting the
/// same CSV doesn't accumulate duplicates.
pub async fn flush_downloadable(
    pool: &MySqlPool,
    links: &[DownloadableLink],
    samples: &[DownloadableSample],
    touched: &[u64],
    batch_size: usize,
) -> Result<(), sqlx::Error> {
    if touched.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;

    let placeholders = vec!["?"; touched.len()].join(",");
    {
        let sql = format!("DELETE FROM downloadable_link WHERE product_id IN ({placeholders})");
        let mut q = sqlx::query(&sql);
        for id in touched {
            q = q.bind(*id as u32);
        }
        q.execute(&mut *tx).await?;
    }
    {
        let sql = format!("DELETE FROM downloadable_sample WHERE product_id IN ({placeholders})");
        let mut q = sqlx::query(&sql);
        for id in touched {
            q = q.bind(*id as u32);
        }
        q.execute(&mut *tx).await?;
    }

    for chunk in links.chunks(batch_size.max(1)) {
        let mut qb: sqlx::QueryBuilder<sqlx::MySql> =
            sqlx::QueryBuilder::new("INSERT INTO downloadable_link (product_id, title, price, number_of_downloads, link_url) ");
        qb.push_values(chunk, |mut b, l: &DownloadableLink| {
            b.push_bind(l.product_id).push_bind(&l.title).push_bind(l.price).push_bind(l.number_of_downloads).push_bind(&l.link_url);
        });
        qb.build().execute(&mut *tx).await?;
    }
    for chunk in samples.chunks(batch_size.max(1)) {
        let mut qb: sqlx::QueryBuilder<sqlx::MySql> =
            sqlx::QueryBuilder::new("INSERT INTO downloadable_sample (product_id, title, sample_url) ");
        qb.push_values(chunk, |mut b, s: &DownloadableSample| {
            b.push_bind(s.product_id).push_bind(&s.title).push_bind(&s.sample_url);
        });
        qb.build().execute(&mut *tx).await?;
    }

    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(data: &str) -> ParsedCsv {
        crate::csv_parse::parse_csv(std::io::Cursor::new(data.as_bytes().to_vec())).unwrap()
    }

    #[test]
    fn no_columns_is_a_no_op() {
        let csv = parse("sku,name\nSKU-A,Widget\n");
        let (links, samples, touched, warnings) = collect_downloadable(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(links.is_empty());
        assert!(samples.is_empty());
        assert!(touched.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn links_and_samples_together() {
        let csv = parse(
            "sku,downloadable_links,downloadable_samples\n\
             SKU-A,\"Album MP3:9.99:0:https://example.com/album.zip;Bonus Track:1.99:5:https://example.com/bonus.mp3\",Preview Clip:https://example.com/preview.mp3\n",
        );
        let (links, samples, touched, warnings) = collect_downloadable(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(warnings.is_empty());
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].title, "Album MP3");
        assert_eq!(links[0].price, 9.99);
        assert_eq!(links[0].link_url.as_deref(), Some("https://example.com/album.zip"));
        assert_eq!(links[1].number_of_downloads, 5);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].title, "Preview Clip");
        assert_eq!(touched, vec![1, 1]);
    }

    #[test]
    fn missing_title_warns_and_is_skipped() {
        let csv = parse("sku,downloadable_links\nSKU-A,\":5:0:https://example.com/x.mp3\"\n");
        let (links, _, _, warnings) = collect_downloadable(&csv, &HashMap::from([("SKU-A".to_string(), 1u64)]));
        assert!(links.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no title"));
    }

    #[test]
    fn unknown_sku_is_skipped() {
        let csv = parse("sku,downloadable_links\nSKU-A,Track:5:0\n");
        let (links, _, touched, _) = collect_downloadable(&csv, &HashMap::new());
        assert!(links.is_empty());
        assert!(touched.is_empty());
    }

    #[tokio::test]
    async fn flush_replaces_fully_on_reimport() {
        let Some(pool) = crate::test_support::test_pool().await else { return };

        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-DL-TEST-1'").execute(&pool).await.unwrap();
        let product_id = sqlx::query("INSERT INTO catalog_product_entity (attribute_set_id, type_id, sku) VALUES (4, 'downloadable', 'RUST-DL-TEST-1')")
            .execute(&pool).await.unwrap().last_insert_id();

        let links1 = vec![DownloadableLink { link_id: 0, product_id: product_id as u32, title: "Old".into(), price: 5.0, number_of_downloads: 0, link_url: None, sort_order: 0 }];
        flush_downloadable(&pool, &links1, &[], &[product_id], 500).await.unwrap();

        let links2 = vec![DownloadableLink { link_id: 0, product_id: product_id as u32, title: "New".into(), price: 5.0, number_of_downloads: 0, link_url: None, sort_order: 0 }];
        flush_downloadable(&pool, &links2, &[], &[product_id], 500).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloadable_link WHERE product_id = ?").bind(product_id as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "full replace, not accumulation");
        let title: String = sqlx::query_scalar("SELECT title FROM downloadable_link WHERE product_id = ?").bind(product_id as u32).fetch_one(&pool).await.unwrap();
        assert_eq!(title, "New");

        sqlx::query("DELETE FROM downloadable_link WHERE product_id = ?").bind(product_id as u32).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM catalog_product_entity WHERE sku = 'RUST-DL-TEST-1'").execute(&pool).await.unwrap();
    }
}
