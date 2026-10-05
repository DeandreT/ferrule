//! Dynamic named XML input and final primary member errors retain exclusive ownership.
use super::{
    XmlBoundaryError, XmlDocumentExecutionError, XmlDocumentOutputOwner, XmlDynamicInputRequest,
    XmlInputSource,
};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmlDynamicInputDocumentOwner {
    Input(XmlInputSource),
    Member(XmlDocumentOutputOwner),
}

#[derive(Debug)]
#[non_exhaustive]
pub struct XmlDynamicInputDocumentExecutionError {
    pub owner: Option<XmlDynamicInputDocumentOwner>,
    pub boundary: Box<XmlBoundaryError>,
    pub request: Option<Box<XmlDynamicInputRequest>>,
}

impl XmlDynamicInputDocumentExecutionError {
    /// The static collector forwards its original input and boxed boundary only.
    pub fn from_input_boundary(
        input: Option<XmlInputSource>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        Self {
            owner: input.map(XmlDynamicInputDocumentOwner::Input),
            boundary,
            request: None,
        }
    }
    /// Derives Input from the exact adapter-origin request after synchronous recovery.
    /// Moves the original request and boundary allocations without cloning.
    /// Supply only the exact request and boundary returned by this fresh adapter
    /// after its first product refusal. This conversion does not authenticate
    /// an arbitrary request or permit adapter reuse after failure.
    pub fn from_dynamic_input_boundary(
        request: Box<XmlDynamicInputRequest>,
        boundary: Box<XmlBoundaryError>,
    ) -> Self {
        let input = XmlInputSource::Named {
            index: request.declaration_index,
            name: request.source,
        };
        Self {
            owner: Some(XmlDynamicInputDocumentOwner::Input(input)),
            boundary,
            request: Some(request),
        }
    }
}
impl From<XmlBoundaryError> for XmlDynamicInputDocumentExecutionError {
    fn from(boundary: XmlBoundaryError) -> Self {
        Self {
            owner: None,
            boundary: Box::new(boundary),
            request: None,
        }
    }
}
impl From<XmlDocumentExecutionError> for XmlDynamicInputDocumentExecutionError {
    fn from(error: XmlDocumentExecutionError) -> Self {
        Self {
            owner: error.member.map(XmlDynamicInputDocumentOwner::Member),
            boundary: error.boundary,
            request: None,
        }
    }
}
impl fmt::Display for XmlDynamicInputDocumentExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            Some(XmlDynamicInputDocumentOwner::Input(input)) => write!(f, "XML input {input:?}: ")?,
            Some(XmlDynamicInputDocumentOwner::Member(member)) => write!(
                f,
                "XML output {:?} member {} `{}`: ",
                member.target, member.index, member.path
            )?,
            None => {}
        }
        self.boundary.fmt(f)
    }
}
impl Error for XmlDynamicInputDocumentExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.boundary.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        XmlBoundaryErrorKind, XmlDocumentSetBudget, XmlDynamicSourceAdapter,
        XmlDynamicSourcePolicy, XmlInputSetBudget, XmlOutputTarget, parse_structured_xml,
        parse_structured_xml_bytes,
    };
    use super::*;
    use crate::{DynamicSourceLoader, DynamicXmlSourceLoader, RuntimeError};

    #[test]
    fn static_parser_schema_and_decoder_boundaries_retain_original_phase_allocation_and_cause() {
        for (schema, bytes, expected) in [
            ("{}", &[0xff][..], XmlBoundaryErrorKind::Utf8),
            ("{", b"<Input/>".as_slice(), XmlBoundaryErrorKind::Schema),
        ] {
            let original = Box::new(parse_structured_xml_bytes(schema, bytes).unwrap_err());
            let allocation = original.as_ref() as *const XmlBoundaryError;
            let cause = original.source().unwrap() as *const dyn Error;
            let error = XmlDynamicInputDocumentExecutionError::from_input_boundary(
                Some(XmlInputSource::Named {
                    index: 2,
                    name: "labels",
                }),
                original,
            );
            assert_eq!(
                error.owner,
                Some(XmlDynamicInputDocumentOwner::Input(XmlInputSource::Named {
                    index: 2,
                    name: "labels"
                }))
            );
            assert_eq!(error.boundary.kind, expected);
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
    fn exact_adapter_recovery_moves_original_request_and_boundary_without_cloning() {
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
        // This fresh adapter is terminal after its single failed load; no retry.
        let original = adapter.recover(RuntimeError::DynamicSourceLoad {
            source: "catalog",
            path: "b.xml".into(),
            message: marker,
        });
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let cause = original.boundary.source().unwrap() as *const dyn Error;
        let request = original.request.unwrap();
        let request_allocation = request.as_ref() as *const XmlDynamicInputRequest;
        let error = XmlDynamicInputDocumentExecutionError::from_dynamic_input_boundary(
            request,
            original.boundary,
        );
        assert_eq!(
            error.owner,
            Some(XmlDynamicInputDocumentOwner::Input(XmlInputSource::Named {
                index: 1,
                name: "catalog"
            }))
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
    fn output_member_and_global_schema_count_mapping_conversions_never_invent_a_request() {
        let original = XmlDocumentExecutionError::serialization(
            1,
            "../雪😀.xml",
            XmlBoundaryError::with_source(
                XmlBoundaryErrorKind::Output,
                std::io::Error::other("original writer"),
            ),
        );
        let allocation = original.boundary.as_ref() as *const XmlBoundaryError;
        let cause = original.boundary.source().unwrap() as *const dyn Error;
        let member = XmlDynamicInputDocumentExecutionError::from(original);
        assert_eq!(
            member.owner,
            Some(XmlDynamicInputDocumentOwner::Member(
                XmlDocumentOutputOwner {
                    target: XmlOutputTarget::Primary,
                    index: 1,
                    path: "../雪😀.xml".into()
                }
            ))
        );
        assert!(member.request.is_none());
        assert!(std::ptr::eq(member.boundary.as_ref(), allocation));
        assert!(std::ptr::eq(member.boundary.source().unwrap(), cause));
        let schema = XmlDocumentExecutionError::serialization(
            1,
            "same.xml",
            parse_structured_xml("{", "<Input/>").unwrap_err(),
        );
        let allocation = schema.boundary.as_ref() as *const XmlBoundaryError;
        let error = XmlDynamicInputDocumentExecutionError::from(schema);
        assert!(error.owner.is_none() && error.request.is_none());
        assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Schema);
        assert!(std::ptr::eq(error.boundary.as_ref(), allocation));
        let count = XmlDynamicInputDocumentExecutionError::from(
            XmlDocumentSetBudget::new(4097).err().unwrap(),
        );
        assert!(count.owner.is_none() && count.request.is_none());
        assert_eq!(count.boundary.kind, XmlBoundaryErrorKind::Output);
        let mapping = XmlDynamicInputDocumentExecutionError::from(XmlBoundaryError::from(
            RuntimeError::DynamicSourceLoad {
                source: "catalog",
                path: "b.xml".into(),
                message: "original host marker".into(),
            },
        ));
        assert!(mapping.owner.is_none() && mapping.request.is_none());
        assert!(
            matches!(mapping.boundary.source().unwrap().downcast_ref::<RuntimeError>(),
            Some(RuntimeError::DynamicSourceLoad { message, .. }) if message == "original host marker")
        );
    }
}
