use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::app::tests::wait_for_file_run_completion;

const ORDERS: &[u8] = include_bytes!("../../../cli/tests/fixtures/orders/Orders.xml");
const PROJECT: &[u8] = include_bytes!("../../../cli/tests/fixtures/orders/project.json");
const CSV: &[u8] = include_bytes!("../../../cli/tests/fixtures/orders/expected_order_lines.csv");

struct Orders(PathBuf);

impl Orders {
    fn open(named: bool) -> anyhow::Result<(Self, FerruleApp)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-destination-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let fixture = Self(root);
        let mut project = mapping::project_file::decode_bytes(PROJECT)?;
        project.source_path = Some("Orders.xml".into());
        project.target_path = Some("order-lines.csv".into());
        if named {
            project.extra_targets.push(mapping::NamedTarget {
                name: "by-order".into(),
                path: Some("named.csv".into()),
                schema: project.target.clone(),
                options: project.target_options.clone(),
                root: project.root.clone(),
            });
        }
        let path = fixture.0.join("orders.json");
        fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
        fs::write(fixture.0.join("Orders.xml"), ORDERS)?;
        let mut app = FerruleApp::default();
        app.load_project_from(&path);
        app.input_path.clear();
        app.output_path.clear();
        Ok((fixture, app))
    }
}

