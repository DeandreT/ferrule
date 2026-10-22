//! Tagged dynamic-source failures: cause retention for each loader family,
//! old/new adapter equivalence, request order, precedence and projection.

use std::cell::{Cell, RefCell};
use std::error::Error;

use ir::{Instance, Value};

use super::*;
use crate::{
    CsvBoundaryError, DynamicXmlSourceLoader, JsonBoundaryError, XmlDynamicSourceAdapter,
    XmlDynamicSourcePolicy, XmlExecutionError, XmlInputSetBudget, field, group, repeated, scalar,
};

const CATALOG_SCHEMA: &str = r#"{"name":"Catalog","kind":{"kind":"group","children":[{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"value","kind":{"kind":"scalar","ty":"string"}}]}}]}}"#;
const XML_SCHEMA: &str = r#"{"name":"Catalog","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Amount","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}]}}"#;

fn drivers(paths: &[&str]) -> Instance {
    group([field(
        "Files",
        repeated(
            paths
                .iter()
                .map(|path| group([field("path", scalar(Value::String((*path).into())))])),
        ),
    )])
}

fn rows(path: &str) -> Instance {
    group([field(
        "Rows",
        repeated([group([field("value", scalar(Value::String(path.into())))])]),
    )])
}

fn load_all(source: &Instance, loader: &dyn DynamicSourceLoader) -> Result<usize, RuntimeError> {
    let root = ScopeContext::new(source).with_dynamic_source_loader(loader);
    DynamicSourceItems::load(&root, "Catalog", &["Files"], &["Rows"], 7, |driver| {
        driver.resolve_scalar(&["path"]).map_err(Into::into)
    })
    .map(|items| items.contexts().len())
}

/// Fails the request for `fail` with one recorded error; other paths load.
struct Scripted<F> {
    fail: &'static str,
    calls: RefCell<Vec<String>>,
    failure: RefCell<Option<F>>,
}

impl<F> Scripted<F> {
    fn new(fail: &'static str, failure: F) -> Self {
        Self {
            fail,
            calls: RefCell::default(),
            failure: RefCell::new(Some(failure)),
        }
    }

    fn take(&self, path: &str) -> Option<F> {
        self.calls.borrow_mut().push(path.to_owned());
        (path == self.fail).then(|| self.failure.borrow_mut().take().unwrap())
    }
}

impl DynamicSourceLoader for Scripted<String> {
    fn load(&self, _: &str, path: &str) -> Result<Instance, String> {
        self.take(path).map_or_else(|| Ok(rows(path)), Err)
    }
}

impl TaggedDynamicSourceLoader for Scripted<DynamicSourceFault> {
    fn load_tagged(&self, _: &str, path: &str) -> Result<Instance, DynamicSourceFault> {
        self.take(path).map_or_else(|| Ok(rows(path)), Err)
    }
}

