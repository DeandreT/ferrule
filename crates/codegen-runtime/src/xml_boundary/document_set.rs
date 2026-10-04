//! Member ownership for mapping-produced XML documents; paths remain host metadata.

use std::{error::Error, fmt};

use super::{
    XmlBoundaryError, XmlBoundaryErrorKind, XmlOutputSetBudget, XmlOutputSetError, XmlOutputTarget,
};

/// Index is zero-based in the final ordered document list, not in the raw drivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDocumentOutputOwner {
    pub target: XmlOutputTarget,
    pub index: usize,
    pub path: String,
}

/// Retains the original boundary and its standard typed source chain.
#[derive(Debug)]
pub struct XmlDocumentExecutionError {
    pub member: Option<XmlDocumentOutputOwner>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlDocumentExecutionError {
    /// Descriptor Schema setup is global; other serializer failures own the member.
    pub fn serialization(index: usize, path: &str, boundary: XmlBoundaryError) -> Self {
        let member =
            (boundary.kind != XmlBoundaryErrorKind::Schema).then(|| XmlDocumentOutputOwner {
                target: XmlOutputTarget::Primary,
                index,
                path: path.to_owned(),
            });
        Self {
            member,
            boundary: Box::new(boundary),
        }
    }

    /// A shape mismatch cannot identify a member that was never serialized.
    pub fn alignment(detail: impl Into<String>) -> Self {
        Self::from(XmlOutputSetError::alignment(detail).boundary)
    }
}

impl From<XmlBoundaryError> for XmlDocumentExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            member: None,
            boundary: Box::new(boundary),
        }
    }
}

impl fmt::Display for XmlDocumentExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(member) = &self.member {
            write!(
                f,
                "XML output {:?} member {} `{}`: ",
                member.target, member.index, member.path
            )?;
        }
        self.boundary.fmt(f)
    }
}

impl Error for XmlDocumentExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

/// Counts actual mapped members and serialized documents, excluding logical paths.
/// Mapping instances may already be live; this is not a streaming or RSS bound.
pub struct XmlDocumentSetBudget {
    inner: XmlOutputSetBudget,
}

impl XmlDocumentSetBudget {
    pub fn new(actual_count: usize) -> Result<Self, XmlDocumentExecutionError> {
        XmlOutputSetBudget::new(actual_count)
            .map(|inner| Self { inner })
            .map_err(|error| XmlDocumentExecutionError::from(error.boundary))
    }

    pub fn charge(
        &mut self,
        index: usize,
        path: &str,
        bytes: usize,
    ) -> Result<(), XmlDocumentExecutionError> {
        self.inner
            .charge(XmlOutputTarget::Primary, bytes)
            .map_err(|error| XmlDocumentExecutionError::serialization(index, path, error.boundary))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MAX_XML_DOCUMENT_BYTES, XmlOutputSetResourceError};
    use super::*;
    use crate::RuntimeError;

    #[test]
    fn member_identity_and_original_boundary_source_are_retained() {
        let boundary = XmlBoundaryError::with_source(
            XmlBoundaryErrorKind::Output,
            std::io::Error::other("original writer"),
        );
        let cause = boundary.source().unwrap() as *const dyn Error;
        let error = XmlDocumentExecutionError::serialization(1, "same.xml", boundary);
        let owner = error.member.as_ref().unwrap();
        assert_eq!(
            (owner.target, owner.index, owner.path.as_str()),
            (XmlOutputTarget::Primary, 1, "same.xml")
        );
        let source = error
            .source()
            .unwrap()
            .downcast_ref::<XmlBoundaryError>()
            .unwrap();
        assert!(std::ptr::eq(source, error.boundary.as_ref()));
        assert!(std::ptr::eq(cause, source.source().unwrap()));
    }

    #[test]
    fn descriptor_mapping_and_alignment_failures_never_invent_a_member() {
        let schema = XmlDocumentExecutionError::serialization(
            4,
            "would-be.xml",
            XmlBoundaryError::detail(XmlBoundaryErrorKind::Schema, "descriptor setup"),
        );
        assert!(schema.member.is_none());
        let mapping = XmlDocumentExecutionError::from(XmlBoundaryError::from(
            RuntimeError::EmptyDynamicTargetPath { node: 7 },
        ));
        assert!(mapping.member.is_none());
        assert!(matches!(
            mapping
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>(),
            Some(RuntimeError::EmptyDynamicTargetPath { node: 7 })
        ));
        let alignment = XmlDocumentExecutionError::alignment("not a document set");
        assert!(alignment.member.is_none());
        assert_eq!(alignment.boundary.kind, XmlBoundaryErrorKind::Output);
        assert!(alignment.boundary.source().is_some());
    }

    #[test]
    fn count_uses_actual_members_including_empty_and_has_no_guessed_owner() {
        assert!(XmlDocumentSetBudget::new(0).is_ok());
        assert!(XmlDocumentSetBudget::new(4096).is_ok());
        let error = XmlDocumentSetBudget::new(4097).err().unwrap();
        assert!(error.member.is_none());
        assert_eq!(
            (
                error.boundary.kind,
                error.boundary.bytes,
                error.boundary.limit
            ),
            (XmlBoundaryErrorKind::Output, None, None)
        );
        let cause = error
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
    fn exact_combined_bytes_and_crossing_owner_ignore_opaque_duplicate_paths() {
        let mut budget = XmlDocumentSetBudget::new(5).unwrap();
        for (index, path) in ["same.xml", "same.xml", "/opaque.xml", "../opaque.xml"]
            .into_iter()
            .enumerate()
        {
            budget.charge(index, path, MAX_XML_DOCUMENT_BYTES).unwrap();
        }
        let error = budget.charge(4, "../opaque.xml", 1).unwrap_err();
        let owner = error.member.as_ref().unwrap();
        assert_eq!((owner.index, owner.path.as_str()), (4, "../opaque.xml"));
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
                256 * 1024 * 1024 + 1,
                256 * 1024 * 1024
            )
        );
    }

    #[test]
    fn per_document_limit_keeps_original_byte_fields_and_member() {
        let boundary = XmlBoundaryError::document_limit(MAX_XML_DOCUMENT_BYTES + 1);
        let error = XmlDocumentExecutionError::serialization(2, "last.xml", boundary);
        assert_eq!(error.member.as_ref().unwrap().index, 2);
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
    }
}
