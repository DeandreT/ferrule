//! Document projects open with their filename plan, save it back, and Run and
//! Preview read documents only beneath the explicit input root.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

use super::FerruleApp;
use super::host_parameters::HostParameterEntry;

#[path = "primary_edi_document_tests.rs"]
mod edi;
#[path = "primary_fixed_width_document_tests.rs"]
mod fixed;
#[path = "primary_json_document_tests.rs"]
mod json;
#[path = "../../../../conformance/primary-filename-documents/projects.rs"]
mod projects;
#[path = "../../../../conformance/primary-xml-documents/projects.rs"]
mod xml_projects;

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-primary-document-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The cross-document grouping case (P04) beside a saved wildcard project.
fn wildcard_project(directory: &Path) -> anyhow::Result<PathBuf> {
    let documents = directory.join("documents/in");
    std::fs::create_dir_all(&documents)?;
    std::fs::write(documents.join("a.csv"), "Key,Text,Count\nk1,c,3\n")?;
    std::fs::write(documents.join("B.csv"), "Key,Text,Count\nk2,b,20\nk1,a,1\n")?;
    std::fs::write(documents.join("é.csv"), "Key,Text,Count\nk3,e,5\nk2,f,7\n")?;
    // The saved source path names a decoy that a document run never reads.
    std::fs::write(
        directory.join("never-preview.csv"),
        "Key,Text,Count\nzz,no,0\n",
    )?;
    let path = directory.join("project.json");
    std::fs::write(
        &path,
        mapping::document_project_file::encode_pretty(&projects::document("parameter_wildcard"))?,
    )?;
    Ok(path)
}

const GROUPED: &str = "Key,Text,Count,Status\nk2,b+f,27,ok\nk1,a+c,4,ok\nk3,e,5,ok\n";

fn run_values(app: &mut FerruleApp) {
    for (name, value) in [("Input", "in/*.csv"), ("Status", "ok")] {
        app.host_parameters.entries.push(HostParameterEntry {
            name: name.into(),
            value: value.into(),
        });
    }
}

#[test]
fn document_projects_reopen_and_save_with_their_filename_plan() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let path = wildcard_project(&directory.0)?;
    let mut app = FerruleApp::default();
    app.load_project_from(&path);
    let session = app.primary_document.as_ref().expect("document session");
    let saved = projects::document("parameter_wildcard");
    assert_eq!(&session.plan, saved.primary_source());
    assert_eq!(Path::new(&session.input_root), directory.0);
    assert_eq!(
        mapping::project_file::encode_pretty(&app.project)?,
        mapping::project_file::encode_pretty(saved.project())?
    );

    let plans = projects::MODELS
        .iter()
        .map(|model| (format!("csv-{model}"), projects::document(model)))
        .chain(
            xml_projects::MODELS
                .iter()
                .map(|model| (format!("xml-{model}"), xml_projects::document(model))),
        );
    for (model, saved) in plans {
        let path = directory.0.join(format!("{model}.json"));
        std::fs::write(
            &path,
            mapping::document_project_file::encode_pretty(&saved)?,
        )?;
        app.load_project_from(&path);
        let copy = directory.0.join(format!("{model}-copy.json"));
        app.save_document_to(&copy)?;
        let reopened = mapping::document_project_file::decode_bytes(&std::fs::read(&copy)?)?;
        assert_eq!(reopened.primary_source(), saved.primary_source(), "{model}");
        assert!(mapping::project_file::decode_bytes(&std::fs::read(&copy)?).is_err());
    }

    // An ordinary project closes the document session.
    let ordinary = directory.0.join("ordinary.json");
    std::fs::write(
        &ordinary,
        mapping::project_file::encode_pretty(saved.project())?,
    )?;
    app.load_project_from(&ordinary);
    assert!(app.primary_document.is_none());
    app.save_document_to(&ordinary)?;
    assert!(mapping::project_file::decode_bytes(&std::fs::read(&ordinary)?).is_ok());
    Ok(())
}

