use std::cell::RefCell;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use ir::{Instance, Value};

use super::dynamic_source_tests::{FixtureLoader, dynamic_project};
use super::{
    DynamicSourceCodecKind, DynamicSourceDiagnostic, DynamicSourceFault, DynamicSourceLoader,
    DynamicSourceResource, DynamicSourceResourceLimit, EngineError, ExecutionContext,
    RestrictedDynamicSourceError, run_with_context,
};

#[derive(Debug)]
struct ParseFailure(&'static str);

impl fmt::Display for ParseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "parse failure: {}", self.0)
    }
}

impl Error for ParseFailure {}

/// Records every request and fails selected paths with typed faults.
#[derive(Default)]
struct TaggedLoader {
    calls: RefCell<Vec<String>>,
}

fn fault_for(path: &str) -> Option<DynamicSourceFault> {
    Some(match path {
        "missing.xml" => DynamicSourceFault::acquisition(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing.xml is absent",
        )),
        "broken.xml" => {
            DynamicSourceFault::codec(DynamicSourceCodecKind::Document, ParseFailure("line 1"))
        }
        "huge.xml" => DynamicSourceFault::resource(DynamicSourceResource::DocumentBytes, 9, 8),
        _ => return None,
    })
}

impl DynamicSourceLoader for TaggedLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.load_tagged(source, path)
            .map_err(DynamicSourceFault::into_message)
    }

    fn load_tagged(&self, source: &str, path: &str) -> Result<Arc<Instance>, DynamicSourceFault> {
        self.calls.borrow_mut().push(path.to_owned());
        match fault_for(path) {
            Some(fault) => Err(fault),
            None => FixtureLoader
                .load(source, path)
                .map_err(DynamicSourceFault::legacy),
        }
    }

    fn load_restricted_tagged(
        &self,
        source: &str,
        path: &str,
        _policy: &crate::RestrictedPolicy,
    ) -> Result<Arc<Instance>, RestrictedDynamicSourceError> {
        if path == "denied.xml" {
            self.calls.borrow_mut().push(path.to_owned());
            return Err(crate::PolicyRefusal::HttpOriginDenied {
                origin: "https://denied.example".into(),
            }
            .into());
        }
        Ok(self.load_tagged(source, path)?)
    }
}

fn files(paths: &[Option<&str>]) -> Instance {
    Instance::Group(
        (vec![(
            "File".into(),
            Instance::Repeated(
                paths
                    .iter()
                    .map(|path| {
                        Instance::Scalar(
                            path.map_or(Value::Null, |path| Value::String(path.into())),
                        )
                    })
                    .collect(),
            ),
        )])
        .into(),
    )
}

fn run(loader: &dyn DynamicSourceLoader, source: &Instance) -> Result<Instance, EngineError> {
    run_with_context(
        &dynamic_project(),
        source,
        &ExecutionContext::new(Path::new("mapping.ferrule.json"))
            .with_dynamic_source_loader(loader),
    )
}

fn tokens(error: &EngineError) -> (&'static str, &'static str, String, Option<String>) {
    let diagnostic = DynamicSourceDiagnostic::from_engine_error(error).expect("dynamic failure");
    (
        diagnostic.phase.as_str(),
        diagnostic.category.as_str(),
        diagnostic.endpoint,
        diagnostic.path,
    )
}

#[test]
fn tagged_faults_keep_category_endpoint_path_and_original_cause() {
    let loader = TaggedLoader::default();
    let error = run(&loader, &files(&[Some("first.xml"), Some("missing.xml")])).unwrap_err();
    let EngineError::DynamicSourceFailure(failure) = &error else {
        panic!("unexpected error {error:?}");
    };
    assert_eq!(failure.source_name, "document");
    assert_eq!(failure.path, "missing.xml");
    assert_eq!(
        failure
            .fault
            .find_cause::<std::io::Error>()
            .map(std::io::Error::kind),
        Some(std::io::ErrorKind::NotFound)
    );
    assert_eq!(
        error.to_string(),
        "loading dynamic source `document` from `missing.xml` failed: missing.xml is absent"
    );
    // The legacy text and an empty source chain keep chain renderers unchanged.
    assert!(error.source().is_none());
    assert_eq!(
        tokens(&error),
        (
            "acquisition",
            "host",
            "document".into(),
            Some("missing.xml".into())
        )
    );

    let error = run(&loader, &files(&[Some("broken.xml")])).unwrap_err();
    assert_eq!(
        tokens(&error),
        (
            "codec",
            "document",
            "document".into(),
            Some("broken.xml".into())
        )
    );
    let EngineError::DynamicSourceFailure(failure) = &error else {
        panic!("unexpected error {error:?}");
    };
    assert!(failure.fault.find_cause::<ParseFailure>().is_some());
    assert_eq!(
        error.to_string(),
        "loading dynamic source `document` from `broken.xml` failed: parse failure: line 1"
    );

    let error = run(&loader, &files(&[Some("huge.xml")])).unwrap_err();
    let diagnostic = DynamicSourceDiagnostic::from_engine_error(&error).unwrap();
    assert_eq!(
        (diagnostic.phase.as_str(), diagnostic.category.as_str()),
        ("resource", "document_bytes")
    );
    assert_eq!(
        diagnostic.resource,
        Some(DynamicSourceResourceLimit {
            resource: DynamicSourceResource::DocumentBytes,
            observed: 9,
            limit: 8,
        })
    );
}

