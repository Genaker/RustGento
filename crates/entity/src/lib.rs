//! Entity layer — plain data structs over the Magento EAV schema.
//! CE (`entity_id`) schema only; EE (`row_id`) support is an explicit
//! non-goal (see the top-level README).

pub mod backend_type;
pub mod category;
pub mod eav_attribute;
pub mod product;

pub use backend_type::BackendType;
pub use category::{Category, CategoryInt, CategoryProduct, CategoryText, CategoryVarchar};
pub use eav_attribute::{EavAttribute, PRODUCT_ENTITY_TYPE_ID};
pub use product::{
    Product, ProductDatetime, ProductDecimal, ProductIndexPrice, ProductInt, ProductText,
    ProductVarchar, StockItem, DEFAULT_STOCK_ID, GUEST_CUSTOMER_GROUP_ID,
};
