mod ir;
mod model;
mod parser;
mod resolve;

pub(crate) use ir::project;
pub use model::{
    Cardinality, DefaultValue, Enum, EnumId, EnumValue, Field, FieldType, Layout, Message,
    MessageId, Oneof, OneofId, ScalarType,
};
