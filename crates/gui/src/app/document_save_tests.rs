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
    assert_eq!(app.status, "saved project.json");
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
    assert_eq!(reopened.status, "saved project.json");
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

    assert_eq!(app.status, "saved project.json without canvas layout");
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

fn relocatable_app() -> FerruleApp {
    let mut app = mapped_app();
    app.project.source_path = Some("../data/source.xml".into());
    app.project.target_path = Some("../results/target.xml".into());
    app.project.extra_sources.push(mapping::NamedSource {
        name: "Catalog".into(),
        path: "../data/catalog.xml".into(),
        schema: SchemaNode::group(
            "Catalog",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "Audit".into(),
        path: Some("../results/audit.xml".into()),
        schema: SchemaNode::group(
            "Audit",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        options: Default::default(),
        root: Scope::default(),
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app
}

fn boundary_paths(project: &mapping::Project) -> [Option<String>; 4] {
    [
        project.source_path.clone(),
        project.target_path.clone(),
        project
            .extra_sources
            .first()
            .map(|source| source.path.clone()),
        project
            .extra_targets
            .first()
            .and_then(|target| target.path.clone()),
    ]
}

fn paths(values: [&str; 4]) -> [Option<String>; 4] {
    values.map(|value| Some(value.to_string()))
}

/// A saved project in `maps/` whose history holds an edit of every boundary.
fn edited_project(temp: &TestDir) -> anyhow::Result<FerruleApp> {
    let original = temp.0.join("maps/project.json");
    std::fs::create_dir_all(temp.0.join("maps"))?;
    let mut app = relocatable_app();
    app.document = DocumentLocation::saved(original.clone());
    app.rebase_history();
    app.save_document_to(&original)?;
    let start = std::time::Instant::now();
    app.project.source_path = Some("../data/other.xml".into());
    app.project.extra_targets[0].path = Some("../results/other-audit.xml".into());
    app.observe_editor_history(start, false);
    app.project.target_path = Some("../results/other.xml".into());
    app.observe_editor_history(start, false);
    assert_eq!(app.history.undo_len(), 2);
    Ok(app)
}

/// Applies one Undo or Redo and checks the restored boundary identities.
fn step(app: &mut FerruleApp, undo: bool, expected: [&str; 4], dirty: bool) {
    if undo {
        app.undo_project();
    } else {
        app.redo_project();
    }
    assert_eq!(boundary_paths(&app.project), paths(expected));
    assert_eq!(app.is_dirty(), dirty);
}

const MOVED_INITIAL: [&str; 4] = [
    "../../data/source.xml",
    "../../results/target.xml",
    "../../data/catalog.xml",
    "../../results/audit.xml",
];
const MOVED_FIRST_EDIT: [&str; 4] = [
    "../../data/other.xml",
    "../../results/target.xml",
    "../../data/catalog.xml",
    "../../results/other-audit.xml",
];

/// Saves into `exports/deep/`, two levels below the original `maps/` basis.
fn save_moved(app: &mut FerruleApp, temp: &TestDir, saved: [&str; 4]) -> anyhow::Result<()> {
    let moved = temp.0.join("exports/deep/project.json");
    std::fs::create_dir_all(temp.0.join("exports/deep"))?;
    app.save_document_to(&moved)?;
    assert_eq!(app.document, DocumentLocation::saved(moved.clone()));
    let stored = mapping::project_file::decode_bytes(&std::fs::read(&moved)?)?;
    assert_eq!(boundary_paths(&stored), paths(saved));
    assert_eq!(boundary_paths(&app.project), paths(saved));
    assert!(!app.is_dirty());
    Ok(())
}

#[test]
fn cross_directory_save_as_rebases_undo_and_redo_snapshots() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = edited_project(&temp)?;
    app.undo_project();
    assert!(app.history.can_redo());
    save_moved(&mut app, &temp, MOVED_FIRST_EDIT)?;

    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(
        app.history.undo_len(),
        1,
        "saving is not recorded as an edit"
    );
    let second_edit = [
        "../../data/other.xml",
        "../../results/other.xml",
        "../../data/catalog.xml",
        "../../results/other-audit.xml",
    ];
    for _ in 0..2 {
        step(&mut app, true, MOVED_INITIAL, true);
        step(&mut app, false, MOVED_FIRST_EDIT, false);
        step(&mut app, false, second_edit, true);
        step(&mut app, true, MOVED_FIRST_EDIT, false);
    }
    assert_eq!(
        app.document,
        DocumentLocation::saved(temp.0.join("exports/deep/project.json"))
    );
    Ok(())
}

#[test]
fn cross_directory_save_as_rebases_a_pending_coalesced_edit() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = edited_project(&temp)?;
    app.undo_project();
    let start = std::time::Instant::now();
    app.project.extra_sources[0].path = "../data/pending.xml".into();
    app.observe_editor_history(start, true);
    assert!(app.pending_history.is_some());
    let pending = [
        "../../data/other.xml",
        "../../results/target.xml",
        "../../data/pending.xml",
        "../../results/other-audit.xml",
    ];
    save_moved(&mut app, &temp, pending)?;

    assert!(app.pending_history.is_some());
    app.observe_editor_history(start + crate::app::HISTORY_COALESCE_DELAY, true);
    assert_eq!(app.history.undo_len(), 2);
    for _ in 0..2 {
        step(&mut app, true, MOVED_FIRST_EDIT, true);
        step(&mut app, true, MOVED_INITIAL, true);
        step(&mut app, false, MOVED_FIRST_EDIT, true);
        step(&mut app, false, pending, false);
    }
    Ok(())
}

#[test]
fn first_save_rebases_history_from_the_untitled_basis() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = relocatable_app();
    app.document = DocumentLocation::untitled(temp.0.join("imports/mapping.json"));
    app.rebase_history();
    app.project.source_path = Some("../data/edited.xml".into());
    app.observe_editor_history(std::time::Instant::now(), false);

    let saved = temp.0.join("maps/nested/project.json");
    std::fs::create_dir_all(temp.0.join("maps/nested"))?;
    app.save_document_to(&saved)?;

    let mut edited = MOVED_INITIAL;
    edited[0] = "../../data/edited.xml";
    assert_eq!(boundary_paths(&app.project), paths(edited));
    step(&mut app, true, MOVED_INITIAL, true);
    step(&mut app, false, edited, false);
    Ok(())
}

#[test]
fn same_directory_save_as_keeps_history_with_unchanged_meaning() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = edited_project(&temp)?;
    // Spelling normalized by the save is not recorded as a separate edit.
    app.project.extra_sources[0].path = r".\..\data\spelled.xml".into();
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.history.undo_len(), 3);

    let renamed = temp.0.join("maps/renamed.json");
    app.save_document_to(&renamed)?;
    assert_eq!(app.history.undo_len(), 3);
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.history.undo_len(), 3);
    assert_eq!(app.project.extra_sources[0].path, "../data/spelled.xml");
    assert!(!app.is_dirty());

    for _ in 0..2 {
        app.undo_project();
        app.undo_project();
        app.undo_project();
        assert_eq!(
            boundary_paths(&app.project),
            paths([
                "../data/source.xml",
                "../results/target.xml",
                "../data/catalog.xml",
                "../results/audit.xml",
            ])
        );
        assert!(app.is_dirty());
        app.redo_project();
        app.redo_project();
        app.redo_project();
        assert_eq!(
            boundary_paths(&app.project),
            paths([
                "../data/other.xml",
                "../results/other.xml",
                "../data/spelled.xml",
                "../results/other-audit.xml",
            ])
        );
        assert!(!app.is_dirty());
    }
    assert_eq!(app.document, DocumentLocation::saved(renamed));
    Ok(())
}