impl TaggedDynamicBytesLoader for Scripted<DynamicSourceFault> {
    fn load_tagged(&self, _: &str, path: &str) -> Result<Vec<u8>, DynamicSourceFault> {
        self.take(path).map_or_else(
            || Ok(format!(r#"{{"Rows":[{{"value":"{path}"}}]}}"#).into_bytes()),
            Err,
        )
    }
}

fn cause_address(fault: &DynamicSourceFault) -> usize {
    fault
        .cause()
        .map_or(0, |cause| std::ptr::from_ref(cause).cast::<()>() as usize)
}

#[test]
fn legacy_instance_loader_moves_its_original_string_and_stops_at_the_first_failure() {
    let message = String::from("host refused b.json");
    let address = message.as_ptr() as usize;
    let host = Scripted::new("b.json", message);
    let error = load_all(&drivers(&["a.json", "b.json", "c.json"]), &host).unwrap_err();
    let RuntimeError::DynamicSourceLoad {
        source,
        path,
        message,
    } = &error
    else {
        panic!("legacy loaders keep the legacy variant, got {error:?}");
    };
    assert_eq!((*source, path.as_str()), ("Catalog", "b.json"));
    assert_eq!(message.as_ptr() as usize, address);
    assert_eq!(*host.calls.borrow(), ["a.json", "b.json"]);
}

#[test]
fn tagged_instance_loader_retains_ownership_phase_and_original_cause() {
    let fault = DynamicSourceFault::acquisition(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "denied",
    ));
    let address = cause_address(&fault);
    let host = Scripted::new("b.json", fault);
    let error = load_all(
        &drivers(&["a.json", "b.json", "c.json"]),
        &TaggedLoaderAdapter(&host),
    )
    .unwrap_err();
    let RuntimeError::DynamicSourceFailure(failure) = &error else {
        panic!("tagged loaders keep their fault, got {error:?}");
    };
    assert_eq!(
        (failure.source, failure.path.as_str()),
        ("Catalog", "b.json")
    );
    assert_eq!(failure.fault.phase(), DynamicSourcePhase::Acquisition);
    assert_eq!(cause_address(&failure.fault), address);
    let io = error
        .source()
        .and_then(Error::source)
        .and_then(|cause| cause.downcast_ref::<std::io::Error>())
        .unwrap();
    assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(
        error.to_string(),
        r#"dynamic source "Catalog" could not load "b.json": denied"#
    );
    assert_eq!(*host.calls.borrow(), ["a.json", "b.json"]);
}

#[test]
fn old_and_new_instance_adapters_are_equivalent() {
    let source = drivers(&["a.json", "b.json", "c.json"]);
    let legacy = Scripted::new("b.json", String::from("refused"));
    let tagged = Scripted::new("b.json", DynamicSourceFault::legacy("refused".into()));
    assert_eq!(
        load_all(&source, &legacy),
        load_all(&source, &TaggedLoaderAdapter(&tagged))
    );
    assert_eq!(*legacy.calls.borrow(), *tagged.calls.borrow());

    // New to old: the legacy method renders exactly the fault's message.
    let ok = drivers(&["a.json", "c.json"]);
    assert_eq!(load_all(&ok, &legacy), Ok(2));
    let fault = DynamicSourceFault::codec(DynamicSourceCodecKind::Document, "bad rows");
    let rendered = fault.to_string();
    let tagged = Scripted::new("b.json", fault);
    assert_eq!(
        DynamicSourceLoader::load(&TaggedLoaderAdapter(&tagged), "Catalog", "b.json"),
        Err(rendered)
    );

    // Old to new: the provided method moves the legacy allocation both ways.
    let message = String::from("kept");
    let address = message.as_ptr() as usize;
    let fault = DynamicSourceFault::legacy(message);
    assert_eq!(fault.phase(), DynamicSourcePhase::Acquisition);
    let message = fault.into_message();
    assert_eq!(
        (message.as_str(), message.as_ptr() as usize),
        ("kept", address)
    );
}

#[test]
fn request_precedence_is_unchanged_before_any_callback() {
    struct Never;
    impl DynamicSourceLoader for Never {
        fn load(&self, _: &str, _: &str) -> Result<Instance, String> {
            panic!("no callback may run");
        }
    }
    let source = drivers(&["a.json"]);
    let evaluated = Cell::new(0);
    let missing = DynamicSourceItems::load(
        &ScopeContext::new(&source),
        "Catalog",
        &["Files"],
        &[],
        3,
        |_| {
            evaluated.set(evaluated.get() + 1);
            Ok(Value::String("a.json".into()))
        },
    );
    assert_eq!(
        missing.err(),
        Some(RuntimeError::MissingDynamicSourceLoader { source: "Catalog" })
    );
    assert_eq!(evaluated.get(), 0);

    let root = ScopeContext::new(&source).with_dynamic_source_loader(&Never);
    let long = "x".repeat(MAX_DYNAMIC_SOURCE_PATH_BYTES + 1);
    let error = DynamicSourceItems::load(&root, "Catalog", &["Files"], &[], 3, |_| {
        Ok(Value::String(long.clone()))
    })
    .err()
    .unwrap();
    assert_eq!(
        DynamicSourceDiagnostic::from_runtime_error(&error),
        Some(DynamicSourceDiagnostic {
            phase: DynamicSourcePhase::Request,
            endpoint: "Catalog",
            category: DynamicSourceCategory::PathTooLong,
            path: None,
            resource: None,
        })
    );
    let error =
        DynamicSourceItems::load(&root, "Catalog", &["Files"], &[], 3, |_| Ok(Value::Int(1)))
            .err()
            .unwrap();
    assert_eq!(
        DynamicSourceDiagnostic::from_runtime_error(&error).map(|diagnostic| diagnostic.category),
        Some(DynamicSourceCategory::PathType)
    );
}

fn schema(source: &str) -> Option<&'static str> {
    (source == "Catalog").then_some(CATALOG_SCHEMA)
}

struct JsonHost {
    documents: RefCell<Vec<Result<Vec<u8>, String>>>,
    calls: RefCell<Vec<(String, String)>>,
}

impl JsonHost {
    fn new(documents: impl IntoIterator<Item = Result<Vec<u8>, String>>) -> Self {
        let mut documents = documents.into_iter().collect::<Vec<_>>();
        documents.reverse();
        Self {
            documents: RefCell::new(documents),
            calls: RefCell::default(),
        }
    }
}