#[test]
fn tagged_loads_keep_request_order_and_first_failure() {
    let loader = TaggedLoader::default();
    let error = run(
        &loader,
        &files(&[
            Some("first.xml"),
            None,
            Some("broken.xml"),
            Some("missing.xml"),
        ]),
    )
    .unwrap_err();
    assert_eq!(tokens(&error).1, "document");
    assert_eq!(*loader.calls.borrow(), ["first.xml", "broken.xml"]);

    // A tagged loader produces the same output as its legacy adaptation.
    let ok = files(&[Some("first.xml"), Some("second.xml")]);
    assert_eq!(run(&loader, &ok), run(&FixtureLoader, &ok));
}

#[test]
fn legacy_loaders_keep_their_string_variant_and_project_as_host_acquisition() {
    let error = run(&FixtureLoader, &files(&[Some("other.xml")])).unwrap_err();
    assert_eq!(
        error,
        EngineError::DynamicSourceLoad {
            source_name: "document".into(),
            path: "other.xml".into(),
            message: "unexpected path other.xml".into(),
        }
    );
    assert_eq!(
        tokens(&error),
        (
            "acquisition",
            "host",
            "document".into(),
            Some("other.xml".into())
        )
    );
    // The default tagged adapter moves the original message back unchanged.
    assert_eq!(
        FixtureLoader
            .load_tagged("document", "other.xml")
            .unwrap_err()
            .into_legacy_message()
            .ok(),
        Some("unexpected path other.xml".into())
    );
}

#[test]
fn request_failures_project_before_any_host_call() {
    let missing = super::run(&dynamic_project(), &files(&[Some("first.xml")])).unwrap_err();
    assert_eq!(
        tokens(&missing),
        ("request", "missing_loader", "document".into(), None)
    );
    let source = Instance::Group(
        (vec![(
            "File".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::Bool(true))]),
        )])
        .into(),
    );
    let loader = TaggedLoader::default();
    let path = run(&loader, &source).unwrap_err();
    assert_eq!(
        tokens(&path),
        ("request", "path_type", "document".into(), None)
    );
    assert!(loader.calls.borrow().is_empty());
}

#[test]
fn restricted_tagged_loads_keep_refusals_typed_and_faults_tagged() {
    let policy = crate::ExecutionPolicy::Restricted(crate::RestrictedPolicy::new(
        crate::ExecutionBudgets::new(1_000, 64, 0),
    ));
    let loader = TaggedLoader::default();
    let run = |paths: &[Option<&str>]| {
        run_with_context(
            &dynamic_project(),
            &files(paths),
            &ExecutionContext::new(Path::new("mapping.ferrule.json"))
                .with_dynamic_source_loader(&loader)
                .with_policy(&policy),
        )
    };

    let refused = run(&[Some("first.xml"), Some("denied.xml"), Some("broken.xml")]).unwrap_err();
    assert_eq!(
        refused,
        EngineError::DynamicSourceRefused(Box::new(crate::DynamicSourceRefusal {
            source_name: "document".into(),
            path: "denied.xml".into(),
            refusal: crate::PolicyRefusal::HttpOriginDenied {
                origin: "https://denied.example".into(),
            },
        }))
    );
    // The refusal keeps its text and stays the typed next link in the chain.
    assert_eq!(
        refused.to_string(),
        "dynamic source `document`: restricted policy refuses HTTP origin `https://denied.example`"
    );
    assert!(
        refused
            .source()
            .and_then(|source| source.downcast_ref::<crate::PolicyRefusal>())
            .is_some()
    );
    // Generated runtimes see a confining host's refusal as an acquisition fault.
    assert_eq!(
        tokens(&refused),
        (
            "acquisition",
            "host",
            "document".into(),
            Some("denied.xml".into())
        )
    );
    assert_eq!(*loader.calls.borrow(), ["first.xml", "denied.xml"]);

    let fault = run(&[Some("broken.xml")]).unwrap_err();
    assert_eq!(tokens(&fault).0, "codec");
}

#[derive(Debug)]
struct Wrapped(EngineError);

impl fmt::Display for Wrapped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("stage failed")
    }
}

impl Error for Wrapped {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

#[test]
fn find_projects_an_engine_failure_beneath_host_errors() {
    let loader = TaggedLoader::default();
    let error = Wrapped(run(&loader, &files(&[Some("broken.xml")])).unwrap_err());
    let diagnostic = DynamicSourceDiagnostic::find(&error).expect("nested dynamic failure");
    assert_eq!(diagnostic.category.as_str(), "document");
    assert_eq!(diagnostic.path.as_deref(), Some("broken.xml"));
    assert_eq!(
        DynamicSourceDiagnostic::find(&Wrapped(EngineError::DebugCancelled)),
        None
    );
}
