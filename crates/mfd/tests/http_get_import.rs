use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_http_export_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> &'a Value {
    instance.field(field).and_then(Instance::as_scalar).unwrap()
}

#[test]
fn imports_static_get_xml_and_executes_with_a_captured_response() {
    let imported = mfd::import(&fixture("http-get.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("http://127.0.0.1:9/feed")
    );
    assert_eq!(
        imported
            .project
            .source_options
            .http_get
            .map(|options| options.timeout_seconds().get()),
        Some(40)
    );
    assert!(imported.project.source_options.xml_document);
    assert!(imported.project.source.child("Item").unwrap().repeating);

    let source = format_xml::read(&fixture("http-feed.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        scalar(&target, "Heading"),
        &Value::String("Daily results".into())
    );
    let rows = target.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(scalar(&rows[0], "Name"), &Value::String("Alpha".into()));
    assert_eq!(scalar(&rows[1], "Score"), &Value::Int(11));
}

#[test]
fn dtd_described_http_document_copy_preserves_present_choice_branches() {
    let imported = mfd::import(&fixture("http-copy.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let source = format_xml::read(&fixture("http-copy.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(target, source);

    let xml = format_xml::to_string(&imported.project.target, &target).unwrap();
    assert_eq!(xml.matches("<Category").count(), 2, "{xml}");
    assert_eq!(xml.matches("<Open>").count(), 1, "{xml}");
    assert_eq!(xml.matches("<Closed>").count(), 1, "{xml}");
    assert!(xml.contains("<Item>One</Item>"), "{xml}");
}

#[test]
fn static_get_xml_export_generates_a_canonical_response_component() {
    let imported = mfd::import(&fixture("http-get.mfd")).unwrap();
    let dir = TempDir::new();
    let design = dir.0.join("feed.mfd");

    let warnings = mfd::export(&imported.project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains(r#"library="webservice""#), "{text}");
    assert!(text.contains(r#"kind="20""#), "{text}");
    assert!(text.contains(r#"sourceMode="manual""#), "{text}");
    assert!(text.contains(r#"httpmethod="GET""#), "{text}");
    assert!(text.contains(r#"timeout="40""#), "{text}");
    assert!(text.contains(r#"url="http://127.0.0.1:9/feed""#), "{text}");
    assert!(text.contains(r#"type="doc-xml""#), "{text}");
    assert!(dir.0.join("feed-source.xsd").is_file());

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.source, imported.project.source);
    assert_eq!(
        reimported
            .project
            .source_options
            .http_get
            .map(|options| options.timeout_seconds().get()),
        Some(40)
    );
    assert!(reimported.project.source_options.xml_document);
    assert_eq!(reimported.project.source_path, imported.project.source_path);

    let source = format_xml::read(&fixture("http-feed.xml"), &reimported.project.source).unwrap();
    let expected = engine::run(&imported.project, &source).unwrap();
    assert_eq!(engine::run(&reimported.project, &source).unwrap(), expected);
}

#[test]
fn whole_http_document_copy_export_roundtrips_as_a_structural_edge() {
    let imported = mfd::import(&fixture("http-copy.mfd")).unwrap();
    let dir = TempDir::new();
    let design = dir.0.join("copy.mfd");

    let warnings = mfd::export(&imported.project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = std::fs::read_to_string(&design).unwrap();
    assert!(text.contains(r#"<dataconnection type="2"/>"#), "{text}");

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.project.root.construction,
        mapping::ScopeConstruction::CopyCurrentSource
    );
    let source = format_xml::read(&fixture("http-copy.xml"), &reimported.project.source).unwrap();
    assert_eq!(engine::run(&reimported.project, &source).unwrap(), source);
}

#[test]
fn http_transport_on_a_target_is_rejected_without_artifacts() {
    let mut project = mfd::import(&fixture("http-get.mfd")).unwrap().project;
    project.target_path = Some("https://example.invalid/result".to_string());
    project.target_options.http_get = project.source_options.http_get;
    let dir = TempDir::new();
    let design = dir.0.join("invalid.mfd");

    assert!(matches!(
        mfd::export(&project, &design),
        Err(mfd::MfdError::Unsupported(message)) if message.contains("only for mapping sources")
    ));
    assert!(!design.exists());
    assert!(!dir.0.join("invalid-source.xsd").exists());
    assert!(!dir.0.join("invalid-target.xsd").exists());
}