#[test]
fn document_runs_and_previews_read_only_beneath_the_input_root() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let path = wildcard_project(&directory.0)?;
    let mut app = FerruleApp::default();
    app.load_project_from(&path);
    run_values(&mut app);
    let session = app.primary_document.as_mut().expect("document session");
    session.input_root = directory.0.join("documents").display().to_string();
    let context = egui::Context::default();

    // Preview publishes nothing and reports the complete grouped document.
    app.begin_preview();
    app.execute_preview();
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_preview.is_some() && Instant::now() < deadline {
        app.poll_preview(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_preview.is_none(), "{}", app.status);
    let output = directory.0.join("never-preview-output.csv");
    assert!(!output.exists());
    let report = app.run_report.as_mut().expect("preview report");
    assert_eq!(report.report.outputs.len(), 1);
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("CSV preview");
    };
    assert_eq!(content.as_str(), GROUPED);

    // Debug preview and Debug Run are refused for document projects.
    app.begin_preview();
    app.execute_debug_preview();
    assert!(app.pending_preview.is_none());
    assert_eq!(app.status, "preview blocked");
    app.preview_draft = None;

    // Run publishes the same document to the saved target path.
    app.run_saved(false);
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_file_run.is_some() && Instant::now() < deadline {
        app.poll_file_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_file_run.is_none(), "{}", app.status);
    assert_eq!(std::fs::read_to_string(&output)?, GROUPED, "{}", app.status);

    // An empty root is refused before anything is read or written.
    std::fs::remove_file(&output)?;
    app.primary_document
        .as_mut()
        .expect("document session")
        .input_root
        .clear();
    app.run_saved(false);
    assert!(app.pending_file_run.is_none());
    assert_eq!(app.status, "run blocked");
    assert!(!output.exists());
    Ok(())
}

#[test]
fn document_failures_are_reported_and_publish_nothing() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let path = wildcard_project(&directory.0)?;
    let mut app = FerruleApp::default();
    app.load_project_from(&path);
    app.host_parameters.entries.push(HostParameterEntry {
        name: "Input".into(),
        value: "in/*.json".into(),
    });
    let session = app.primary_document.as_mut().expect("document session");
    session.input_root = directory.0.join("documents").display().to_string();
    app.run_saved(false);
    let context = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_file_run.is_some() && Instant::now() < deadline {
        app.poll_file_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_file_run.is_none());
    assert!(!directory.0.join("never-preview-output.csv").exists());
    let messages = app
        .diagnostics
        .items()
        .iter()
        .map(|item| format!("{item:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(messages.contains("matched no files"), "{messages}");
    Ok(())
}

fn orders(rows: &[(&str, &str, i64)]) -> String {
    let orders: String = rows
        .iter()
        .map(|(key, text, count)| {
            format!("<Order><Key>{key}</Key><Text>{text}</Text><Count>{count}</Count></Order>")
        })
        .collect();
    format!("<Orders>{orders}</Orders>")
}

#[test]
fn xml_document_projects_run_and_preview_one_document_set() -> anyhow::Result<()> {
    let directory = TestDir::new()?;
    let documents = directory.0.join("documents/in");
    std::fs::create_dir_all(&documents)?;
    std::fs::write(documents.join("a.xml"), orders(&[("k1", "c", 3)]))?;
    std::fs::write(
        documents.join("B.xml"),
        orders(&[("k2", "b", 20), ("k1", "a", 1)]),
    )?;
    let path = directory.0.join("orders.json");
    std::fs::write(
        &path,
        mapping::document_project_file::encode_pretty(&xml_projects::document(
            "parameter_wildcard",
        ))?,
    )?;
    let mut app = FerruleApp::default();
    app.load_project_from(&path);
    for (name, value) in [("Input", "in/*.xml"), ("Status", "ok")] {
        app.host_parameters.entries.push(HostParameterEntry {
            name: name.into(),
            value: value.into(),
        });
    }
    app.primary_document
        .as_mut()
        .expect("document session")
        .input_root = directory.0.join("documents").display().to_string();
    let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Summary>\n  <Group>\n    <Key>k2</Key>\n    <Text>b</Text>\n    <Count>20</Count>\n    <Status>ok</Status>\n  </Group>\n  <Group>\n    <Key>k1</Key>\n    <Text>a+c</Text>\n    <Count>4</Count>\n    <Status>ok</Status>\n  </Group>\n</Summary>";
    let context = egui::Context::default();
    app.begin_preview();
    app.execute_preview();
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_preview.is_some() && Instant::now() < deadline {
        app.poll_preview(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_preview.is_none(), "{}", app.status);
    let report = app.run_report.as_mut().expect("preview report");
    let crate::run_report::OutputPreview::Text { content, .. } = report.report.outputs[0].preview()
    else {
        panic!("XML preview");
    };
    assert_eq!(content.as_str(), expected);
    let output = directory.0.join("never-preview-output.xml");
    assert!(!output.exists());
    app.run_saved(false);
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.pending_file_run.is_some() && Instant::now() < deadline {
        app.poll_file_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.pending_file_run.is_none(), "{}", app.status);
    assert_eq!(
        std::fs::read_to_string(&output)?,
        expected,
        "{}",
        app.status
    );
    Ok(())
}
