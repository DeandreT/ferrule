//! File-capability gate for JSON Schema tuple frames.

use ir::SchemaNode;

use crate::Project;

/// Required by files whose schemas contain a tuple frame. Older readers
/// ignore unknown schema members, so without this capability they would
/// drop the frame's layout and read the tuple as an object.
pub const JSON_TUPLE_ARRAYS_V1_CAPABILITY: &str = "json_tuple_arrays_v1";

impl Project {
    /// Whether any schema stored in this project contains a tuple frame.
    pub fn uses_json_tuple_arrays(&self) -> bool {
        self.any_stored_schema(SchemaNode::contains_json_tuple)
    }
}

#[cfg(test)]
#[path = "json_tuple_arrays_tests.rs"]
mod tests;
