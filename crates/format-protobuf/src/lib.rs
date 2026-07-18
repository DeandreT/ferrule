//! Bounded proto2/proto3 schema and instance I/O.
//!
//! [`Layout`] resolves every field type and validates tag/cardinality rules
//! before it is exposed. Runtime output can therefore operate on typed field
//! descriptors instead of interpreting `.proto` source while encoding.

use std::io::Read;
use std::path::Path;

use ir::Instance;
use thiserror::Error;

mod decode;
mod encode;
mod ir_schema;
mod schema;

/// Maximum accepted `.proto` schema size.
pub const MAX_SCHEMA_BYTES: usize = 1024 * 1024;

/// Maximum accepted encoded message size.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

pub use schema::{
    Cardinality, DefaultValue, Enum, EnumId, EnumValue, Field, FieldType, Layout, Message,
    MessageId, ScalarType,
};

/// Errors from schema parsing, layout validation, or instance encoding.
#[derive(Debug, Error)]
pub enum ProtobufError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("proto schema at line {line}, column {column}: {message}")]
    Parse {
        line: usize,
        column: usize,
        message: String,
    },
    #[error("invalid proto schema: {0}")]
    InvalidSchema(String),
    #[error("unknown protobuf root message `{0}`")]
    UnknownRoot(String),
    #[error("protobuf root message `{0}` is ambiguous; use its fully-qualified name")]
    AmbiguousRoot(String),
    #[error("protobuf value at `{path}`: {message}")]
    InvalidInstance { path: String, message: String },
}

impl ProtobufError {
    pub(crate) fn parse(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self::Parse {
            line,
            column,
            message: message.into(),
        }
    }

    pub(crate) fn schema(message: impl Into<String>) -> Self {
        Self::InvalidSchema(message.into())
    }

    pub(crate) fn instance(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidInstance {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Parses a proto2/proto3-lite schema from disk.
pub fn read_layout(path: &Path) -> Result<Layout, ProtobufError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err(ProtobufError::schema(format!(
            "schema exceeds the {MAX_SCHEMA_BYTES}-byte limit"
        )));
    }
    let source =
        String::from_utf8(bytes).map_err(|_| ProtobufError::schema("schema is not valid UTF-8"))?;
    Layout::parse(&source)
}

/// Encodes one root message into a new byte vector.
pub fn to_vec(
    layout: &Layout,
    root: impl AsRef<str>,
    instance: &Instance,
) -> Result<Vec<u8>, ProtobufError> {
    let message = layout.resolve_message(root.as_ref())?;
    encode::encode(layout, message, instance)
}

/// Encodes one already-resolved root message into a new byte vector.
pub fn to_vec_message(
    layout: &Layout,
    root: MessageId,
    instance: &Instance,
) -> Result<Vec<u8>, ProtobufError> {
    if layout.message(root).is_none() {
        return Err(ProtobufError::schema(format!(
            "message id {} does not belong to this layout",
            root.index()
        )));
    }
    encode::encode(layout, root, instance)
}

/// Decodes one root message from a bounded byte slice.
pub fn from_slice(
    layout: &Layout,
    root: impl AsRef<str>,
    bytes: &[u8],
) -> Result<Instance, ProtobufError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtobufError::instance(
            root.as_ref(),
            format!("message exceeds the {MAX_MESSAGE_BYTES}-byte limit"),
        ));
    }
    let message = layout.resolve_message(root.as_ref())?;
    decode::decode(layout, message, bytes)
}

/// Decodes one already-resolved root message from a bounded byte slice.
pub fn from_slice_message(
    layout: &Layout,
    root: MessageId,
    bytes: &[u8],
) -> Result<Instance, ProtobufError> {
    if layout.message(root).is_none() {
        return Err(ProtobufError::schema(format!(
            "message id {} does not belong to this layout",
            root.index()
        )));
    }
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtobufError::instance(
            root.index().to_string(),
            format!("message exceeds the {MAX_MESSAGE_BYTES}-byte limit"),
        ));
    }
    decode::decode(layout, root, bytes)
}

/// Projects a resolved protobuf root into ferrule's tree-shaped schema IR.
///
/// Proto message recursion cannot be represented by [`ir::SchemaNode`] and
/// is rejected explicitly. Enum fields project as integer scalars; bytes
/// project as strings because [`ir::Value`] has no binary variant.
pub fn to_ir_schema(
    layout: &Layout,
    root: impl AsRef<str>,
) -> Result<ir::SchemaNode, ProtobufError> {
    let message = layout.resolve_message(root.as_ref())?;
    ir_schema::project(layout, message)
}

/// Encodes one root message and writes it atomically with respect to schema
/// and instance validation: the destination is not touched unless encoding
/// succeeds.
pub fn write(
    path: &Path,
    layout: &Layout,
    root: impl AsRef<str>,
    instance: &Instance,
) -> Result<(), ProtobufError> {
    let bytes = to_vec(layout, root, instance)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Reads and decodes one root message from disk without accepting an
/// unbounded allocation.
pub fn read(
    path: &Path,
    layout: &Layout,
    root: impl AsRef<str>,
) -> Result<Instance, ProtobufError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take((MAX_MESSAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    from_slice(layout, root, &bytes)
}

#[cfg(test)]
mod tests;
