use super::*;
use crate::app::{CanvasDocumentState, MappingDocument};
use crate::diagnostics::Diagnostic;
use engine::ValidationEndpoint;
use mapping::{NamedTarget, ScopeIteration};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../cli/tests/fixtures/orders")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ferrule-repair-{}-{index}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn intact() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.load_project_from(&fixture().join("project.json"));
    assert!(app.diagnostics.is_empty());
    app
}

fn damage(app: &mut FerruleApp) {
    app.project.graph.nodes.remove(&4).unwrap();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.observe_editor_history(Instant::now(), false);
    app.validate_now();
}

fn issue(app: &FerruleApp, endpoint: &ValidationEndpoint) -> Diagnostic {
    app.diagnostics
        .items()
        .iter()
        .find(|item| {
            item.broken_binding
                .as_ref()
                .is_some_and(|b| &b.owner.target == endpoint && b.field == "Desc" && b.missing == 4)
        })
        .unwrap()
        .clone()
}

fn inspect(app: &mut FerruleApp) {
    app.validate_now();
    assert!(app.navigate_to_diagnostic(&issue(app, &ValidationEndpoint::Target)));
    assert!(app.connection_repair.is_some());
}

#[test]
fn editor_workflow_previews_the_repaired_saved_copy_then_undo_restores_its_diagnostic() {
    use crate::app::tests::wait_for_preview_completion;
    use crate::preview_table::PreviewTable;
    use crate::run_report::OutputPreview;

    let scratch = Scratch::new();
    let path = scratch.0.join("orders.json");
    let output = scratch.0.join("order-lines.csv");
    std::fs::write(&path, cli::orders_example::PROJECT).unwrap();
    let mut app = FerruleApp::default();
    app.load_project_from(&path);
    damage(&mut app);
    let problem = issue(&app, &ValidationEndpoint::Target);
    let undo = app.history.undo_len();
    assert!(app.navigate_to_diagnostic(&problem));
    app.apply_connection_repair();
    app.validate_now();
    assert!(app.diagnostics.is_empty());
    assert_eq!(app.history.undo_len(), undo + 1);
    app.begin_preview();
    let draft = app.preview_draft.as_mut().unwrap();
    draft.input_identity = "Orders.xml".into();
    draft.output_identity = output.to_str().unwrap().to_owned();
    draft.input_text = cli::orders_example::INPUT.to_owned();
    app.execute_preview();
    wait_for_preview_completion(&mut app);
    let report = &mut app.run_report.as_mut().unwrap().report;
    assert_eq!(report.records_written, 6);
    let OutputPreview::Text {
        content,
        total_bytes: 307,
        truncated: false,
    } = report.outputs[0].preview()
    else {
        panic!("expected a complete 307-byte in-memory preview");
    };
    assert_eq!(
        content.as_bytes(),
        include_bytes!("../../../../cli/tests/fixtures/orders/expected_order_lines.csv")
    );
    let table = PreviewTable::parse(
        content.as_bytes(),
        &app.project.target_options,
        &app.project.target,
    )
    .unwrap();
    let order = table.column("Order_ID").unwrap();
    let item = table.column("Item_ID").unwrap();
    let desc = table.column("Desc").unwrap();
    let mut matches = table
        .rows
        .iter()
        .filter(|row| row[order] == "A-1001" && row[item] == "BR-100");
    assert_eq!(matches.next().unwrap()[desc], "PAINT BRUSH");
    assert!(matches.next().is_none());
    app.undo_project();
    app.validate_now();
    assert_eq!(issue(&app, &ValidationEndpoint::Target), problem);
    assert_eq!(std::fs::read(path).unwrap(), cli::orders_example::PROJECT);
    assert!(!output.exists());
}

