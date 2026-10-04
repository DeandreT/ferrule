//! Raw XML documents requested only by the existing typed driver runtime.

use std::cell::{Cell, RefCell};

use super::{XmlBoundaryError, XmlExecutionError, XmlInputSetBudget, XmlInputSource};
use crate::{DynamicSourceLoader, Instance, RuntimeError};

/// The host resolves and confines the logical path; generated code performs no I/O.
pub trait DynamicXmlSourceLoader {
    fn load(&self, source: &str, logical_path: &str) -> Result<Vec<u8>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDynamicInputRequest {
    pub declaration_index: usize,
    pub source: &'static str,
    pub path: String,
    pub ordinal: u64,
    pub callback_invoked: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct XmlDynamicSourcePolicy {
    pub declaration_index: usize,
    pub source: &'static str,
    pub schema: &'static str,
}

// The typed loader moves its String failure unchanged into DynamicSourceLoad.
// The original nonempty String allocation identifies this adapter's failed load.
// Matching text from an external host cannot arm or recover the product channel.
const PRODUCT_REFUSAL: &str = "generated XML input adapter refused a dynamic document";

/// One execution's dynamic byte admission and product-error recovery channel.
///
/// Create a fresh adapter for each execution. After any failed load, including
/// a host failure, this adapter is terminal: do not retry or reuse it. Only the
/// first failed load may be recovered. To restore a product refusal, keep the
/// original nonempty marker String alive and move it unchanged through the
/// typed DynamicSourceLoad error into synchronous recovery. A clone or matching
/// text cannot identify that failure.
/// Generated entry points already abort and recover immediately on failure.
pub struct XmlDynamicSourceAdapter<'a> {
    loader: &'a dyn DynamicXmlSourceLoader,
    policy: XmlDynamicSourcePolicy,
    budget: RefCell<XmlInputSetBudget>,
    ordinal: Cell<u64>,
    failure: RefCell<Option<(XmlExecutionError, usize)>>,
}

impl<'a> XmlDynamicSourceAdapter<'a> {
    pub fn new(
        loader: &'a dyn DynamicXmlSourceLoader,
        policy: XmlDynamicSourcePolicy,
        budget: XmlInputSetBudget,
    ) -> Self {
        Self {
            loader,
            policy,
            budget: RefCell::new(budget),
            ordinal: Cell::new(0),
            failure: RefCell::new(None),
        }
    }

    /// Restores only the product refusal propagated by this adapter's failed load.
    /// An external host failure and any unrelated mapping failure stay Mapping.
    /// To restore a product refusal, pass the first load failure synchronously,
    /// with its original marker String still live inside the typed error.
    /// This does not make load retries valid.
    pub fn recover(&self, error: RuntimeError) -> XmlExecutionError {
        let matches = if let RuntimeError::DynamicSourceLoad {
            source,
            path,
            message,
        } = &error
        {
            self.failure
                .borrow()
                .as_ref()
                .is_some_and(|(failure, marker)| {
                    failure.request.as_ref().is_some_and(|request| {
                        request.source == *source
                            && request.path == *path
                            && message.as_ptr() as usize == *marker
                            && message == PRODUCT_REFUSAL
                    })
                })
        } else {
            false
        };
        if matches {
            return self
                .failure
                .borrow_mut()
                .take()
                .expect("matched armed XML input refusal")
                .0;
        }
        XmlBoundaryError::from(error).into()
    }

