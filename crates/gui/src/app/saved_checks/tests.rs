use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::document_file::fault::{self, Fault};
use crate::run_report::MAX_PREVIEW_BYTES;
use editor_ui::mapping_checks::{Assertion, KeyPart};
use ir::{ScalarType, SchemaNode};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-saved-checks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cli/tests/fixtures/orders")
        .join(file)
}

fn schema() -> SchemaNode {
    SchemaNode::group("row", vec![SchemaNode::scalar("id", ScalarType::String)])
}

#[test]
fn native_authoring_and_enter_on_failure_open_the_real_reordered_preview_cell() {
    use crate::preview::{PreviewDraft, PreviewTarget};
    use egui_kittest::{Harness, kittest::Queryable as _};

    let temp = Temp::new();
    let mapping = temp.0.join("orders.json");
    let mut project =
        mapping::project_file::decode_bytes(&fs::read(fixture("project.json")).unwrap()).unwrap();
    // The ordinary source field instead of upper(Desc).
    project
        .root
        .bindings
        .iter_mut()
        .find(|b| b.target_field == "Desc")
        .unwrap()
        .node = 3;
    fs::write(
        &mapping,
        mapping::project_file::encode_pretty(&project).unwrap(),
    )
    .unwrap();
    let mut app = FerruleApp::default();
    app.load_project_from(&mapping);
    let output_path = temp.0.join("candidate.csv");
    let mut draft = PreviewDraft::new(
        PreviewTarget::Primary,
        "Orders.xml".into(),
        output_path.to_string_lossy().into_owned(),
    );
    let mut input = fs::read_to_string(fixture("Orders.xml")).unwrap();
    let start = input.find("<Item>").unwrap();
    let after = input.find("</Item>").unwrap() + "</Item>".len();
    let brush = input[start..after].to_owned();
    input.replace_range(start..after, "");
    input.insert_str(input.find("</Items>").unwrap(), &brush);
    draft.input_text = input;
    app.preview_draft = Some(draft);
    app.execute_preview();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let ctx = egui::Context::default();
    while app.pending_preview.is_some() {
        assert!(
            std::time::Instant::now() < deadline,
            "preview worker timed out"
        );
        app.poll_preview(&ctx);
        std::thread::yield_now();
    }
    let (target, artifact, table) = app.selected_preview_table().unwrap();
    let output = Output {
        target: target.into(),
        artifact: artifact.into(),
    };
    let captured = std::ptr::from_ref(table);
    assert_eq!(table.rows[2][3], "paint brush");
    app.open_saved_checks();
    let mut window = app.saved_checks.take().unwrap();
    window
        .start_from_reference(&app, output, &fixture("expected_order_lines.csv"))
        .unwrap();
    app.saved_checks = Some(window);
    let mut frame = eframe::Frame::_new_kittest();
    let mut initialized = false;
    let mut harness = Harness::builder()
        .with_size((1280.0, 1152.0))
        .with_accessibility_check(false) // The full editor includes legacy unnamed controls.
        .build_ui_state(
            move |ui, app: &mut FerruleApp| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    initialized = true;
                }
                eframe::App::ui(app, ui, &mut frame);
            },
            app,
        );
    harness.run_steps(3);
    harness.get_by_label("Set name").click();
    harness.run_steps(2);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    harness
        .get_by_label("Set name")
        .type_text("Orders to order lines");
    harness.run_steps(2);
    for name in ["Key: Order_ID", "Key: Item_ID", "Check Desc"] {
        harness.get_by_label(name).click();
        harness.run_steps(2);
    }
    harness.get_by_label("Cell check name").click();
    harness.run_steps(2);
    harness
        .get_by_label("Cell check name")
        .type_text("Brush description");
    harness.run_steps(2);
    harness.get_by_label("Add keyed cell check").click();
    harness.run_steps(2);
    harness.get_by_label("Save check set").click();
    harness.run_steps(2);
    let saved = Store::open(&mapping).unwrap();
    assert_eq!(saved.sets.len(), 1);
    assert_eq!(saved.sets[0].name, "Orders to order lines");
    let checks = &saved.sets[0].checks;
    assert_eq!(checks.len(), 3);
    assert_eq!(checks[2].name, "Brush description");
    assert_eq!(
        checks[2].assertion,
        orders_set(&fixture("expected_order_lines.csv")).0.checks[2].assertion
    );
    harness
        .get_by_label("Run checks on selected preview")
        .click();
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Last check run: 2 passed · 1 failed")
            .is_some()
    );
    harness.get_by_label("Fail · Brush description").focus();
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    assert!(
        harness
            .query_by_label("Selected cell · Row 3 · Desc")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("Actual: Desc = \"paint brush\" (1 matching row)")
            .is_some()
    );
    let app = harness.state();
    assert_eq!(
        std::ptr::from_ref(app.selected_preview_table().unwrap().2),
        captured
    );
    assert!(app.pending_preview.is_none() && !app.is_dirty() && !output_path.exists());
}

