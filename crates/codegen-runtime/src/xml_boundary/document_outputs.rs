//! One static primary document and ordered members of a named target.
//! Logical paths remain opaque metadata owned by the caller.

use std::{error::Error, fmt};

use super::{
    XmlBoundaryError, XmlBoundaryErrorKind, XmlOutputSetBudget, XmlOutputSetError, XmlOutputTarget,
};

/// A static primary has no member index or logical path. Named indices are zero-based
/// in the final ordered list, independently of the original target declaration index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlDocumentOutputsOwner {
    Primary,
    NamedMember {
        declaration_index: usize,
        name: String,
        index: usize,
        path: String,
    },
}

/// Retains the original boundary and its standard typed source chain.
/// Input, mapping, schema setup, alignment and total artifact count are unowned.
#[derive(Debug)]
pub struct XmlDocumentOutputsExecutionError {
    pub owner: Option<XmlDocumentOutputsOwner>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlDocumentOutputsExecutionError {
    /// Descriptor Schema setup is global; other primary serializer failures own Primary.
    pub fn primary_serialization(boundary: XmlBoundaryError) -> Self {
        let owner = (boundary.kind != XmlBoundaryErrorKind::Schema)
            .then_some(XmlDocumentOutputsOwner::Primary);
        Self {
            owner,
            boundary: Box::new(boundary),
        }
    }

    /// Names and opaque paths are copied without normalization or filesystem access.
    /// Descriptor Schema setup is global; other failures own the exact named member.
    pub fn named_serialization(
        declaration_index: usize,
        name: &str,
        index: usize,
        path: &str,
        boundary: XmlBoundaryError,
    ) -> Self {
        let owner = (boundary.kind != XmlBoundaryErrorKind::Schema).then(|| {
            XmlDocumentOutputsOwner::NamedMember {
                declaration_index,
                name: name.to_owned(),
                index,
                path: path.to_owned(),
            }
        });
        Self {
            owner,
            boundary: Box::new(boundary),
        }
    }

    /// Shape/count arithmetic cannot identify a document that was never serialized.
    pub fn alignment(detail: impl Into<String>) -> Self {
        Self::from(XmlOutputSetError::alignment(detail).boundary)
    }
}

impl From<XmlBoundaryError> for XmlDocumentOutputsExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            owner: None,
            boundary: Box::new(boundary),
        }
    }
}

impl fmt::Display for XmlDocumentOutputsExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            Some(XmlDocumentOutputsOwner::Primary) => write!(f, "primary XML output: ")?,
            Some(XmlDocumentOutputsOwner::NamedMember {
                declaration_index,
                name,
                index,
                path,
            }) => {
                write!(
                    f,
                    "named XML output {declaration_index} `{name}` member {index} `{path}`: "
                )?;
            }
            None => {}
        }
        self.boundary.fmt(f)
    }
}

impl Error for XmlDocumentOutputsExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

/// One complete mapped execution: count includes the static primary and all named
/// members. The caller checks `1 + members.len()` before capacity/serialization,
/// serializes and charges Primary first, then each member in final order, and aborts
/// after the first failure. This is a serialized-byte bound, not a streaming/RSS bound.
/// Per-document limits are enforced by the serializer before charging this counter.
pub struct XmlDocumentOutputsBudget {
    inner: XmlOutputSetBudget,
}

impl XmlDocumentOutputsBudget {
    pub fn new(actual_artifact_count: usize) -> Result<Self, XmlDocumentOutputsExecutionError> {
        if actual_artifact_count == 0 {
            return Err(XmlDocumentOutputsExecutionError::alignment(
                "mixed XML outputs require the static primary artifact",
            ));
        }
        XmlOutputSetBudget::new(actual_artifact_count)
            .map(|inner| Self { inner })
            .map_err(|error| XmlDocumentOutputsExecutionError::from(error.boundary))
    }

    /// Charge the primary UTF-8 document before retaining or converting it.
    pub fn charge_primary(&mut self, bytes: usize) -> Result<(), XmlDocumentOutputsExecutionError> {
        self.inner
            .charge(XmlOutputTarget::Primary, bytes)
            .map_err(|error| {
                XmlDocumentOutputsExecutionError::primary_serialization(error.boundary)
            })
    }

