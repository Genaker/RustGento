use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Maps to `downloadable_link`. Real Magento splits title into
/// `downloadable_link_title` and price into `downloadable_link_price`,
/// both per-store; this project simplifies that to plain columns.
/// `product_id` is the *parent* (downloadable-type) product this link
/// belongs to, not a link to another catalog product.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct DownloadableLink {
    pub link_id: u64,
    pub product_id: u32,
    pub title: String,
    pub price: f64,
    /// 0 means unlimited, matching Magento's own convention.
    pub number_of_downloads: i64,
    pub link_url: Option<String>,
    pub sort_order: i64,
}

/// Maps to `downloadable_sample` -- a free preview attached to a
/// downloadable-type product, as opposed to [`DownloadableLink`], which is
/// the paid content itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct DownloadableSample {
    pub sample_id: u64,
    pub product_id: u32,
    pub title: String,
    pub sample_url: Option<String>,
    pub sort_order: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloadable_link_round_trip() {
        let l = DownloadableLink {
            link_id: 1,
            product_id: 5,
            title: "Album MP3".into(),
            price: 9.99,
            number_of_downloads: 0,
            link_url: Some("https://example.com/album.zip".into()),
            sort_order: 0,
        };
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<DownloadableLink>(&json).unwrap(), l);
    }

    #[test]
    fn downloadable_sample_round_trip() {
        let s = DownloadableSample { sample_id: 1, product_id: 5, title: "Preview".into(), sample_url: None, sort_order: 0 };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<DownloadableSample>(&json).unwrap(), s);
    }
}