impl DynamicJsonSourceLoader for JsonHost {
    fn load(&self, source: &str, path: &str) -> Result<Vec<u8>, String> {
        self.calls.borrow_mut().push((source.into(), path.into()));
        self.documents.borrow_mut().pop().unwrap()
    }
}

fn json_fault(adapter: &JsonDynamicSourceAdapter<'_>, source: &str) -> DynamicSourceFault {
    DynamicSourceLoader::load_tagged(adapter, source, "p.json").unwrap_err()
}

#[test]
fn json_adapter_retains_codec_causes_and_legacy_rendering() {
    let host = JsonHost::new([
        Ok(br#"{"Rows":[{"value":"ok"}]}"#.to_vec()),
        Ok(vec![b'{', 0xff]),
        Ok(br#"{"Rows":"#.to_vec()),
        Ok(b"{}".to_vec()),
    ]);
    let adapter = JsonDynamicSourceAdapter::new(&host, schema);
    assert!(DynamicSourceLoader::load_tagged(&adapter, "Catalog", "p.json").is_ok());

    let utf8 = json_fault(&adapter, "Catalog");
    assert_eq!(utf8.category(), DynamicSourceCategory::Encoding);
    let invalid = vec![b'{', 0xff];
    let original = std::str::from_utf8(&invalid).unwrap_err();
    assert_eq!(
        utf8.cause().unwrap().downcast_ref::<std::str::Utf8Error>(),
        Some(&original)
    );
    // The display text is the previous generated adapter's legacy message.
    assert_eq!(
        utf8.to_string(),
        format!("document is not UTF-8: {original}")
    );

    let document = json_fault(&adapter, "Catalog");
    assert_eq!(document.category(), DynamicSourceCategory::Document);
    let boundary = document
        .cause()
        .unwrap()
        .downcast_ref::<JsonBoundaryError>()
        .unwrap();
    let JsonBoundaryError::InvalidDocument(cause) = boundary else {
        panic!("format failures are retained, got {boundary:?}");
    };
    assert!(cause.source().unwrap().is::<format_json::JsonFormatError>());
    assert_eq!(
        document.to_string(),
        crate::parse_json(CATALOG_SCHEMA, r#"{"Rows":"#)
            .unwrap_err()
            .to_string()
    );

    let undeclared = json_fault(&adapter, "Other");
    assert_eq!(
        undeclared.category(),
        DynamicSourceCategory::UndeclaredSource
    );
    assert_eq!(
        undeclared.to_string(),
        r#"undeclared dynamic source "Other""#
    );
    // The host is called before endpoint codec selection, as before.
    assert_eq!(host.calls.borrow().len(), 4);
}

#[test]
fn json_adapter_resource_limits_are_typed_and_ordered() {
    let host = JsonHost::new([
        Ok(br#"{"Rows":[]}"#.to_vec()),
        Ok(vec![b' '; 17]),
        Ok(br#"{"Rows":[]} "#.to_vec()),
    ]);
    let adapter = JsonDynamicSourceAdapter::with_limits(&host, schema, 16, 20);
    assert!(DynamicSourceLoader::load_tagged(&adapter, "Catalog", "a").is_ok());
    let document = json_fault(&adapter, "Catalog");
    assert_eq!(
        document.resource_limit(),
        Some(DynamicSourceResourceLimit {
            resource: DynamicSourceResource::DocumentBytes,
            observed: 17,
            limit: 16,
        })
    );
    assert_eq!(
        document.to_string(),
        "document exceeds the 16-byte dynamic-source limit"
    );
    // A refused document charges nothing; the next one exceeds the total.
    let total = json_fault(&adapter, "Catalog");
    assert_eq!(
        total.resource_limit(),
        Some(DynamicSourceResourceLimit {
            resource: DynamicSourceResource::TotalBytes,
            observed: 23,
            limit: 20,
        })
    );
    assert_eq!(
        total.to_string(),
        "documents exceed the 20-byte combined dynamic-source limit"
    );
    assert_eq!(total.phase(), DynamicSourcePhase::Resource);
    assert!(total.cause().is_none());
}

#[test]
fn json_host_failures_pass_through_with_their_original_causes() {
    let message = String::from("json host refused");
    let address = message.as_ptr() as usize;
    let legacy = JsonHost::new([Err(message)]);
    let adapter = JsonDynamicSourceAdapter::new(&legacy, schema);
    let error = load_all(&drivers(&["a.json"]), &adapter).unwrap_err();
    let RuntimeError::DynamicSourceLoad { message, .. } = &error else {
        panic!("legacy JSON host failures keep the legacy variant, got {error:?}");
    };
    assert_eq!(message.as_ptr() as usize, address);
    let legacy_text = DynamicSourceLoader::load(
        &JsonDynamicSourceAdapter::new(&JsonHost::new([Err("same".into())]), schema),
        "Catalog",
        "a.json",
    );
    assert_eq!(legacy_text.err().as_deref(), Some("same"));

    let fault = DynamicSourceFault::acquisition(std::io::Error::other("offline"));
    let address = cause_address(&fault);
    let tagged = Scripted::new("b.json", fault);
    let tagged = TaggedLoaderAdapter(&tagged);
    let adapter = JsonDynamicSourceAdapter::new(&tagged, schema);
    let error = load_all(&drivers(&["a.json", "b.json"]), &adapter).unwrap_err();
    let RuntimeError::DynamicSourceFailure(failure) = &error else {
        panic!("tagged JSON host failures keep their fault, got {error:?}");
    };
    assert_eq!(cause_address(&failure.fault), address);
    assert_eq!(failure.path, "b.json");
}

#[test]
fn json_codec_failures_reach_the_generated_boundary_with_their_causes() {
    let host = JsonHost::new([Ok(vec![0xff])]);
    let adapter = JsonDynamicSourceAdapter::new(&host, schema);
    let error = JsonBoundaryError::from(load_all(&drivers(&["a.json"]), &adapter).unwrap_err());
    let mut chain = Vec::new();
    let mut current: Option<&(dyn Error + 'static)> = Some(&error);
    while let Some(error) = current {
        chain.push(error);
        current = error.source();
    }
    assert!(chain.iter().any(|error| error.is::<std::str::Utf8Error>()));
    assert_eq!(
        DynamicSourceDiagnostic::find(&error),
        Some(DynamicSourceDiagnostic {
            phase: DynamicSourcePhase::Codec,
            endpoint: "Catalog",
            category: DynamicSourceCategory::Encoding,
            path: Some("a.json".into()),
            resource: None,
        })
    );
}

struct XmlHost {
    result: RefCell<Option<Result<Vec<u8>, DynamicSourceFault>>>,
}

impl TaggedDynamicBytesLoader for XmlHost {
    fn load_tagged(&self, _: &str, _: &str) -> Result<Vec<u8>, DynamicSourceFault> {
        self.result.borrow_mut().take().unwrap()
    }
}

fn xml_adapter<'a>(host: &'a dyn DynamicXmlSourceLoader) -> XmlDynamicSourceAdapter<'a> {
    XmlDynamicSourceAdapter::new(
        host,
        XmlDynamicSourcePolicy {
            declaration_index: 2,
            source: "Catalog",
            schema: XML_SCHEMA,
        },
        XmlInputSetBudget::new(1).unwrap(),
    )
}

fn xml_load(adapter: &XmlDynamicSourceAdapter<'_>) -> XmlExecutionError {
    let source = drivers(&["a.xml"]);
    let error = load_all(&source, adapter).unwrap_err();
    adapter.recover(error)
}

#[test]
fn xml_tagged_host_fault_stays_unowned_with_its_original_cause() {
    let fault = DynamicSourceFault::acquisition(std::io::Error::other("unreachable share"));
    let address = cause_address(&fault);
    let host = XmlHost {
        result: RefCell::new(Some(Err(fault))),
    };
    let tagged = TaggedLoaderAdapter(&host);
    let adapter = xml_adapter(&tagged);
    let error = xml_load(&adapter);
    assert!(error.request.is_none() && error.input.is_none());
    assert_eq!(error.boundary.kind, crate::XmlBoundaryErrorKind::Mapping);
    let RuntimeError::DynamicSourceFailure(failure) = error
        .boundary
        .source()
        .unwrap()
        .downcast_ref::<RuntimeError>()
        .unwrap()
    else {
        panic!("tagged host fault must be retained");
    };
    assert_eq!(cause_address(&failure.fault), address);
    assert_eq!(
        DynamicSourceDiagnostic::find(&error).map(|diagnostic| diagnostic.phase),
        Some(DynamicSourcePhase::Acquisition)
    );
}

#[test]
fn xml_product_refusal_through_the_tagged_path_keeps_its_owned_recovery() {
    let host = XmlHost {
        result: RefCell::new(Some(Ok(vec![0xff]))),
    };
    let tagged = TaggedLoaderAdapter(&host);
    let adapter = xml_adapter(&tagged);
    let error = xml_load(&adapter);
    let request = error.request.as_deref().unwrap();
    assert_eq!(
        (
            request.source,
            request.path.as_str(),
            request.callback_invoked
        ),
        ("Catalog", "a.xml", true)
    );
    assert_eq!(error.boundary.kind, crate::XmlBoundaryErrorKind::Utf8);
    assert_eq!(
        DynamicSourceDiagnostic::find(&error),
        Some(DynamicSourceDiagnostic {
            phase: DynamicSourcePhase::Codec,
            endpoint: "Catalog",
            category: DynamicSourceCategory::Encoding,
            path: Some("a.xml".into()),
            resource: None,
        })
    );
}

#[test]
fn xml_owned_resource_refusals_project_their_counters() {
    let host = XmlHost {
        result: RefCell::new(None),
    };
    let tagged = TaggedLoaderAdapter(&host);
    let adapter = XmlDynamicSourceAdapter::new(
        &tagged,
        XmlDynamicSourcePolicy {
            declaration_index: 2,
            source: "Catalog",
            schema: XML_SCHEMA,
        },
        XmlInputSetBudget::new(4096).unwrap(),
    );
    let error = xml_load(&adapter);
    assert_eq!(
        DynamicSourceDiagnostic::find(&error),
        Some(DynamicSourceDiagnostic {
            phase: DynamicSourcePhase::Resource,
            endpoint: "Catalog",
            category: DynamicSourceCategory::RequestCount,
            path: Some("a.xml".into()),
            resource: Some(DynamicSourceResourceLimit {
                resource: DynamicSourceResource::RequestCount,
                observed: 4097,
                limit: 4096,
            }),
        })
    );
}

#[test]
fn projection_tokens_and_runtime_mapping_are_stable() {
    let phases = [
        DynamicSourcePhase::Request,
        DynamicSourcePhase::Acquisition,
        DynamicSourcePhase::Codec,
        DynamicSourcePhase::Resource,
    ];
    assert_eq!(
        phases.map(DynamicSourcePhase::as_str),
        ["request", "acquisition", "codec", "resource"]
    );
    let categories = [
        DynamicSourceCategory::MissingLoader,
        DynamicSourceCategory::PathType,
        DynamicSourceCategory::PathTooLong,
        DynamicSourceCategory::TooManyRequests,
        DynamicSourceCategory::Host,
        DynamicSourceCategory::Encoding,
        DynamicSourceCategory::Document,
        DynamicSourceCategory::UndeclaredSource,
        DynamicSourceCategory::DocumentBytes,
        DynamicSourceCategory::TotalBytes,
        DynamicSourceCategory::RequestCount,
    ];
    assert_eq!(
        categories.map(DynamicSourceCategory::as_str),
        [
            "missing_loader",
            "path_type",
            "path_too_long",
            "too_many_requests",
            "host",
            "encoding",
            "document",
            "undeclared_source",
            "document_bytes",
            "total_bytes",
            "request_count",
        ]
    );

    let project = |error: &RuntimeError| {
        DynamicSourceDiagnostic::from_runtime_error(error)
            .map(|diagnostic| (diagnostic.phase.as_str(), diagnostic.category.as_str()))
    };
    let failure = |fault| {
        RuntimeError::DynamicSourceFailure(DynamicSourceLoadFailure {
            source: "Catalog",
            path: "p".into(),
            fault,
        })
    };
    for (error, expected) in [
        (
            RuntimeError::MissingDynamicSourceLoader { source: "Catalog" },
            ("request", "missing_loader"),
        ),
        (
            RuntimeError::DynamicSourceTooMany {
                source: "Catalog",
                maximum: 1,
            },
            ("request", "too_many_requests"),
        ),
        (
            RuntimeError::DynamicSourceLoad {
                source: "Catalog",
                path: "p".into(),
                message: "m".into(),
            },
            ("acquisition", "host"),
        ),
        (
            failure(DynamicSourceFault::acquisition("m")),
            ("acquisition", "host"),
        ),
        (
            failure(DynamicSourceFault::undeclared_source("Other")),
            ("codec", "undeclared_source"),
        ),
        (
            failure(DynamicSourceFault::resource(
                DynamicSourceResource::RequestCount,
                2,
                1,
            )),
            ("resource", "request_count"),
        ),
    ] {
        assert_eq!(project(&error), Some(expected), "{error:?}");
    }
    assert_eq!(project(&RuntimeError::FilterMapCancelled), None);
    let csv = CsvBoundaryError::from(failure(DynamicSourceFault::codec(
        DynamicSourceCodecKind::Document,
        "bad",
    )));
    assert_eq!(
        DynamicSourceDiagnostic::find(&csv).map(|diagnostic| diagnostic.path),
        Some(Some("p".into()))
    );
}
