mod files;
mod ir;
mod model;
mod parser;
mod resolve;

pub use files::{
    MAX_IMPORT_DEPTH, MAX_IMPORT_PATH_BYTES, MAX_SCHEMA_FILES, MAX_SCHEMA_GRAPH_BYTES,
    SchemaBundle, SchemaFile, canonical_schema_path,
};
pub(crate) use ir::project;
pub use model::{
    Cardinality, DefaultValue, Enum, EnumId, EnumValue, Field, FieldType, Layout, Message,
    MessageId, Oneof, OneofId, ScalarType,
};
