use serde::{Deserialize, Serialize};

/// Portable Protocol Buffers target metadata.
///
/// The schema source is embedded when a design is imported so execution does
/// not depend on the original `.proto` file remaining beside the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtobufOptions {
    pub schema: String,
    pub root_message: String,
}
