use super::*;
use engine::DynamicSourceLoader;
use ir::{ScalarType, Value};
use mapping::{DynamicSourcePath, NamedSource};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_loader_requests_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Default)]
struct Requests(RefCell<Vec<(String, String)>>);

impl TraceSink for Requests {
    fn record(&self, _: TraceEvent) {}

    fn dynamic_source_load_requested(&self, source_name: &str, requested_path: &str) {
        self.0
            .borrow_mut()
            .push((source_name.into(), requested_path.into()));
    }
}

fn source(name: &str) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Document",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath::legacy(0, Vec::new())),
    }
}

fn expected(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

#[test]
fn loader_requests_include_cache_hits_before_acquisition() {
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("a.xml"),
        "<Document><Value>alpha</Value></Document>",
    )
    .unwrap();
    std::fs::write(
        dir.0.join("b.xml"),
        "<Document><Value>beta</Value></Document>",
    )
    .unwrap();
    let sources = [source("document")];
    let requests = Requests::default();
    let loader = ProjectDynamicSourceLoader::new(&dir.0, &sources).with_trace_sink(Some(&requests));
    let first = loader.load("document", "a.xml");
    eprintln!("R01 first={first:?} requests={:?}", requests.0.borrow());
    std::fs::remove_file(dir.0.join("a.xml")).unwrap();
    let cached = loader.load("document", "a.xml");
    eprintln!("R01 cached={cached:?} requests={:?}", requests.0.borrow());
    let other = loader.load("document", "b.xml");
    eprintln!("R01 other={other:?} requests={:?}", requests.0.borrow());
    let first = first.unwrap();
    let cached = cached.unwrap();
    assert!(Arc::ptr_eq(&first, &cached));
    assert_eq!(*first, expected("alpha"));
    assert_eq!(*other.unwrap(), expected("beta"));
    assert_eq!(
        *requests.0.borrow(),
        [
            ("document".into(), "a.xml".into()),
            ("document".into(), "a.xml".into()),
            ("document".into(), "b.xml".into()),
        ]
    );
}

#[test]
fn loader_requests_preserve_tuple_identity_and_raw_alias() {
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("a.xml"),
        "<Document><Value>alpha</Value></Document>",
    )
    .unwrap();
    let sources = [source("first"), source("second")];
    let requests = Requests::default();
    let loader = ProjectDynamicSourceLoader::new(&dir.0, &sources).with_trace_sink(Some(&requests));
    let first = loader.load("first", "a.xml");
    eprintln!("R02 first={first:?} requests={:?}", requests.0.borrow());
    let second = loader.load("second", "a.xml");
    eprintln!("R02 second={second:?} requests={:?}", requests.0.borrow());
    let alias = loader.load("first", "./a.xml");
    eprintln!("R02 alias={alias:?} requests={:?}", requests.0.borrow());
    let first = first.unwrap();
    let second = second.unwrap();
    let alias = alias.unwrap();
    assert_eq!(*first, expected("alpha"));
    assert_eq!(*first, *second);
    assert_eq!(*first, *alias);
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&first, &alias));
    assert_eq!(loader.cache.borrow().len(), 3);
    assert_eq!(
        *requests.0.borrow(),
        [
            ("first".into(), "a.xml".into()),
            ("second".into(), "a.xml".into()),
            ("first".into(), "./a.xml".into()),
        ]
    );
}

#[test]
fn loader_requests_include_failures_without_caching_them() {
    let dir = TestDir::new();
    let sources = [source("document")];
    let requests = Requests::default();
    let loader = ProjectDynamicSourceLoader::new(&dir.0, &sources).with_trace_sink(Some(&requests));
    let first = loader.load("document", "missing.xml");
    eprintln!(
        "R03 first={first:?} requests={:?} cache={:?}",
        requests.0.borrow(),
        loader.cache.borrow()
    );
    let second = loader.load("document", "missing.xml");
    eprintln!(
        "R03 second={second:?} requests={:?} cache={:?}",
        requests.0.borrow(),
        loader.cache.borrow()
    );
    let cache_after_failures = loader.cache.borrow().len();
    std::fs::write(
        dir.0.join("missing.xml"),
        "<Document><Value>repaired</Value></Document>",
    )
    .unwrap();
    let repaired = loader.load("document", "missing.xml");
    eprintln!(
        "R03 repaired={repaired:?} requests={:?}",
        requests.0.borrow()
    );
    let unknown = loader.load("unknown", "missing.xml");
    eprintln!("R03 unknown={unknown:?} requests={:?}", requests.0.borrow());
    assert!(first.is_err());
    assert!(second.is_err());
    assert_eq!(cache_after_failures, 0);
    assert_eq!(*repaired.unwrap(), expected("repaired"));
    assert_eq!(
        unknown.unwrap_err(),
        "dynamic source `unknown` is not declared"
    );
    assert_eq!(loader.cache.borrow().len(), 1);
    assert_eq!(
        *requests.0.borrow(),
        [
            ("document".into(), "missing.xml".into()),
            ("document".into(), "missing.xml".into()),
            ("document".into(), "missing.xml".into()),
            ("unknown".into(), "missing.xml".into()),
        ]
    );
}

#[test]
fn legacy_sink_default_and_absent_sink_preserve_loader_results() {
    struct Legacy;
    impl TraceSink for Legacy {
        fn record(&self, _: TraceEvent) {}
    }
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("a.xml"),
        "<Document><Value>alpha</Value></Document>",
    )
    .unwrap();
    let sources = [source("document")];
    let legacy = Legacy;
    let absent = ProjectDynamicSourceLoader::new(&dir.0, &sources);
    let observed = ProjectDynamicSourceLoader::new(&dir.0, &sources).with_trace_sink(Some(&legacy));
    let absent_result = absent.load("document", "a.xml");
    let legacy_result = observed.load("document", "a.xml");
    eprintln!("R06 absent={absent_result:?} legacy={legacy_result:?}");
    assert!(absent.trace_sink.is_none());
    assert_eq!(*absent_result.unwrap(), expected("alpha"));
    assert_eq!(*legacy_result.unwrap(), expected("alpha"));
}

#[test]
fn ordinary_pdf_loader_observation_preserves_exact_instance() {
    use mapping::{PdfCapture, PdfCommand, PdfLayout, PdfPageSelection, PdfRegion};
    let dir = TestDir::new();
    std::fs::write(
        dir.0.join("hello.pdf"),
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    )
    .unwrap();
    let layout = PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap();
    let sources = [NamedSource {
        schema: layout.schema(),
        options: mapping::FormatOptions {
            pdf: Some(layout),
            ..Default::default()
        },
        ..source("document")
    }];
    let requests = Requests::default();
    let plain = ProjectDynamicSourceLoader::new(&dir.0, &sources);
    let observed =
        ProjectDynamicSourceLoader::new(&dir.0, &sources).with_trace_sink(Some(&requests));
    let plain_result = plain.load("document", "hello.pdf");
    let observed_result = observed.load("document", "hello.pdf");
    eprintln!(
        "R04 ordinary_pdf plain={plain_result:?} observed={observed_result:?} requests={:?}",
        requests.0.borrow()
    );
    assert_eq!(*plain_result.unwrap(), expected("Hello"));
    assert_eq!(*observed_result.unwrap(), expected("Hello"));
    assert_eq!(
        *requests.0.borrow(),
        [("document".into(), "hello.pdf".into())]
    );
}
