//! Internal JSON instance-constraint evaluation shared by native and generated
//! boundaries. Schema import/export owns syntax and composition, not execution.
//!
//! Only the exact uniqueness helpers are public for `codegen-runtime`. The
//! remaining validators consume the effective IR and preserve the caller's
//! validation order and per-document pattern budget.

mod arrays;
pub(crate) mod constants;
mod objects;
mod patterns;
mod predicate;
mod scalars;
pub(crate) mod unique_items;

pub(crate) use arrays::{validate_contains, validate_item_count};
pub(crate) use constants::validate_json as validate_constant;
pub(crate) use objects::{
    validate_dependent_schemas, validate_property_count, validate_property_dependencies,
};
pub(crate) use patterns::{PatternPrograms, PatternRuntime};
pub(crate) use predicate::validate_private_unique_items;
pub(crate) use scalars::{
    validate_allowed_value, validate_multiple_of, validate_numeric_range, validate_string_length,
};
pub use unique_items::{
    UniqueItemsValidationError, validate_raw_json_unique_items, validate_unique_json_items,
};
