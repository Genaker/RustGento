use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Maps to `catalog_product_entity_media_gallery` -- a global value pool
/// with no `entity_id` of its own; linking to a specific product goes
/// through [`ProductMediaGalleryValueToEntity`]. Real Magento also has a
/// per-store `catalog_product_entity_media_gallery_value` table for
/// label/position/disabled overrides; this project simplifies that by
/// keeping `disabled` directly on this pool row instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductMediaGallery {
    pub value_id: u64,
    pub attribute_id: u16,
    pub value: Option<String>,
    pub media_type: String,
    pub disabled: u16,
}

/// Maps to `catalog_product_entity_media_gallery_value_to_entity` --
/// composite primary key `(entity_id, value_id)`, no separate id column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, FromRow)]
pub struct ProductMediaGalleryValueToEntity {
    pub entity_id: u64,
    pub value_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_media_gallery_round_trip() {
        let g = ProductMediaGallery {
            value_id: 1,
            attribute_id: 87,
            value: Some("/m/y/image1.jpg".into()),
            media_type: "image".into(),
            disabled: 0,
        };
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<ProductMediaGallery>(&json).unwrap(), g);
    }

    #[test]
    fn value_to_entity_round_trip() {
        let l = ProductMediaGalleryValueToEntity { entity_id: 5, value_id: 1 };
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<ProductMediaGalleryValueToEntity>(&json).unwrap(), l);
    }
}