fn orders_set(path: &Path) -> (CheckSet, PreviewTable) {
    let (reference, table) = Reference::capture(path, &Default::default(), &schema()).unwrap();
    let output = Output {
        target: "Primary".into(),
        artifact: "output.csv".into(),
    };
    let mut draft = Draft::new(output, reference, table);
    let reference = draft.table.take().unwrap();
    draft.set.name = "Orders to order lines".into();
    draft.set.checks[0].name = "Six order lines".into();
    draft.set.checks[1].name = "CSV columns in order".into();
    draft.set.checks.push(NamedCheck {
        name: "Brush description".into(),
        assertion: Assertion::capture_cell(
            vec![
                KeyPart {
                    column: "Order_ID".into(),
                    value: "A-1001".into(),
                },
                KeyPart {
                    column: "Item_ID".into(),
                    value: "BR-100".into(),
                },
            ],
            "Desc".into(),
            headers(&reference),
            &reference.rows,
        )
        .unwrap(),
    });
    (draft.set, reference)
}

#[test]
fn named_checks_resolve_only_the_unique_cell_in_the_current_complete_capture() {
    let (mut set, mut candidate) = orders_set(&fixture("expected_order_lines.csv"));
    set.output.target = "Line report".into();
    let path = Path::new("output.csv");
    let target = "Line report";
    candidate.rows.reverse();
    let good = set.run((target, path, &candidate)).unwrap();
    assert_eq!(candidate.rows.len(), 6);
    assert!(good.results.iter().all(|check| check.passed));
    candidate.rows[5][3] = "paint brush".into();
    let bad = set.run((target, path, &candidate)).unwrap();
    assert_eq!(
        bad.results.iter().map(|r| r.passed).collect::<Vec<_>>(),
        [true, true, false]
    );
    assert!(bad.results[2].actual.contains("paint brush"));
    assert_eq!(
        set.resolve_result(&bad, 2, (target, path, &candidate))
            .unwrap(),
        Some((5, "Desc"))
    );
    assert!(
        set.resolve_result(&bad, 2, ("Primary", path, &candidate))
            .is_err()
    );
    assert!(
        set.resolve_result(&bad, 2, (target, Path::new("other.csv"), &candidate))
            .is_err()
    );
    assert!(
        set.resolve_result(&good, 2, (target, path, &candidate))
            .is_err()
    );
    candidate.rows.reverse();
    assert!(
        set.resolve_result(&bad, 2, (target, path, &candidate))
            .unwrap_err()
            .contains("changed")
    );
    let rerun = set.run((target, path, &candidate)).unwrap();
    assert_eq!(
        set.resolve_result(&rerun, 2, (target, path, &candidate))
            .unwrap(),
        Some((0, "Desc"))
    );
    candidate.rows.push(candidate.rows[0].clone());
    let ambiguous = set.run((target, path, &candidate)).unwrap();
    assert_eq!(
        ambiguous.results[2].actual,
        "Ambiguous key: 2 matching rows."
    );
    assert!(
        set.resolve_result(&ambiguous, 2, (target, path, &candidate))
            .is_err()
    );
    candidate.rows.clear();
    let empty = set.run((target, path, &candidate)).unwrap();
    assert_eq!(
        empty.results.iter().map(|r| r.passed).collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(empty.results[2].actual, "Missing key: 0 matching rows.");
    assert!(
        set.resolve_result(&empty, 2, (target, path, &candidate))
            .is_err()
    );
}

#[test]
fn older_mapping_reopens_without_preview_or_reference_and_cancel_leaves_history_alone() {
    let temp = Temp::new();
    let mapping = temp.0.join("legacy.json");
    let reference = temp.0.join("known-good.csv");
    fs::copy(fixture("project.json"), &mapping).unwrap();
    fs::copy(fixture("expected_order_lines.csv"), &reference).unwrap();
    let original = fs::read(&mapping).unwrap();
    let mut app = FerruleApp::default();
    app.load_project_from(&mapping);
    app.open_saved_checks();
    let mut window = app.saved_checks.take().unwrap();
    assert!(window.store.sets.is_empty());
    assert!(!window.store.path.exists());
    let (set, _) = orders_set(&reference);
    window.draft = Some(Draft::edit(0, set.clone()));
    window.act(Action::Cancel, &mut app).unwrap();
    assert!(!window.store.path.exists());
    window.store.save(None, set.clone()).unwrap();
    fs::remove_file(reference).unwrap();
    drop(window);
    app.load_project_from(&mapping);
    app.open_saved_checks();
    let mut reopened = app.saved_checks.take().unwrap();
    assert_eq!(reopened.store.sets, [set]);
    assert!(reopened.reference_notice.contains("unavailable"));
    assert!(
        reopened.last_run.is_none() && app.run_report.is_none() && app.pending_preview.is_none()
    );
    let reason = app.selected_preview_table().err().unwrap();
    assert_eq!(reopened.act(Action::Run, &mut app).unwrap_err(), reason);
    reopened.act(Action::Edit, &mut app).unwrap();
    reopened.draft.as_mut().unwrap().set.name = "Renamed".into();
    reopened.act(Action::Save, &mut app).unwrap();
    reopened.act(Action::Undo, &mut app).unwrap();
    assert_eq!(reopened.store.sets[0].name, "Orders to order lines");
    reopened.act(Action::Undo, &mut app).unwrap();
    assert_eq!(Store::open(&mapping).unwrap().sets[0].name, "Renamed");
    assert!(!app.is_dirty() && !app.can_undo());
    assert_eq!(fs::read(&mapping).unwrap(), original);
}

#[test]
fn atomic_failure_concurrent_edits_and_unknown_versions_preserve_saved_checks() {
    let temp = Temp::new();
    let mapping = temp.0.join("mapping.json");
    let mut first = Store::open(&mapping).unwrap();
    let mut other = Store::open(&mapping).unwrap();
    let (set, _) = orders_set(&fixture("expected_order_lines.csv"));
    first.save(None, set.clone()).unwrap();
    let original = fs::read(&first.path).unwrap();
    assert!(
        other
            .save(None, set.clone())
            .unwrap_err()
            .to_string()
            .contains("changed on disk")
    );
    assert!(other.sets.is_empty() && !other.can_undo_or_redo());
    let mut changed = set.clone();
    changed.name = "Changed".into();
    fault::arm(&first.path, Fault::BeforePublish);
    assert!(first.save(Some(0), changed).is_err());
    assert_eq!(first.sets, [set]);
    assert_eq!(fs::read(&first.path).unwrap(), original);
    fault::arm(&first.path, Fault::PartialWrite);
    assert!(first.undo_or_redo().is_err());
    assert!(!first.redo && first.can_undo_or_redo());
    first.undo_or_redo().unwrap();
    assert!(Store::open(&mapping).unwrap().sets.is_empty());
    let unknown = br#"{"version":99,"sets":[]}"#;
    fs::write(&first.path, unknown).unwrap();
    assert!(
        Store::open(&mapping)
            .err()
            .unwrap()
            .to_string()
            .contains("Unsupported")
    );
    assert!(first.undo_or_redo().is_err());
    assert_eq!(fs::read(&first.path).unwrap(), unknown);
}

#[test]
fn chosen_reference_uses_captured_dialect_and_rejects_missing_excess_and_output_aliases() {
    let temp = Temp::new();
    let path = temp.0.join("known-good.arbitrary");
    let options = mapping::FormatOptions {
        delimiter: Some(';'),
        csv_quote: Some('\''),
        ..Default::default()
    };
    fs::write(
        &path,
        b"\xef\xbb\xbffield;label\n001;'line; one\nline ''two'''\n",
    )
    .unwrap();
    let (baseline, table) = Reference::capture(&path, &options, &schema()).unwrap();
    assert_eq!(table.headers, ["field", "label"]);
    assert_eq!(table.rows[0], ["001", "line; one\nline 'two'"]);
    let output = Output {
        target: "Primary".into(),
        artifact: "known-good.arbitrary".into(),
    };
    assert!(store::require_independent_file(&temp.0.join("project.json"), &output, &path).is_err());
    let alias = temp.0.join("another-name.csv");
    fs::hard_link(&path, &alias).unwrap();
    assert!(
        store::require_independent_file(&temp.0.join("project.json"), &output, &alias).is_err()
    );
    fs::write(&path, vec![b'a'; MAX_PREVIEW_BYTES + 1]).unwrap();
    assert!(
        Reference::capture(&path, &options, &schema())
            .err()
            .unwrap()
            .to_string()
            .contains("exceeds")
    );
    fs::remove_file(&path).unwrap();
    assert!(Reference::capture(&path, &options, &schema()).is_err());
    assert_eq!(baseline.rows_for_edit(&options, &schema()).unwrap(), table);
    assert!(
        baseline
            .rows_for_edit(&Default::default(), &schema())
            .is_err()
    );
}