    /// Generated declaration names have static lifetime, matching XmlOutputTarget.
    /// Charge the named member before retaining or converting it.
    pub fn charge_named(
        &mut self,
        declaration_index: usize,
        name: &'static str,
        index: usize,
        path: &str,
        bytes: usize,
    ) -> Result<(), XmlDocumentOutputsExecutionError> {
        self.inner
            .charge(
                XmlOutputTarget::Named {
                    index: declaration_index,
                    name,
                },
                bytes,
            )
            .map_err(|error| {
                XmlDocumentOutputsExecutionError::named_serialization(
                    declaration_index,
                    name,
                    index,
                    path,
                    error.boundary,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        MAX_XML_DOCUMENT_BYTES, MAX_XML_OUTPUT_SET_BYTES, XmlOutputSetResourceError,
    };
    use super::*;
    use crate::RuntimeError;

    #[test]
    fn static_primary_and_named_final_member_are_distinct_and_preserve_typed_cause_identity() {
        for owner in [
            XmlDocumentOutputsOwner::Primary,
            XmlDocumentOutputsOwner::NamedMember {
                declaration_index: 3,
                name: "audit".into(),
                index: 1,
                path: "../same.xml".into(),
            },
        ] {
            let boundary = XmlBoundaryError::with_source(
                XmlBoundaryErrorKind::Output,
                std::io::Error::other("original writer"),
            );
            let original_cause = boundary.source().unwrap() as *const dyn Error;
            let error = match &owner {
                XmlDocumentOutputsOwner::Primary => {
                    XmlDocumentOutputsExecutionError::primary_serialization(boundary)
                }
                XmlDocumentOutputsOwner::NamedMember {
                    declaration_index,
                    name,
                    index,
                    path,
                } => XmlDocumentOutputsExecutionError::named_serialization(
                    *declaration_index,
                    name,
                    *index,
                    path,
                    boundary,
                ),
            };
            assert_eq!(error.owner, Some(owner));
            let original_boundary = error
                .source()
                .unwrap()
                .downcast_ref::<XmlBoundaryError>()
                .unwrap();
            assert!(std::ptr::eq(original_boundary, error.boundary.as_ref()));
            assert!(std::ptr::eq(
                original_cause,
                original_boundary.source().unwrap()
            ));
        }
    }

    #[test]
    fn global_schema_mapping_input_and_alignment_never_invent_a_primary_or_member() {
        let schema_primary = XmlDocumentOutputsExecutionError::primary_serialization(
            XmlBoundaryError::detail(XmlBoundaryErrorKind::Schema, "descriptor"),
        );
        let schema_named = XmlDocumentOutputsExecutionError::named_serialization(
            0,
            "audit",
            1,
            "/opaque.xml",
            XmlBoundaryError::detail(XmlBoundaryErrorKind::Schema, "descriptor"),
        );
        assert!(schema_primary.owner.is_none());
        assert!(schema_named.owner.is_none());
        let mapping = XmlDocumentOutputsExecutionError::from(XmlBoundaryError::from(
            RuntimeError::EmptyDynamicTargetPath { node: 7 },
        ));
        assert!(mapping.owner.is_none());
        assert!(matches!(
            mapping
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>(),
            Some(RuntimeError::EmptyDynamicTargetPath { node: 7 })
        ));
        let input = XmlDocumentOutputsExecutionError::from(XmlBoundaryError::detail(
            XmlBoundaryErrorKind::Input,
            "parse",
        ));
        assert!(input.owner.is_none());
        let alignment = XmlDocumentOutputsExecutionError::alignment("artifact count overflow");
        assert!(alignment.owner.is_none());
        assert_eq!(alignment.boundary.kind, XmlBoundaryErrorKind::Output);
        assert!(alignment.boundary.source().is_some());
    }

    #[test]
    fn actual_artifacts_include_primary_and_count_refusal_is_global() {
        let zero = XmlDocumentOutputsBudget::new(0).err().unwrap();
        assert!(zero.owner.is_none());
        assert_eq!(zero.boundary.kind, XmlBoundaryErrorKind::Output);
        assert!(zero.boundary.source().is_some());
        assert!(XmlDocumentOutputsBudget::new(1).is_ok());
        assert!(XmlDocumentOutputsBudget::new(1 + 4095).is_ok());
        let count = XmlDocumentOutputsBudget::new(1 + 4096).err().unwrap();
        assert!(count.owner.is_none());
        assert_eq!((count.boundary.bytes, count.boundary.limit), (None, None));
        let cause = count
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlOutputSetResourceError>()
            .unwrap();
        assert_eq!(
            (cause.resource, cause.observed_count, cause.limit),
            ("xml_output_artifact_count", 4097, 4096)
        );
    }

    #[test]
    fn shared_utf8_budget_charges_primary_before_named_and_owns_crossing_final_member() {
        let mut budget = XmlDocumentOutputsBudget::new(5).unwrap();
        budget.charge_primary(MAX_XML_DOCUMENT_BYTES).unwrap();
        for (index, path) in ["same.xml", "same.xml", "/opaque.xml"]
            .into_iter()
            .enumerate()
        {
            budget
                .charge_named(0, "audit", index, path, MAX_XML_DOCUMENT_BYTES)
                .unwrap();
        }
        let error = budget
            .charge_named(0, "audit", 3, "../opaque.xml", 1)
            .unwrap_err();
        assert_eq!(
            error.owner,
            Some(XmlDocumentOutputsOwner::NamedMember {
                declaration_index: 0,
                name: "audit".into(),
                index: 3,
                path: "../opaque.xml".into(),
            })
        );
        assert_eq!((error.boundary.bytes, error.boundary.limit), (None, None));
        let cause = error
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlOutputSetResourceError>()
            .unwrap();
        assert_eq!(
            (cause.resource, cause.observed_count, cause.limit),
            (
                "xml_output_set_utf8_bytes",
                MAX_XML_OUTPUT_SET_BYTES + 1,
                MAX_XML_OUTPUT_SET_BYTES
            )
        );
    }

    #[test]
    fn primary_and_named_document_limits_retain_original_fields_and_owners() {
        let primary = XmlDocumentOutputsExecutionError::primary_serialization(
            XmlBoundaryError::document_limit(MAX_XML_DOCUMENT_BYTES + 1),
        );
        let named = XmlDocumentOutputsExecutionError::named_serialization(
            0,
            "audit",
            1,
            "same.xml",
            XmlBoundaryError::document_limit(MAX_XML_DOCUMENT_BYTES + 1),
        );
        assert_eq!(primary.owner, Some(XmlDocumentOutputsOwner::Primary));
        assert_eq!(
            named.owner,
            Some(XmlDocumentOutputsOwner::NamedMember {
                declaration_index: 0,
                name: "audit".into(),
                index: 1,
                path: "same.xml".into(),
            })
        );
        for error in [primary, named] {
            assert_eq!(
                (
                    error.boundary.kind,
                    error.boundary.bytes,
                    error.boundary.limit
                ),
                (
                    XmlBoundaryErrorKind::DocumentLimit,
                    Some(MAX_XML_DOCUMENT_BYTES + 1),
                    Some(MAX_XML_DOCUMENT_BYTES)
                )
            );
            assert!(error.boundary.source().is_none());
        }
    }
}
