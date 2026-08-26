//! Entity layer — plain data structs over the Magento EAV schema.
//! CE (`entity_id`) schema only; EE (`row_id`) support is an explicit
//! non-goal (see the top-level README).

pub mod backend_type;
pub mod category;
pub mod downloadable;
pub mod eav_attribute;
pub mod media_gallery;
pub mod product;
pub mod product_bundle;
pub mod product_link;
pub mod product_option;
pub mod product_super;
pub mod tier_price;

pub use backend_type::BackendType;
pub use category::{Category, CategoryInt, CategoryProduct, CategoryText, CategoryVarchar};
pub use downloadable::{DownloadableLink, DownloadableSample};
pub use eav_attribute::{EavAttribute, CATEGORY_ENTITY_TYPE_ID, PRODUCT_ENTITY_TYPE_ID};
pub use media_gallery::{ProductMediaGallery, ProductMediaGalleryValueToEntity};
pub use product::{
    Product, ProductDatetime, ProductDecimal, ProductIndexPrice, ProductInt, ProductText,
    ProductVarchar, StockItem, DEFAULT_STOCK_ID, GUEST_CUSTOMER_GROUP_ID,
};
pub use product_bundle::{
    is_valid_bundle_option_type, ProductBundleOption, ProductBundleSelection,
    BUNDLE_OPTION_CHECKBOX, BUNDLE_OPTION_MULTI, BUNDLE_OPTION_RADIO, BUNDLE_OPTION_SELECT,
};
pub use product_link::{
    ProductLink, LINK_TYPE_CROSSSELL, LINK_TYPE_GROUPED, LINK_TYPE_RELATED, LINK_TYPE_UPSELL,
};
pub use product_option::{
    is_select_option_type, is_valid_option_type, ProductOption, ProductOptionTypeValue,
    OPTION_TYPE_AREA, OPTION_TYPE_CHECKBOX, OPTION_TYPE_DATE, OPTION_TYPE_DATE_TIME,
    OPTION_TYPE_DROP_DOWN, OPTION_TYPE_FIELD, OPTION_TYPE_FILE, OPTION_TYPE_MULTISELECT,
    OPTION_TYPE_RADIO, OPTION_TYPE_TIME,
};
pub use product_super::{ProductSuperAttribute, ProductSuperLink};
pub use tier_price::TierPrice;