    fn refuse(&self, request: XmlDynamicInputRequest, boundary: XmlBoundaryError) -> String {
        let marker = PRODUCT_REFUSAL.to_owned();
        let mut failure = self.failure.borrow_mut();
        if failure.is_none() {
            *failure = Some((
                XmlExecutionError::dynamic_input(request, boundary),
                marker.as_ptr() as usize,
            ));
        }
        marker
    }
}

impl DynamicSourceLoader for XmlDynamicSourceAdapter<'_> {
    fn load(&self, source: &str, path: &str) -> Result<Instance, String> {
        if source != self.policy.source {
            return Err(format!("undeclared dynamic XML source {source:?}"));
        }
        let owner = XmlInputSource::Named {
            index: self.policy.declaration_index,
            name: self.policy.source,
        };
        let mut request = XmlDynamicInputRequest {
            declaration_index: self.policy.declaration_index,
            source: self.policy.source,
            path: path.to_owned(),
            ordinal: self.ordinal.get().saturating_add(1),
            callback_invoked: false,
        };
        let reservation = self.budget.borrow_mut().reserve(owner);
        if let Err(error) = reservation {
            return Err(self.refuse(request, error.into_boundary()));
        }
        self.ordinal.set(request.ordinal);
        request.callback_invoked = true;
        // Deliberately outside every product-parser error handler.
        let document = self.loader.load(source, path)?;
        if let Err(error) = super::check_document_size(document.len()) {
            return Err(self.refuse(request, error));
        }
        let charge = self.budget.borrow_mut().charge(owner, document.len());
        if let Err(error) = charge {
            return Err(self.refuse(request, error.into_boundary()));
        }
        super::parse_structured_xml_bytes(self.policy.schema, &document)
            .map_err(|error| self.refuse(request, error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::XmlInputSetResourceError;
    use std::error::Error;

    struct Loader {
        calls: Cell<usize>,
        result: Result<Vec<u8>, String>,
    }
    impl DynamicXmlSourceLoader for Loader {
        fn load(&self, _: &str, _: &str) -> Result<Vec<u8>, String> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }
    fn policy() -> XmlDynamicSourcePolicy {
        XmlDynamicSourcePolicy {
            declaration_index: 7,
            source: "catalog",
            schema: "{}",
        }
    }
    fn propagated(message: String, path: &str) -> RuntimeError {
        RuntimeError::DynamicSourceLoad {
            source: "catalog",
            path: path.to_owned(),
            message,
        }
    }
    #[test]
    fn count_refusal_reserves_no_host_callback_and_retains_original_resource() {
        let host = Loader {
            calls: Cell::new(0),
            result: Ok(vec![0xff]),
        };
        let adapter =
            XmlDynamicSourceAdapter::new(&host, policy(), XmlInputSetBudget::new(4096).unwrap());
        let message = DynamicSourceLoader::load(&adapter, "catalog", "late.xml").unwrap_err();
        let error = adapter.recover(propagated(message, "late.xml"));
        assert_eq!(host.calls.get(), 0);
        assert_eq!(
            error.input,
            Some(XmlInputSource::Named {
                index: 7,
                name: "catalog"
            })
        );
        assert!(error.output.is_none());
        assert!(!error.request.as_ref().unwrap().callback_invoked);
        assert_eq!(error.request.as_ref().unwrap().ordinal, 1);
        let cause = error
            .boundary
            .source()
            .unwrap()
            .downcast_ref::<XmlInputSetResourceError>()
            .unwrap();
        assert_eq!(
            (cause.resource, cause.observed_count, cause.limit),
            ("xml_input_artifact_count", 4097, 4096)
        );
        assert!(std::ptr::eq(
            error
                .source()
                .unwrap()
                .downcast_ref::<XmlBoundaryError>()
                .unwrap(),
            error.boundary.as_ref()
        ));
    }
    #[test]
    fn host_failure_does_not_arm_product_channel_or_charge_bytes() {
        let host = Loader {
            calls: Cell::new(0),
            result: Err(PRODUCT_REFUSAL.to_owned()),
        };
        let adapter =
            XmlDynamicSourceAdapter::new(&host, policy(), XmlInputSetBudget::new(1).unwrap());
        let message = DynamicSourceLoader::load(&adapter, "catalog", "host.xml").unwrap_err();
        let error = adapter.recover(propagated(message, "host.xml"));
        assert_eq!(host.calls.get(), 1);
        adapter
            .budget
            .borrow_mut()
            .charge(
                XmlInputSource::Primary,
                super::super::MAX_XML_INPUT_SET_BYTES as usize,
            )
            .unwrap();
        assert!(error.input.is_none() && error.output.is_none() && error.request.is_none());
        assert_eq!(
            error.boundary.kind,
            super::super::XmlBoundaryErrorKind::Mapping
        );
        assert!(
            error
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>()
                .is_some()
        );
    }
    #[test]
    fn invalid_utf8_keeps_the_original_boundary_and_request_after_callback() {
        let host = Loader {
            calls: Cell::new(0),
            result: Ok(vec![0xff]),
        };
        let adapter =
            XmlDynamicSourceAdapter::new(&host, policy(), XmlInputSetBudget::new(1).unwrap());
        let message = DynamicSourceLoader::load(&adapter, "catalog", "bad.xml").unwrap_err();
        let forged = adapter.recover(propagated(message.clone(), "bad.xml"));
        assert!(forged.request.is_none());
        assert_eq!(
            forged.boundary.kind,
            super::super::XmlBoundaryErrorKind::Mapping
        );
        // An unrelated mapping failure cannot take this adapter's armed cause.
        let unrelated =
            adapter.recover(RuntimeError::MissingDynamicSourceLoader { source: "catalog" });
        assert!(unrelated.request.is_none());
        let error = adapter.recover(propagated(message, "bad.xml"));
        assert_eq!(
            error.boundary.kind,
            super::super::XmlBoundaryErrorKind::Utf8
        );
        assert!(error.request.as_ref().unwrap().callback_invoked);
        assert_eq!(host.calls.get(), 1);
        adapter
            .budget
            .borrow_mut()
            .charge(
                XmlInputSource::Primary,
                (super::super::MAX_XML_INPUT_SET_BYTES - 1) as usize,
            )
            .unwrap();
        assert!(
            adapter
                .budget
                .borrow_mut()
                .charge(XmlInputSource::Primary, 1)
                .is_err()
        );
        assert!(
            error
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<std::str::Utf8Error>()
                .is_some()
        );
    }

    #[test]
    fn original_live_marker_does_not_restore_a_different_source_or_path() {
        for (source, path) in [("other", "bad.xml"), ("catalog", "other.xml")] {
            let host = Loader {
                calls: Cell::new(0),
                result: Ok(vec![0xff]),
            };
            let adapter =
                XmlDynamicSourceAdapter::new(&host, policy(), XmlInputSetBudget::new(1).unwrap());
            let original = DynamicSourceLoader::load(&adapter, "catalog", "bad.xml").unwrap_err();
            assert!(!original.is_empty());
            let marker = original.as_ptr() as usize;
            let error = adapter.recover(RuntimeError::DynamicSourceLoad {
                source,
                path: path.to_owned(),
                message: original,
            });
            assert_eq!(host.calls.get(), 1);
            assert!(error.input.is_none() && error.output.is_none() && error.request.is_none());
            assert_eq!(
                error.boundary.kind,
                super::super::XmlBoundaryErrorKind::Mapping
            );
            let retained = error
                .boundary
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>()
                .unwrap();
            match retained {
                RuntimeError::DynamicSourceLoad {
                    source: actual_source,
                    path: actual_path,
                    message,
                } => {
                    assert_eq!(*actual_source, source);
                    assert_eq!(actual_path, path);
                    assert_eq!(message.as_ptr() as usize, marker);
                    assert_eq!(message, PRODUCT_REFUSAL);
                }
                _ => panic!("the original typed load error must be retained"),
            }
            let armed = adapter.failure.borrow();
            let (failure, saved_marker) = armed.as_ref().unwrap();
            assert_eq!(*saved_marker, marker);
            let request = failure.request.as_ref().unwrap();
            assert_eq!(
                (
                    request.declaration_index,
                    request.source,
                    request.path.as_str()
                ),
                (7, "catalog", "bad.xml")
            );
            assert_eq!((request.ordinal, request.callback_invoked), (1, true));
            assert!(std::ptr::eq(
                error
                    .source()
                    .unwrap()
                    .downcast_ref::<XmlBoundaryError>()
                    .unwrap(),
                error.boundary.as_ref()
            ));
            // error still owns the original live marker. No further load or retry.
        }
    }
}
