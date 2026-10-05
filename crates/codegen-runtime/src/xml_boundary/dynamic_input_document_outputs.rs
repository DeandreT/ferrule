//! Dynamic XML inputs and mixed document outputs retain one authentic phase owner.
use super::{
    XmlBoundaryError, XmlDocumentOutputsExecutionError, XmlDocumentOutputsOwner,
    XmlDynamicInputRequest, XmlInputSource,
};
use std::{error::Error, fmt};

/// Input declaration identity is independent of the final output member identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmlDynamicInputDocumentOutputsOwner {
    Input(XmlInputSource),
    Output(XmlDocumentOutputsOwner),
}

/// Keeps the original boxed boundary, standard cause chain and adapter request.
#[derive(Debug)]
#[non_exhaustive]
pub struct XmlDynamicInputDocumentOutputsExecutionError {
    pub owner: Option<XmlDynamicInputDocumentOutputsOwner>,
    pub boundary: Box<XmlBoundaryError>,
    pub request: Option<Box<XmlDynamicInputRequest>>,
}
impl XmlDynamicInputDocumentOutputsExecutionError {
    /// Only trusted static admission feeds this channel. The original box moves
    /// unchanged; a descriptor Schema error retains its input source.
    /// This is not a conversion for arbitrary output or dynamic execution errors.
    pub fn from_input_boundary(
        input: Option<XmlInputSource>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        Self {
            owner: input.map(XmlDynamicInputDocumentOutputsOwner::Input),
            boundary,
            request: None,
        }
    }
    /// Use the exact request and boundary synchronously recovered from a fresh
    /// adapter's first product refusal, while the typed loader preserves its
    /// original live marker. This conversion does not authenticate arbitrary
    /// requests and does not permit retry or reuse after any failed load.
    /// Both original allocations move without cloning.
    pub fn from_dynamic_input_boundary(
        request: Box<XmlDynamicInputRequest>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        let input = XmlInputSource::Named {
            index: request.declaration_index,
            name: request.source,
        };
        Self {
            owner: Some(XmlDynamicInputDocumentOutputsOwner::Input(input)),
            boundary,
            request: Some(request),
        }
    }
}
impl From<XmlBoundaryError> for XmlDynamicInputDocumentOutputsExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            owner: None,
            boundary: Box::new(boundary),
            request: None,
        }
    }
}
impl From<XmlDocumentOutputsExecutionError> for XmlDynamicInputDocumentOutputsExecutionError {
    fn from(error: XmlDocumentOutputsExecutionError) -> Self {
        Self {
            owner: error.owner.map(XmlDynamicInputDocumentOutputsOwner::Output),
            boundary: error.boundary,
            request: None,
        }
    }
}
impl fmt::Display for XmlDynamicInputDocumentOutputsExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            Some(XmlDynamicInputDocumentOutputsOwner::Input(input)) => {
                write!(f, "XML input {input:?}: ")?
            }
            Some(XmlDynamicInputDocumentOutputsOwner::Output(output)) => {
                write!(f, "XML output {output:?}: ")?
            }
            None => {}
        }
        self.boundary.fmt(f)
    }
}
impl Error for XmlDynamicInputDocumentOutputsExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        XmlBoundaryErrorKind, XmlDocumentOutputsBudget, XmlDynamicSourceAdapter,
        XmlDynamicSourcePolicy, XmlInputSetBudget, parse_structured_xml_bytes,
    };
    use super::*;
    use crate::{DynamicSourceLoader, DynamicXmlSourceLoader, RuntimeError};

    #[test]
    fn static_input_schema_and_utf8_keep_original_box_and_standard_cause() {
        for (schema, bytes, kind) in [
            ("{}", &[0xff][..], XmlBoundaryErrorKind::Utf8),
            ("{", b"<Input/>".as_slice(), XmlBoundaryErrorKind::Schema),
        ] {
            let boundary = Box::new(parse_structured_xml_bytes(schema, bytes).unwrap_err());
            let allocation = boundary.as_ref() as *const XmlBoundaryError;
            let cause = boundary.source().unwrap() as *const dyn Error;
            let error = XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(
                Some(XmlInputSource::Named {
                    index: 2,
                    name: "unused",
                }),
                boundary,
            );
            assert_eq!(
                error.owner,
                Some(XmlDynamicInputDocumentOutputsOwner::Input(
                    XmlInputSource::Named {
                        index: 2,
                        name: "unused"
                    }
                ))
            );
            assert_eq!(error.boundary.kind, kind);
            assert!(error.request.is_none());
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
    }

    struct InvalidUtf8Loader;
    impl DynamicXmlSourceLoader for InvalidUtf8Loader {
        fn load(&self, _: &str, _: &str) -> Result<Vec<u8>, String> {
            Ok(vec![0xff])
        }
    }
    #[test]
    fn fresh_terminal_adapter_moves_exact_request_boundary_and_utf8_cause() {
        let adapter = XmlDynamicSourceAdapter::new(
            &InvalidUtf8Loader,
            XmlDynamicSourcePolicy {
                declaration_index: 1,
                source: "catalog",
                schema: "{}",
            },
            XmlInputSetBudget::new(3).unwrap(),
        );
        let marker = DynamicSourceLoader::load(&adapter, "catalog", "b.xml").unwrap_err();
        let original = adapter.recover(RuntimeError::DynamicSourceLoad {
            source: "catalog",
            path: "b.xml".into(),
            message: marker,
        });
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let cause = original.boundary.source().unwrap() as *const dyn Error;
        let request = original.request.unwrap();
        let request_allocation = request.as_ref() as *const XmlDynamicInputRequest;
        let error = XmlDynamicInputDocumentOutputsExecutionError::from_dynamic_input_boundary(
            request,
            original.boundary,
        );
        assert_eq!(
            error.owner,
            Some(XmlDynamicInputDocumentOutputsOwner::Input(
                XmlInputSource::Named {
                    index: 1,
                    name: "catalog"
                }
            ))
        );
        let actual = error.request.as_ref().unwrap();
        assert_eq!(
            (
                actual.ordinal,
                actual.callback_invoked,
                actual.path.as_str()
            ),
            (1, true, "b.xml")
        );
        assert!(std::ptr::eq(actual.as_ref(), request_allocation));
        assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
        assert!(error.boundary.source().unwrap().is::<std::str::Utf8Error>());
    }

    #[test]
    fn wrong_source_path_or_new_marker_allocation_cannot_invent_adapter_request() {
        for mismatch in 0..3 {
            let adapter = XmlDynamicSourceAdapter::new(
                &InvalidUtf8Loader,
                XmlDynamicSourcePolicy {
                    declaration_index: 1,
                    source: "catalog",
                    schema: "{}",
                },
                XmlInputSetBudget::new(3).unwrap(),
            );
            let original_marker =
                DynamicSourceLoader::load(&adapter, "catalog", "b.xml").unwrap_err();
            // The original allocation stays live even when the equal-text impostor is used.
            let marker = if mismatch == 2 {
                original_marker.clone()
            } else {
                original_marker
            };
            let source = if mismatch == 0 { "other" } else { "catalog" };
            let path = if mismatch == 1 { "other.xml" } else { "b.xml" };
            let original = adapter.recover(RuntimeError::DynamicSourceLoad {
                source,
                path: path.into(),
                message: marker,
            });
            assert!(original.input.is_none() && original.request.is_none());
            let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
            let error = XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(
                None,
                original.boundary,
            );
            assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
            assert!(error.owner.is_none() && error.request.is_none());
            assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Mapping);
            assert!(
                matches!(error.boundary.source().unwrap().downcast_ref::<RuntimeError>(),
                Some(RuntimeError::DynamicSourceLoad { source: actual_source, path: actual_path, .. })
                if *actual_source == source && actual_path == path)
            );
        }
    }

    struct HostFailure;
    impl DynamicXmlSourceLoader for HostFailure {
        fn load(&self, _: &str, _: &str) -> Result<Vec<u8>, String> {
            Err("generated XML input adapter refused a dynamic document".into())
        }
    }
    #[test]
    fn external_host_failure_stays_global_with_original_runtime_cause() {
        let adapter = XmlDynamicSourceAdapter::new(
            &HostFailure,
            XmlDynamicSourcePolicy {
                declaration_index: 1,
                source: "catalog",
                schema: "{}",
            },
            XmlInputSetBudget::new(3).unwrap(),
        );
        let marker = DynamicSourceLoader::load(&adapter, "catalog", "b.xml").unwrap_err();
        let original = adapter.recover(RuntimeError::DynamicSourceLoad {
            source: "catalog",
            path: "b.xml".into(),
            message: marker,
        });
        assert!(original.input.is_none() && original.request.is_none());
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let error = XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(
            None,
            original.boundary,
        );
        assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        assert!(error.owner.is_none() && error.request.is_none());
        assert!(
            matches!(error.boundary.source().unwrap().downcast_ref::<RuntimeError>(),
            Some(RuntimeError::DynamicSourceLoad { message, .. }) if message == "generated XML input adapter refused a dynamic document")
        );
    }

    #[test]
    fn original_primary_named_and_global_output_owners_never_gain_a_request() {
        for named in [false, true] {
            let boundary = XmlBoundaryError::with_source(
                XmlBoundaryErrorKind::Output,
                std::io::Error::other("original writer"),
            );
            let original = if named {
                XmlDocumentOutputsExecutionError::named_serialization(
                    1,
                    "beta",
                    1,
                    "../雪.xml",
                    boundary,
                )
            } else {
                XmlDocumentOutputsExecutionError::primary_serialization(boundary)
            };
            let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
            let cause = original.boundary.source().unwrap() as *const dyn Error;
            let owner = original.owner.clone().unwrap();
            let error = XmlDynamicInputDocumentOutputsExecutionError::from(original);
            assert_eq!(
                error.owner,
                Some(XmlDynamicInputDocumentOutputsOwner::Output(owner))
            );
            assert!(error.request.is_none());
            assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
            assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
        }
        for original in [
            XmlDocumentOutputsExecutionError::named_serialization(
                1,
                "beta",
                1,
                "same.xml",
                parse_structured_xml_bytes("{", b"<Input/>").unwrap_err(),
            ),
            XmlDocumentOutputsExecutionError::alignment("incomplete envelopes"),
            XmlDocumentOutputsBudget::new(4097).err().unwrap(),
        ] {
            let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
            let error = XmlDynamicInputDocumentOutputsExecutionError::from(original);
            assert!(error.owner.is_none() && error.request.is_none());
            assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        }
    }
}
