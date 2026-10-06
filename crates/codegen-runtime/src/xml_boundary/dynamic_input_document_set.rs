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

    const MULTIPLE_FLOAT_SCHEMA: &str = r#"{"name":"Catalog","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Amount","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"float"}}]}}"#;
    const MULTIPLE_INT_SCHEMA: &str = r#"{"name":"Codes","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}]}}"#;
    const MULTIPLE_CATALOG: &[u8] = b"<Catalog><Amount>-0.0</Amount></Catalog>";
    const MULTIPLE_CODES: &[u8] = b"<Codes><Code>9007199254740993</Code></Codes>";

    struct MultiplePrimaryBytes {
        calls: std::cell::RefCell<Vec<(String, String)>>,
        invalid_codes: bool,
    }
    impl DynamicXmlSourceLoader for MultiplePrimaryBytes {
        fn load(&self, source: &str, path: &str) -> Result<Vec<u8>, String> {
            self.calls.borrow_mut().push((source.into(), path.into()));
            match source {
                "catalog" => Ok(MULTIPLE_CATALOG.to_vec()),
                "codes" if self.invalid_codes => Ok(vec![0xff]),
                "codes" => Ok(MULTIPLE_CODES.to_vec()),
                _ => Err("unexpected dynamic source".into()),
            }
        }
    }
    fn multiple_primary_policies() -> Vec<XmlDynamicSourcePolicy> {
        vec![
            XmlDynamicSourcePolicy {
                declaration_index: 1,
                source: "catalog",
                schema: MULTIPLE_FLOAT_SCHEMA,
            },
            XmlDynamicSourcePolicy {
                declaration_index: 3,
                source: "codes",
                schema: MULTIPLE_INT_SCHEMA,
            },
        ]
    }
    fn multiple_primary_scalar(instance: &crate::Instance, field: &str) -> crate::Value {
        let crate::Instance::Group(fields) = instance else {
            panic!("loaded group")
        };
        let crate::Instance::Scalar(value) =
            &fields.iter().find(|(name, _)| name == field).unwrap().1
        else {
            panic!("loaded scalar")
        };
        value.clone()
    }

    #[test]
    fn multiple_primary_adapter_keeps_same_path_distinct_scalar_domains() {
        let host = MultiplePrimaryBytes {
            calls: Default::default(),
            invalid_codes: false,
        };
        let adapter = XmlDynamicSourceAdapter::for_sources(
            &host,
            multiple_primary_policies(),
            XmlInputSetBudget::new(3).unwrap(),
        );
        let catalog = DynamicSourceLoader::load(&adapter, "catalog", "same.xml").unwrap();
        let codes = DynamicSourceLoader::load(&adapter, "codes", "same.xml").unwrap();
        let crate::Value::Float(amount) = multiple_primary_scalar(&catalog, "Amount") else {
            panic!("Float amount")
        };
        assert_eq!(amount.to_bits(), (-0.0f64).to_bits());
        assert_eq!(
            multiple_primary_scalar(&codes, "Code"),
            crate::Value::Int(9_007_199_254_740_993)
        );
        assert_eq!(
            host.calls.borrow().as_slice(),
            [
                ("catalog".to_owned(), "same.xml".to_owned()),
                ("codes".to_owned(), "same.xml".to_owned()),
            ]
        );
    }

    #[test]
    fn second_primary_source_recovery_keeps_shared_ordinal_and_original_allocations() {
        // Only counter setup is seeded. These tiny callbacks do not qualify real input caps.
        for mode in 0..3 {
            let host = MultiplePrimaryBytes {
                calls: Default::default(),
                invalid_codes: mode == 0,
            };
            let mut budget = XmlInputSetBudget::new(if mode == 1 { 4095 } else { 3 }).unwrap();
            if mode == 2 {
                budget
                    .charge(
                        XmlInputSource::Primary,
                        super::super::MAX_XML_INPUT_SET_BYTES as usize - MULTIPLE_CATALOG.len(),
                    )
                    .unwrap();
            }
            let adapter =
                XmlDynamicSourceAdapter::for_sources(&host, multiple_primary_policies(), budget);
            DynamicSourceLoader::load(&adapter, "catalog", "same.xml").unwrap();
            let marker = DynamicSourceLoader::load(&adapter, "codes", "same.xml").unwrap_err();
            let original = adapter.recover(RuntimeError::DynamicSourceLoad {
                source: "codes",
                path: "same.xml".into(),
                message: marker,
            });
            let boundary = original.boundary.as_ref() as *const XmlBoundaryError;
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
                    index: 3,
                    name: "codes"
                }))
            );
            let actual = error.request.as_ref().unwrap();
            assert_eq!(
                (
                    actual.declaration_index,
                    actual.source,
                    actual.path.as_str(),
                    actual.ordinal,
                    actual.callback_invoked
                ),
                (3, "codes", "same.xml", 2, mode != 1)
            );
            assert!(std::ptr::eq(actual.as_ref(), request_allocation));
            assert!(std::ptr::eq(error.boundary.as_ref(), boundary));
            assert!(std::ptr::eq(error.boundary.source().unwrap(), cause));
            assert_eq!(host.calls.borrow().len(), if mode == 1 { 1 } else { 2 });
            if mode == 0 {
                assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Utf8);
                assert!(error.boundary.source().unwrap().is::<std::str::Utf8Error>());
            } else {
                assert_eq!(error.boundary.kind, XmlBoundaryErrorKind::Input);
                let resource = error
                    .boundary
                    .source()
                    .unwrap()
                    .downcast_ref::<crate::XmlInputSetResourceError>()
                    .unwrap();
                assert_eq!(
                    (resource.resource, resource.observed_count, resource.limit),
                    if mode == 1 {
                        ("xml_input_artifact_count", 4097, 4096)
                    } else {
                        (
                            "xml_input_set_utf8_bytes",
                            super::super::MAX_XML_INPUT_SET_BYTES + MULTIPLE_CODES.len() as u64,
                            super::super::MAX_XML_INPUT_SET_BYTES,
                        )
                    }
                );
            }
            // First refusal is terminal; no additional source is loaded or recovered.
        }
    }
}