impl Drop for Orders {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn review(app: &mut FerruleApp) -> &mut DestinationReview {
    assert!(app.pending_file_run.is_none());
    app.run_destinations.as_mut().expect("Run opens review")
}

#[test]
fn occupied_name_apply_recheck_and_run_the_reviewed_orders_file() -> anyhow::Result<()> {
    let (fixture, mut app) = Orders::open(false)?;
    let occupied = fixture.0.join("order-lines.csv");
    let free = fixture.0.join("order-lines-02.csv");
    fs::write(&occupied, b"existing")?;
    app.run(&egui::Context::default());
    let draft = review(&mut app);
    assert_eq!(
        draft.inputs[0].path.as_deref(),
        Some(fixture.0.join("Orders.xml").as_path())
    );
    assert_eq!(draft.outputs[0].path(), occupied);
    assert!(
        draft.outputs[0]
            .problem
            .as_ref()
            .unwrap()
            .contains("occupied")
    );
    assert!(!draft.ready());
    draft.outputs[0].filename = Some("order-lines-02.csv".into());
    draft.checked = false;
    app.confirm_run_destinations();
    assert!(app.pending_file_run.is_none());
    review(&mut app).recheck();
    assert!(review(&mut app).ready());
    assert!(!free.exists());
    app.confirm_run_destinations();
    wait_for_file_run_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
    assert_eq!(fs::read(&free)?, CSV);
    assert_eq!(fs::read(&occupied)?, b"existing");
    assert_eq!(app.project.target_path.as_deref(), Some("order-lines.csv"));
    Ok(())
}

#[test]
fn blank_directory_same_input_and_bad_parent_block_run() -> anyhow::Result<()> {
    let (fixture, mut app) = Orders::open(false)?;
    app.run(&egui::Context::default());
    let draft = review(&mut app);
    for (name, reason) in [
        (" ", "filename"),
        ("Orders.xml", "input"),
        ("orders.json", "project"),
        ("unavailable/file.csv", "folder"),
        ("Orders.xml/file.csv", "directory"),
    ] {
        draft.outputs[0].filename = Some(name.into());
        draft.recheck();
        assert!(!draft.ready(), "{name}");
        assert!(
            draft.outputs[0]
                .problem
                .as_deref()
                .unwrap()
                .contains(reason),
            "{name}: {:?}",
            draft.outputs[0].problem
        );
    }
    fs::create_dir(fixture.0.join("folder"))?;
    draft.outputs[0].filename = Some("folder".into());
    draft.recheck();
    assert!(
        draft.outputs[0]
            .problem
            .as_deref()
            .unwrap()
            .contains("directory")
    );
    app.confirm_run_destinations();
    assert!(app.pending_file_run.is_none());
    assert_eq!(fs::read(fixture.0.join("Orders.xml"))?, ORDERS);
    Ok(())
}

#[test]
fn named_selection_runs_only_the_named_target() -> anyhow::Result<()> {
    let (fixture, mut app) = Orders::open(true)?;
    app.run(&egui::Context::default());
    let draft = review(&mut app);
    assert_eq!(draft.outputs.len(), 2);
    draft.run.selection = Some(PreviewTarget::Named("by-order".into()));
    draft.refresh_plan();
    assert!(draft.ready());
    assert_eq!(draft.outputs.len(), 1);
    assert_eq!(draft.outputs[0].path(), fixture.0.join("named.csv"));
    draft.outputs[0].filename = Some("selected.csv".into());
    draft.recheck();
    app.confirm_run_destinations();
    wait_for_file_run_completion(&mut app);
    assert_eq!(fs::read(fixture.0.join("selected.csv"))?, CSV);
    assert!(!fixture.0.join("order-lines.csv").exists());
    assert!(!fixture.0.join("named.csv").exists());
    Ok(())
}

#[test]
fn all_targets_use_their_reviewed_distinct_paths() -> anyhow::Result<()> {
    let (fixture, mut app) = Orders::open(true)?;
    app.run(&egui::Context::default());
    let draft = review(&mut app);
    draft.outputs[1].filename = draft.outputs[0].filename.clone();
    draft.recheck();
    assert!(!draft.ready());
    draft.outputs[1].filename = Some("renamed.csv".into());
    draft.recheck();
    app.confirm_run_destinations();
    wait_for_file_run_completion(&mut app);
    assert_eq!(fs::read(fixture.0.join("order-lines.csv"))?, CSV);
    assert_eq!(fs::read(fixture.0.join("renamed.csv"))?, CSV);
    assert!(!fixture.0.join("named.csv").exists());
    Ok(())
}

#[test]
fn collision_after_review_or_after_worker_launch_preserves_competing_bytes() -> anyhow::Result<()> {
    let (fixture, mut app) = Orders::open(false)?;
    let out = fixture.0.join("order-lines.csv");
    app.run(&egui::Context::default());
    assert!(review(&mut app).ready());
    fs::write(&out, b"before confirmation")?;
    app.confirm_run_destinations();
    assert!(!review(&mut app).ready());
    assert_eq!(fs::read(&out)?, b"before confirmation");
    fs::remove_file(&out)?;
    review(&mut app).recheck();
    app.confirm_run_destinations();
    assert!(app.pending_file_run.is_some());
    // No UI poll has granted publication to the saved-file worker yet.
    fs::write(&out, b"after confirmation")?;
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run failed");
    assert!(!app.show_run_report);
    assert_eq!(fs::read(out)?, b"after confirmation");
    Ok(())
}

#[test]
fn editor_workflow_preview_does_not_publish_and_a_late_conflict_reports_both_paths()
-> anyhow::Result<()> {
    use crate::app::run_ui::{FileRunCommand, FileRunEvent};
    use crate::app::tests::wait_for_preview_completion;

    let (fixture, mut app) = Orders::open(true)?;
    let primary = fixture.0.join("order-lines.csv");
    let named = fixture.0.join("named.csv");
    let input = fixture.0.join("Orders.xml");
    let project = fixture.0.join("orders.json");
    let project_before = fs::read(&project)?;

    app.begin_preview();
    app.preview_draft.as_mut().unwrap().input_text = cli::orders_example::INPUT.to_owned();
    app.execute_preview();
    wait_for_preview_completion(&mut app);
    assert!(app.show_run_report, "{}", app.status);
    assert_eq!(fs::read_dir(&fixture.0)?.count(), 2);
    assert!(!primary.exists() && !named.exists());
    app.show_run_report = false;
    app.run(&egui::Context::default());
    assert!(review(&mut app).ready());
    app.confirm_run_destinations();
    // Intervene only after the real worker has finished staging and asked the
    // UI for publication. This distinguishes a partial publication from a
    // preflight refusal without a timing-dependent filesystem race.
    let event = app
        .pending_file_run
        .as_ref()
        .unwrap()
        .receiver
        .recv_timeout(std::time::Duration::from_secs(10))?;
    assert!(matches!(event, FileRunEvent::ReadyToPublish));
    let competitor = b"created after review";
    fs::write(&named, competitor)?;
    app.file_run_command(FileRunCommand::Publish);
    wait_for_file_run_completion(&mut app);
    assert_eq!(app.status, "run failed");
    let [diagnostic] = app.diagnostics.items() else {
        panic!("expected one publication diagnostic");
    };
    let (conflict, published) = diagnostic
        .message
        .split_once("; complete files already published: ")
        .expect("diagnostic distinguishes the conflict from committed files");
    assert!(
        conflict.contains(&named.display().to_string()),
        "{conflict}"
    );
    assert_eq!(published, primary.display().to_string());
    assert_eq!(fs::read(primary)?, CSV);
    assert_eq!(fs::read(named)?, competitor);
    assert_eq!(fs::read(input)?, ORDERS);
    assert_eq!(fs::read(project)?, project_before);
    assert_eq!(fs::read_dir(&fixture.0)?.count(), 4);
    Ok(())
}

#[test]
fn tab_apply_and_escape_do_not_start_a_worker() -> anyhow::Result<()> {
    use egui_kittest::{
        Harness,
        kittest::{NodeT as _, Queryable as _},
    };
    let (fixture, mut app) = Orders::open(false)?;
    fs::write(fixture.0.join("order-lines.csv"), b"existing")?;
    app.run(&egui::Context::default());
    let mut harness = Harness::builder()
        .with_size((1000.0, 700.0))
        .with_accessibility_check(false)
        .build_ui_state(
            |ui, app: &mut FerruleApp| app.show_run_destinations(ui.ctx()),
            app,
        );
    let role = egui::accesskit::Role::TextInput;
    for _ in 0..12 {
        if harness
            .get_by_role_and_label(role, "Filename or path")
            .is_focused()
        {
            break;
        }
        harness.key_press(egui::Key::Tab);
        harness.run_steps(2);
    }
    assert!(
        harness
            .get_by_role_and_label(role, "Filename or path")
            .is_focused()
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run_steps(2);
    harness
        .get_by_role_and_label(role, "Filename or path")
        .type_text("keyboard.csv");
    harness.run_steps(2);
    assert!(!harness.state().run_destinations.as_ref().unwrap().ready());
    for _ in 0..12 {
        if harness.get_by_label("Apply and recheck").is_focused() {
            break;
        }
        harness.key_press(egui::Key::Tab);
        harness.run_steps(2);
    }
    assert!(harness.get_by_label("Apply and recheck").is_focused());
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().run_destinations.as_ref().unwrap().ready());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().run_destinations.is_none());
    assert!(harness.state().pending_file_run.is_none());
    assert!(!fixture.0.join("keyboard.csv").exists());
    assert_eq!(fs::read(fixture.0.join("order-lines.csv"))?, b"existing");
    Ok(())
}

#[test]
fn unconfigured_output_is_editable_but_never_a_default_publication() -> anyhow::Result<()> {
    let (fixture, mut app) = Orders::open(false)?;
    app.project.target_path = None;
    app.run(&egui::Context::default());
    let draft = review(&mut app);
    assert_eq!(draft.outputs[0].filename.as_deref(), Some(""));
    assert!(!draft.ready());
    draft.outputs[0].filename = Some("entered.csv".into());
    draft.recheck();
    app.confirm_run_destinations();
    wait_for_file_run_completion(&mut app);
    assert_eq!(fs::read(fixture.0.join("entered.csv"))?, CSV);
    Ok(())
}