#[test]
fn ordinary_save_keeps_pending_and_recorded_history() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = edited_project(&temp)?;
    let start = std::time::Instant::now();
    app.project.extra_sources[0].path = "../data/typed.xml".into();
    app.observe_editor_history(start, true);

    app.save_with_continuation(None, &egui::Context::default());
    assert!(!app.is_dirty());
    app.observe_editor_history(start + crate::app::HISTORY_COALESCE_DELAY, true);
    assert_eq!(app.history.undo_len(), 3);
    app.undo_project();
    assert_eq!(app.project.extra_sources[0].path, "../data/catalog.xml");
    assert!(app.is_dirty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_save_as_keeps_static_resources_through_history_and_reopen() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    for directory in [
        "data",
        "results",
        "storage/deep",
        "storage/data",
        "storage/results",
    ] {
        std::fs::create_dir_all(temp.0.join(directory))?;
    }
    for (directory, names) in [
        ("data", &["source.xml", "other.xml", "catalog.xml"][..]),
        (
            "results",
            &["target.xml", "other.xml", "audit.xml", "other-audit.xml"][..],
        ),
    ] {
        for name in names {
            std::fs::write(temp.0.join(directory).join(name), b"intended resource")?;
            std::fs::write(
                temp.0.join("storage").join(directory).join(name),
                b"wrong resource",
            )?;
        }
    }
    let linked = temp.0.join("shortcut[archive]");
    std::os::unix::fs::symlink(temp.0.join("storage/deep"), &linked)?;
    let mut app = edited_project(&temp)?;
    app.undo_project();
    let moved = linked.join("project.json");
    app.save_document_to(&moved)?;
    for _ in 0..2 {
        for path in boundary_paths(&app.project).into_iter().flatten() {
            assert_eq!(std::fs::read(linked.join(path))?, b"intended resource");
        }
        step(&mut app, true, MOVED_INITIAL, true);
        step(&mut app, false, MOVED_FIRST_EDIT, false);
        step(
            &mut app,
            false,
            [
                "../../data/other.xml",
                "../../results/other.xml",
                "../../data/catalog.xml",
                "../../results/other-audit.xml",
            ],
            true,
        );
        step(&mut app, true, MOVED_FIRST_EDIT, false);
    }
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&moved);
    assert_eq!(key(&reopened.project), key(&app.project));
    for path in boundary_paths(&reopened.project).into_iter().flatten() {
        assert_eq!(std::fs::read(linked.join(path))?, b"intended resource");
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn unresolved_resource_symlink_does_not_publish_a_rebased_document() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let original = temp.0.join("maps/project.json");
    let destination = temp.0.join("saved/project.json");
    std::fs::create_dir_all(original.parent().unwrap())?;
    std::fs::create_dir_all(destination.parent().unwrap())?;
    let mut app = mapped_app();
    app.document = DocumentLocation::saved(original.clone());
    saved_baseline(&mut app, &original)?;
    std::os::unix::fs::symlink(temp.0.join("missing"), temp.0.join("maps/broken"))?;
    app.project.source_path = Some("broken/../source.xml".into());
    app.observe_editor_history(std::time::Instant::now(), true);
    let undo_len = app.history.undo_len();
    let can_redo = app.history.can_redo();
    let pending = app
        .pending_history
        .as_ref()
        .map(|pending| key(&pending.before.project));
    let before = key(&app.project);
    std::fs::write(&destination, b"existing destination")?;
    assert!(app.save_document_to(&destination).is_err());
    assert_eq!(std::fs::read(&destination)?, b"existing destination");
    assert!(!layout_path(&destination).exists());
    assert_eq!(app.document, DocumentLocation::saved(original));
    assert_eq!(key(&app.project), before);
    assert!(app.is_dirty());
    assert_eq!(app.history.undo_len(), undo_len);
    assert_eq!(app.history.can_redo(), can_redo);
    assert_eq!(
        app.pending_history
            .as_ref()
            .map(|pending| key(&pending.before.project)),
        pending
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn save_as_discards_history_that_cannot_retain_its_resource_identity() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let original = temp.0.join("maps/project.json");
    let destination = temp.0.join("saved/project.json");
    std::fs::create_dir_all(original.parent().unwrap())?;
    std::fs::create_dir_all(destination.parent().unwrap())?;
    let mut app = mapped_app();
    app.document = DocumentLocation::saved(original.clone());
    app.rebase_history();
    saved_baseline(&mut app, &original)?;
    std::os::unix::fs::symlink(temp.0.join("missing"), temp.0.join("maps/broken"))?;
    let start = std::time::Instant::now();
    app.project.source_path = Some("broken/../source.xml".into());
    app.observe_editor_history(start, false);
    app.project.source_path = Some("source.xml".into());
    app.observe_editor_history(start, false);
    assert!(app.history.can_undo());
    app.save_document_to(&destination)?;
    assert!(!app.history.can_undo());
    assert!(!app.history.can_redo());
    assert!(app.pending_history.is_none());
    assert_eq!(app.document, DocumentLocation::saved(destination.clone()));
    let saved = mapping::project_file::decode_bytes(&std::fs::read(destination)?)?;
    assert_eq!(saved.source_path.as_deref(), Some("../maps/source.xml"));
    Ok(())
}
