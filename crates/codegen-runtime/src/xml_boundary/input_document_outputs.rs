//! Static named XML inputs and mixed primary/named document outputs retain exclusive phase ownership.
use super::{
    XmlBoundaryError, XmlDocumentOutputsExecutionError, XmlDocumentOutputsOwner, XmlInputSource,
};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmlInputDocumentOutputsOwner {
    Input(XmlInputSource),
    Output(XmlDocumentOutputsOwner),
}

#[derive(Debug)]
#[non_exhaustive]
/// The original boundary allocation and its complete standard typed cause chain.
/// Input descriptor failures retain Input; output Schema setup and global
/// names/count/mapping/alignment failures have no invented output owner.
pub struct XmlInputDocumentOutputsExecutionError {
    pub owner: Option<XmlInputDocumentOutputsOwner>,
    pub boundary: Box<XmlBoundaryError>,
}

impl XmlInputDocumentOutputsExecutionError {
    /// Only the trusted static collector forwards its original phase and Box.
    /// This is not a general conversion of arbitrary XmlExecutionError fields.
    pub fn from_input_boundary(
        input: Option<XmlInputSource>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        Self {
            owner: input.map(XmlInputDocumentOutputsOwner::Input),
            boundary,
        }
    }
}
impl From<XmlBoundaryError> for XmlInputDocumentOutputsExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            owner: None,
            boundary: Box::new(boundary),
        }
    }
}
impl From<XmlDocumentOutputsExecutionError> for XmlInputDocumentOutputsExecutionError {
    fn from(error: XmlDocumentOutputsExecutionError) -> Self {
        Self {
            owner: error.owner.map(XmlInputDocumentOutputsOwner::Output),
            boundary: error.boundary,
        }
    }
}
impl fmt::Display for XmlInputDocumentOutputsExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            Some(XmlInputDocumentOutputsOwner::Input(input)) => write!(f, "XML input {input:?}: ")?,
            Some(XmlInputDocumentOutputsOwner::Output(output)) => {
                write!(f, "XML output {output:?}: ")?
            }
            None => {}
        }
        self.boundary.fmt(f)
    }
}
impl Error for XmlInputDocumentOutputsExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        MAX_XML_DOCUMENT_BYTES, MAX_XML_INPUT_SET_BYTES, MAX_XML_OUTPUT_SET_BYTES,
        XmlBoundaryErrorKind, XmlDocumentOutputsBudget, XmlInputSetBudget,
        XmlInputSetResourceError, XmlOutputSetResourceError, parse_structured_xml,
        parse_structured_xml_bytes, xml_input_indices,
    };
    use super::*;
    use crate::RuntimeError;

    #[test]
    fn named_utf8_error_moves_the_original_boundary_and_typed_decoder_cause() {
        let boundary = Box::new(parse_structured_xml_bytes("{}", &[0xff]).unwrap_err());
        let allocation = boundary.as_ref() as *const XmlBoundaryError;
        let cause = boundary.source().unwrap() as *const dyn Error;
        assert!(boundary.source().unwrap().is::<std::str::Utf8Error>());
        let error = XmlInputDocumentOutputsExecutionError::from_input_boundary(
            Some(XmlInputSource::Named {
                index: 2,
                name: "unused-padding",
            }),
            boundary,
        );
        assert_eq!(
            error.owner,
            Some(XmlInputDocumentOutputsOwner::Input(XmlInputSource::Named {
                index: 2,
                name: "unused-padding"
            }))
        );
        assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        assert!(std::ptr::eq(
            error
                .source()
                .unwrap()
                .downcast_ref::<XmlBoundaryError>()
                .unwrap(),
            allocation
        ));
        assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
    }

    #[test]
    fn input_schema_has_a_source_but_output_schema_stays_global() {
        let input = XmlInputDocumentOutputsExecutionError::from_input_boundary(
            Some(XmlInputSource::Primary),
            Box::new(parse_structured_xml("{", "<Input/>").unwrap_err()),
        );
        assert_eq!(input.boundary.kind, XmlBoundaryErrorKind::Schema);
        assert_eq!(
            input.owner,
            Some(XmlInputDocumentOutputsOwner::Input(XmlInputSource::Primary))
        );
        for original in [
            XmlDocumentOutputsExecutionError::primary_serialization(
                parse_structured_xml("{", "<Input/>").unwrap_err(),
            ),
            XmlDocumentOutputsExecutionError::named_serialization(
                1,
                "a-beta",
                3,
                "same.xml",
                parse_structured_xml("{", "<Input/>").unwrap_err(),
            ),
        ] {
            let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
            let cause = original.boundary.source().unwrap() as *const dyn Error;
            let error = XmlInputDocumentOutputsExecutionError::from(original);
            assert!(error.owner.is_none());
            assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Schema);
            assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
            assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
        }
    }

    #[test]
    fn primary_and_second_named_member_preserve_distinct_owners_and_writer_identity() {
        for owner in [
            XmlDocumentOutputsOwner::Primary,
            XmlDocumentOutputsOwner::NamedMember {
                declaration_index: 1,
                name: "a-beta".into(),
                index: 1,
                path: "../雪😀.xml".into(),
            },
        ] {
            let boundary = XmlBoundaryError::with_source(
                XmlBoundaryErrorKind::Output,
                std::io::Error::other("original writer"),
            );
            let cause = boundary.source().unwrap() as *const dyn Error;
            let original = match &owner {
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
            let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
            let error = XmlInputDocumentOutputsExecutionError::from(original);
            assert_eq!(
                error.owner,
                Some(XmlInputDocumentOutputsOwner::Output(owner))
            );
            assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
            assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
        }
        let original = XmlDocumentOutputsExecutionError::named_serialization(
            1,
            "a-beta",
            1,
            "/opaque.xml",
            XmlBoundaryError::document_limit(MAX_XML_DOCUMENT_BYTES + 1),
        );
        let error = XmlInputDocumentOutputsExecutionError::from(original);
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

    #[test]
    fn count_names_mapping_and_alignment_stay_global_with_original_causes() {
        let original = XmlInputSetBudget::new(4097).unwrap_err();
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let input = XmlInputDocumentOutputsExecutionError::from_input_boundary(
            original.input,
            original.boundary,
        );
        assert!(input.owner.is_none());
        assert!(std::ptr::eq(input.boundary.as_ref(), allocation));
        let cause = input
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlInputSetResourceError>()
            .unwrap();
        assert_eq!(
            (cause.resource, cause.observed_count, cause.limit),
            ("xml_input_artifact_count", 4097, 4096)
        );
        let names = xml_input_indices(&["z-rates", "a-count"], &["a-count"]).unwrap_err();
        let cause = names.boundary.source().unwrap() as *const dyn Error;
        let names =
            XmlInputDocumentOutputsExecutionError::from_input_boundary(names.input, names.boundary);
        assert!(names.owner.is_none());
        assert_eq!(names.boundary.kind, XmlBoundaryErrorKind::Mapping);
        assert!(std::ptr::eq(names.boundary.source().unwrap(), cause));
        let count = XmlInputDocumentOutputsExecutionError::from(
            XmlDocumentOutputsBudget::new(4097).err().unwrap(),
        );
        assert!(count.owner.is_none());
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
        let mapping = XmlInputDocumentOutputsExecutionError::from(XmlBoundaryError::from(
            RuntimeError::EmptyDynamicTargetPath { node: 3 },
        ));
        assert!(mapping.owner.is_none());
        assert!(matches!(
            mapping
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>(),
            Some(RuntimeError::EmptyDynamicTargetPath { node: 3 })
        ));
        let alignment = XmlInputDocumentOutputsExecutionError::from(
            XmlDocumentOutputsExecutionError::alignment("incomplete ordered envelopes"),
        );
        assert!(alignment.owner.is_none());
        assert_eq!(alignment.boundary.kind, XmlBoundaryErrorKind::Output);
        assert!(alignment.boundary.source().is_some());
    }

    #[test]
    fn independent_ledgers_keep_crossing_input_or_second_named_member_exact() {
        // Counter-only transitions; this does not claim actual large XML execution.
        let mut inputs = XmlInputSetBudget::new(5).unwrap();
        for source in [
            XmlInputSource::Primary,
            XmlInputSource::Named {
                index: 0,
                name: "z-rates",
            },
            XmlInputSource::Named {
                index: 1,
                name: "a-count",
            },
            XmlInputSource::Named {
                index: 2,
                name: "unused-padding",
            },
        ] {
            inputs.charge(source, MAX_XML_DOCUMENT_BYTES).unwrap();
        }
        let original = inputs
            .charge(
                XmlInputSource::Named {
                    index: 3,
                    name: "tail",
                },
                1,
            )
            .unwrap_err();
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let input = XmlInputDocumentOutputsExecutionError::from_input_boundary(
            original.input,
            original.boundary,
        );
        assert_eq!(
            input.owner,
            Some(XmlInputDocumentOutputsOwner::Input(XmlInputSource::Named {
                index: 3,
                name: "tail"
            }))
        );
        assert!(std::ptr::eq(input.boundary.as_ref(), allocation));
        let input_cause = input
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlInputSetResourceError>()
            .unwrap();
        assert_eq!(
            (
                input_cause.resource,
                input_cause.observed_count,
                input_cause.limit
            ),
            (
                "xml_input_set_utf8_bytes",
                MAX_XML_INPUT_SET_BYTES + 1,
                MAX_XML_INPUT_SET_BYTES
            )
        );
        let mut outputs = XmlDocumentOutputsBudget::new(5).unwrap();
        outputs.charge_primary(MAX_XML_DOCUMENT_BYTES).unwrap();
        outputs
            .charge_named(0, "z-alpha", 0, "same.xml", MAX_XML_DOCUMENT_BYTES)
            .unwrap();
        outputs
            .charge_named(1, "a-beta", 0, "same.xml", MAX_XML_DOCUMENT_BYTES)
            .unwrap();
        outputs
            .charge_named(1, "a-beta", 1, "/opaque.xml", MAX_XML_DOCUMENT_BYTES)
            .unwrap();
        let original = outputs
            .charge_named(1, "a-beta", 2, "../last.xml", 1)
            .unwrap_err();
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let output = XmlInputDocumentOutputsExecutionError::from(original);
        assert_eq!(
            output.owner,
            Some(XmlInputDocumentOutputsOwner::Output(
                XmlDocumentOutputsOwner::NamedMember {
                    declaration_index: 1,
                    name: "a-beta".into(),
                    index: 2,
                    path: "../last.xml".into(),
                }
            ))
        );
        assert!(std::ptr::eq(output.boundary.as_ref(), allocation));
        let cause = output
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
}
