//! Instance trees and validated document-set members.

use serde::{Deserialize, Serialize};

use crate::{InstanceGroup, Value};

/// An actual value tree, shaped by some [`crate::SchemaNode`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Instance {
    Scalar(Value),
    Group(InstanceGroup),
    Repeated(Vec<Instance>),
    /// Ordered documents. Each member retains a portable path and may also
    /// retain its resolved source location while its value remains an ordinary
    /// schema-shaped tree. Host-specific path validation belongs to the I/O
    /// boundary.
    DocumentSet(Vec<DocumentMember>),
    /// Mapping-produced XML element occurrences whose cardinality is
    /// independent of the schema node's declared repetition.
    MappedSequence(Vec<Instance>),
}

/// One structurally valid member of an [`Instance::DocumentSet`].
///
/// The portable path is non-empty but otherwise opaque here; filesystem
/// boundaries validate and confine it for their host before performing I/O.
/// A source member may additionally retain the non-empty resolved location
/// used by current-document-path expressions. Output boundaries continue to
/// consume only the portable path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentMember {
    path: String,
    #[serde(skip)]
    resolved_source_path: Option<String>,
    value: Box<Instance>,
}

impl DocumentMember {
    pub fn new(path: impl Into<String>, value: Instance) -> Option<Self> {
        Self::new_with_source_path(path, None, value)
    }

    pub fn new_source(
        path: impl Into<String>,
        source_path: impl Into<String>,
        value: Instance,
    ) -> Option<Self> {
        Self::new_with_source_path(path, Some(source_path.into()), value)
    }

    fn new_with_source_path(
        path: impl Into<String>,
        resolved_source_path: Option<String>,
        value: Instance,
    ) -> Option<Self> {
        let path = path.into();
        (!path.is_empty()
            && resolved_source_path
                .as_ref()
                .is_none_or(|path| !path.is_empty())
            && !matches!(value, Instance::DocumentSet(_)))
        .then(|| Self {
            path,
            resolved_source_path,
            value: Box::new(value),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn source_path(&self) -> &str {
        self.resolved_source_path.as_deref().unwrap_or(&self.path)
    }

    pub fn value(&self) -> &Instance {
        &self.value
    }
}

impl<'de> Deserialize<'de> for DocumentMember {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            path: String,
            value: Instance,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.path, wire.value).ok_or_else(|| {
            serde::de::Error::custom(
                "document-set members require non-empty paths and a non-document-set value",
            )
        })
    }
}

impl Instance {
    pub fn field(&self, name: &str) -> Option<&Instance> {
        match self {
            Instance::Group(fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            Instance::DocumentSet(documents) => documents.first()?.value().field(name),
            _ => None,
        }
    }

    pub fn as_scalar(&self) -> Option<&Value> {
        match self {
            Instance::Scalar(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_repeated(&self) -> Option<&[Instance]> {
        match self {
            Instance::Repeated(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_mapped_sequence(&self) -> Option<&[Instance]> {
        match self {
            Instance::MappedSequence(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_document_set(&self) -> Option<&[DocumentMember]> {
        match self {
            Instance::DocumentSet(documents) => Some(documents),
            _ => None,
        }
    }
}