#[test]
fn restore_the_orders_definition_as_one_edit_then_run_and_verify_the_keyed_cell_and_all_bytes() {
    let mut app = intact();
    let original = project_snapshot_key(&app.project);
    damage(&mut app);
    let broken = project_snapshot_key(&app.project);
    let count = app.history.undo_len();
    let problem = issue(&app, &ValidationEndpoint::Target);
    assert_eq!(
        problem.message,
        "root scope: binding for `Desc` references missing node 4"
    );
    assert!(app.navigate_to_diagnostic(&problem));
    let draft = app.connection_repair.as_ref().unwrap();
    assert_eq!(draft.target_label, "row.Desc");
    let saved = draft.saved.as_ref().unwrap();
    assert_eq!(saved.label, "Uppercase");
    assert_eq!(saved.inputs.len(), 1);
    assert!(saved.inputs[0].contains("Node 3"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved.definition).unwrap(),
        serde_json::json!({"kind":"call","function":"upper","args":[3]})
    );
    assert_eq!(project_snapshot_key(&app.project), broken);
    assert_eq!(app.history.undo_len(), count);

    app.apply_connection_repair();
    assert_eq!(app.history.undo_len(), count + 1);
    assert_eq!(app.history.undo_label(), Some("Repair connection"));
    assert_eq!(project_snapshot_key(&app.project), original);
    assert!(app.diagnostics.is_empty());
    app.undo_project();
    app.validate_now();
    assert_eq!(issue(&app, &ValidationEndpoint::Target), problem);
    assert_eq!(project_snapshot_key(&app.project), broken);
    app.redo_project();
    app.validate_now();
    assert!(app.diagnostics.is_empty());
    assert_eq!(project_snapshot_key(&app.project), original);

    let scratch = Scratch::new();
    let path = scratch.0.join("restored.json");
    app.save_document_to(&path).unwrap();
    let output = scratch.0.join("after.csv");
    assert_eq!(
        cli::run_project(&path, &fixture().join("Orders.xml"), &output).unwrap(),
        6
    );
    let actual = std::fs::read(output).unwrap();
    assert_eq!(
        actual,
        std::fs::read(fixture().join("expected_order_lines.csv")).unwrap()
    );
    let matching: Vec<_> = std::str::from_utf8(&actual)
        .unwrap()
        .lines()
        .skip(1)
        .map(|row| row.split(',').collect::<Vec<_>>())
        .filter(|cells| cells[0] == "A-1001" && cells[2] == "BR-100")
        .map(|cells| cells[3])
        .collect();
    assert_eq!(matching, ["PAINT BRUSH"]);
}

#[test]
fn no_guess_after_opening_an_invalid_file_and_manual_repair_changes_only_the_chosen_binding() {
    let mut app = intact();
    damage(&mut app);
    let scratch = Scratch::new();
    let path = scratch.0.join("broken.json");
    std::fs::write(
        &path,
        mapping::project_file::encode_pretty(&app.project).unwrap(),
    )
    .unwrap();
    app.load_project_from(&path);
    assert_eq!(app.document.saved_path(), Some(path.as_path()));
    assert!(!app.is_dirty());
    let problem = issue(&app, &ValidationEndpoint::Target);
    assert!(app.navigate_to_diagnostic(&problem));
    let draft = app.connection_repair.as_mut().unwrap();
    assert!(draft.saved.is_none());
    assert!(draft.replacement.is_none());
    let index = draft.binding.index;
    draft.replacement = Some(3);
    let mut expected = app.project.clone();
    expected.root.bindings[index].node = 3;
    app.apply_connection_repair();
    assert_eq!(
        project_snapshot_key(&app.project),
        project_snapshot_key(&expected)
    );
    assert!(app.diagnostics.is_empty());
    assert_eq!(app.history.undo_len(), 1);
    app.undo_project();
    app.validate_now();
    assert_eq!(issue(&app, &ValidationEndpoint::Target), problem);
    assert!(!app.is_dirty());
}

#[test]
fn only_the_last_successful_save_can_supply_a_matching_definition() {
    let mut app = intact();
    let scratch = Scratch::new();
    let saved = scratch.0.join("changed.json");
    app.project.graph.nodes.insert(
        4,
        Node::Call {
            function: "lower".into(),
            args: vec![3],
        },
    );
    app.save_document_to(&saved).unwrap();
    app.project.graph.nodes.insert(
        4,
        Node::Call {
            function: "upper".into(),
            args: vec![3],
        },
    );
    assert!(
        app.save_document_to(&scratch.0.join("no-parent").join("failed.json"))
            .is_err()
    );
    damage(&mut app);
    inspect(&mut app);
    assert_eq!(
        app.connection_repair
            .as_ref()
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .label,
        "Lowercase"
    );
    // Publishing the invalid file replaces the old evidence. An earlier undo
    // state, the public fixture and the field's name are not saved proof.
    app.save_document_to(&saved).unwrap();
    inspect(&mut app);
    assert!(app.connection_repair.as_ref().unwrap().saved.is_none());
}

