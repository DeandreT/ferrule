use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Node, Scope};

use super::super::{CanvasDocumentState, SaveContinuation};
use super::*;
use crate::document_file::fault::{Fault, arm};
use crate::layout_store::layout_path;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-document-save-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mapped_app() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    app.project.source_path = Some("source.xml".into());
    app.project.target_path = Some("target.xml".into());
    app.project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app
}

fn key(project: &mapping::Project) -> String {
    crate::project_state::project_snapshot_key(project)
}

fn edit(app: &mut FerruleApp, value: &str) {
    app.project.graph.nodes.insert(
        9,
        Node::Const {
            value: ir::Value::String(value.into()),
        },
    );
}

/// Saves a baseline and returns the published project and sidecar bytes.
fn saved_baseline(app: &mut FerruleApp, path: &Path) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    app.save_document_to(path)?;
    assert!(!app.is_dirty());
    Ok((std::fs::read(path)?, std::fs::read(layout_path(path))?))
}

#[test]
fn injected_write_failures_keep_saved_bytes_association_and_dirty_state() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let path = temp.0.join("project.json");
    let mut app = mapped_app();
    let (project_bytes, layout_bytes) = saved_baseline(&mut app, &path)?;
    edit(&mut app, "unsaved");
    let edited = app.project.clone();

    for fault in [Fault::PartialWrite, Fault::BeforePublish] {
        arm(&path, fault);
        app.save_with_continuation(None, &egui::Context::default());

        assert_eq!(app.status, format!("failed to save {}", path.display()));
        assert!(
            app.diagnostics
                .items()
                .iter()
                .any(|item| item.message.contains("injected")),
            "{:?}",
            app.diagnostics.items()
        );
        assert_eq!(std::fs::read(&path)?, project_bytes);
        assert_eq!(std::fs::read(layout_path(&path))?, layout_bytes);
        assert_eq!(app.document, DocumentLocation::saved(path.clone()));
        assert_eq!(key(&app.project), key(&edited));
        assert!(app.is_dirty());
    }
    let mut names = std::fs::read_dir(&temp.0)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    assert_eq!(names, ["project.json", "project.layout.json"]);

    app.save_with_continuation(None, &egui::Context::default());
    assert_eq!(app.status, format!("saved {}", path.display()));
    assert!(!app.is_dirty());
    assert_eq!(
        key(&mapping::project_file::decode_bytes(&std::fs::read(
            &path
        )?)?),
        key(&edited)
    );
    Ok(())
}

#[test]
fn failed_save_as_publication_keeps_the_previous_document_basis() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let original = temp.0.join("maps/project.json");
    let moved = temp.0.join("elsewhere/nested/project.json");
    std::fs::create_dir_all(original.parent().expect("project has a parent"))?;
    std::fs::create_dir_all(moved.parent().expect("project has a parent"))?;
    let mut app = mapped_app();
    saved_baseline(&mut app, &original)?;
    edit(&mut app, "unsaved");
    let edited = app.project.clone();

    arm(&moved, Fault::BeforePublish);
    assert!(app.save_document_to(&moved).is_err());

    assert!(!moved.exists());
    assert!(!layout_path(&moved).exists());
    assert_eq!(app.document, DocumentLocation::saved(original.clone()));
    assert_eq!(
        key(&app.project),
        key(&edited),
        "paths are not rebased before publication"
    );
    assert!(app.is_dirty());
    Ok(())
}

#[test]
fn failed_save_blocks_the_saved_run_continuation() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let path = temp.0.join("project.json");
    let mut app = mapped_app();
    let (project_bytes, _) = saved_baseline(&mut app, &path)?;
    edit(&mut app, "unsaved");

    arm(&path, Fault::PartialWrite);
    app.run(&egui::Context::default());

    assert_eq!(app.status, format!("failed to save {}", path.display()));
    assert!(app.pending_file_run.is_none());
    assert_eq!(std::fs::read(&path)?, project_bytes);
    assert!(app.is_dirty());
    Ok(())
}

#[test]
fn sidecar_failure_after_publication_stays_recoverable() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let path = temp.0.join("project.json");
    let mut app = mapped_app();
    let (_, layout_bytes) = saved_baseline(&mut app, &path)?;
    edit(&mut app, "published");

    arm(&layout_path(&path), Fault::PartialWrite);
    app.save_with_continuation(
        Some(SaveContinuation::Destructive(
            super::super::DestructiveAction::NewProject,
        )),
        &egui::Context::default(),
    );

    // The semantic save published and its continuation ran; the previous
    // sidecar remains complete and no longer matches the published project.
    assert!(app.document.saved_path().is_none());
    let published = mapping::project_file::decode_bytes(&std::fs::read(&path)?)?;
    assert!(published.graph.nodes.contains_key(&9));
    assert_eq!(std::fs::read(layout_path(&path))?, layout_bytes);
    let stale = crate::layout_store::read_layout(&path)?.expect("previous sidecar is intact");
    assert!(!stale.matches_project(&published));

    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    assert_eq!(key(&reopened.project), key(&published));
    assert!(!reopened.is_dirty());

    // Saving again completes the sidecar for the published project.
    reopened.save_with_continuation(None, &egui::Context::default());
    assert_eq!(reopened.status, format!("saved {}", path.display()));
    let layout = crate::layout_store::read_layout(&path)?.expect("sidecar is rewritten");
    assert!(layout.matches_project(&published));
    Ok(())
}

#[test]
fn sidecar_failure_is_reported_on_the_saved_document() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let path = temp.0.join("project.json");
    let mut app = mapped_app();
    saved_baseline(&mut app, &path)?;
    edit(&mut app, "published");

    arm(&layout_path(&path), Fault::BeforePublish);
    app.save_with_continuation(None, &egui::Context::default());

    assert_eq!(
        app.status,
        format!("saved {} without canvas layout", path.display())
    );
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("canvas layout was not saved")),
        "{:?}",
        app.diagnostics.items()
    );
    assert_eq!(app.document, DocumentLocation::saved(path.clone()));
    assert!(!app.is_dirty());
    Ok(())
}
