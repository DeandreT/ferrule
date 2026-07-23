use serde::{Deserialize, Serialize};

/// Portable Protocol Buffers boundary metadata.
///
/// The schema source is embedded when a design is imported so execution does
/// not depend on the original `.proto` file remaining beside the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtobufOptions {
    pub schema: String,
    pub root_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<ProtobufSchemaFile>,
}

/// One canonical root-relative source file embedded beside a protobuf root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtobufSchemaFile {
    pub path: String,
    pub source: String,
}