#[test]
fn changed_scope_changed_or_absent_input_and_stale_proposals_do_not_restore_a_node() {
    for change in 0..3 {
        let mut app = intact();
        damage(&mut app);
        match change {
            0 => {
                app.project.root.iteration = ScopeIteration::None;
            }
            1 => {
                app.project
                    .graph
                    .nodes
                    .insert(3, app.project.graph.nodes[&2].clone());
            }
            _ => {
                app.project.graph.nodes.remove(&3);
            }
        }
        inspect(&mut app);
        assert!(app.connection_repair.as_ref().unwrap().saved.is_none());
        assert!(!app.project.graph.nodes.contains_key(&4));
    }
    let mut app = intact();
    damage(&mut app);
    let old = issue(&app, &ValidationEndpoint::Target);
    inspect(&mut app);
    app.project
        .graph
        .nodes
        .insert(3, app.project.graph.nodes[&2].clone());
    let before = project_snapshot_key(&app.project);
    let count = app.history.undo_len();
    app.apply_connection_repair();
    assert_eq!(project_snapshot_key(&app.project), before);
    assert_eq!(app.history.undo_len(), count);
    assert!(!app.navigate_to_diagnostic(&old));
}

#[test]
fn an_unsaved_clean_baseline_and_an_unchanged_argument_id_are_not_saved_proof() {
    let mut unsaved = FerruleApp::default();
    unsaved.project = intact().project;
    unsaved.main_canvas = CanvasDocumentState::main(&unsaved.project);
    unsaved.mark_clean();
    unsaved.rebase_history();
    damage(&mut unsaved);
    inspect(&mut unsaved);
    assert!(unsaved.connection_repair.as_ref().unwrap().saved.is_none());

    let mut app = intact();
    let scratch = Scratch::new();
    let input = app.project.graph.nodes[&3].clone();
    app.project.graph.nodes.insert(7, input);
    app.project.graph.nodes.insert(
        3,
        Node::Call {
            function: "trim".into(),
            args: vec![7],
        },
    );
    app.save_document_to(&scratch.0.join("nested.json"))
        .unwrap();
    damage(&mut app);
    inspect(&mut app);
    assert!(app.connection_repair.as_ref().unwrap().saved.is_some());
    app.connection_repair = None;
    app.project
        .graph
        .nodes
        .insert(7, app.project.graph.nodes[&2].clone());
    inspect(&mut app);
    assert!(app.connection_repair.as_ref().unwrap().saved.is_none());
}

#[test]
fn manual_choice_addresses_the_exact_named_target_even_when_the_primary_has_the_same_issue() {
    let mut app = intact();
    app.project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        schema: app.project.target.clone(),
        root: app.project.root.clone(),
        path: None,
        options: Default::default(),
    });
    damage(&mut app);
    let named = issue(
        &app,
        &ValidationEndpoint::NamedTarget {
            index: 0,
            name: "audit".into(),
        },
    );
    assert!(app.navigate_to_diagnostic(&named));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Target(0));
    let draft = app.connection_repair.as_mut().unwrap();
    assert!(draft.saved.is_none());
    assert!(draft.target_label.contains("audit"));
    let index = draft.binding.index;
    draft.replacement = Some(3);
    app.apply_connection_repair();
    assert_eq!(app.project.extra_targets[0].root.bindings[index].node, 3);
    assert_eq!(app.project.root.bindings[index].node, 4);
    assert_eq!(
        issue(&app, &ValidationEndpoint::Target)
            .broken_binding
            .unwrap()
            .missing,
        4
    );
    assert!(!app.project.graph.nodes.contains_key(&4));
}

#[test]
fn enter_on_the_issue_opens_the_proposal_and_escape_does_not_edit() {
    use egui_kittest::{
        Harness,
        kittest::{NodeT as _, Queryable as _},
    };
    let mut app = intact();
    damage(&mut app);
    let key = project_snapshot_key(&app.project);
    let count = app.history.undo_len();
    let mut harness = Harness::builder()
        .with_size((960.0, 640.0))
        .with_accessibility_check(false)
        .build_ui_state(
            |ui, app: &mut FerruleApp| {
                if let Some(issue) = app.diagnostics.show(ui) {
                    app.navigate_to_diagnostic(&issue);
                }
                app.show_connection_repair(ui.ctx());
            },
            app,
        );
    for _ in 0..10 {
        if harness.get_by_label("Inspect connection").is_focused() {
            break;
        }
        harness.key_press(egui::Key::Tab);
        harness.run_steps(2);
    }
    assert!(harness.get_by_label("Inspect connection").is_focused());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    harness.get_by_label("Restore Uppercase");
    harness.get_by_label("row.Desc");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().connection_repair.is_none());
    assert_eq!(project_snapshot_key(&harness.state().project), key);
    assert_eq!(harness.state().history.undo_len(), count);
    assert_eq!(
        issue(harness.state(), &ValidationEndpoint::Target)
            .broken_binding
            .unwrap()
            .missing,
        4
    );
}
