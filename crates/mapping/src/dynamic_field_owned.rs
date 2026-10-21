//! Runtime-named source property reads whose open object has an explicit owner.
//!
//! The object is selected from exactly one Primary or named endpoint and frame;
//! the computed key only names a property of that selected object. Another
//! endpoint or frame never supplies the object or a missing property.

use serde::{Deserialize, Serialize};

use crate::{NodeId, OwnedSourceRef};

/// The owned counterpart of [`crate::Node::DynamicSourceField`].
///
/// The body is strict: unknown or duplicate keys, a legacy path array in place
/// of the owned object, and malformed or empty named owners are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedDynamicSourceField {
    /// Endpoint-relative absolute path of the open object, including the empty
    /// path for an open endpoint root.
    pub object: OwnedSourceRef,
    /// Exact endpoint-local frame identity. `None` selects the latest
    /// same-owner frame prefixing the object path; an explicit `[]` is the
    /// endpoint-root pin. When present it must prefix `object.path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Vec<String>>,
    /// Parent-context property name, evaluated before any source access.
    pub key: NodeId,
}
