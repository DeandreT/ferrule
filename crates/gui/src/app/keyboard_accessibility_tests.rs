//! Keyboard-only qualification of the desktop editor.
//!
//! Every interaction drives the complete application with key and text events
//! only; fixtures are loaded through the app's own open entry point. Ignored
//! tests assert the desired behaviour of each gap recorded in
//! `docs/qualification/keyboard-only-editor.md`; run them with
//! `--run-ignored only` to reproduce the gaps.
//!
//! The native Open and Save As choosers are outside egui. Tests reach Open only
//! with unsaved work, so the shortcut stops at Ferrule's own confirmation.

use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use ir::{ScalarType, SchemaNode};
use mapping::Binding;

const PREVIEW_BUTTON: &str = "Preview the current target from in-memory input (Ctrl+Shift+P)";
const EDIT_CONST: &str = "Edit Const: Keyboard · F2";
const UNSAVED_DIALOG_BUTTONS: [&str; 4] = ["Close", "Discard changes", "Save", "Cancel"];
/// More than two complete Tab cycles of the fixture editor.
const MAX_STOPS: usize = 160;

/// A project directory removed even when an assertion fails.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Order { Name, Note } to the same shape; Note is bound to a constant and
/// Name is left for the connection workflows.
fn order_project() -> Project {
    let mut project = blank_project();
    let schema = SchemaNode::group(
        "Order",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Note", ScalarType::String),
        ],
    );
    project.source = schema.clone();
    project.target = schema;
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: ir::Value::String("Keyboard".into()),
        },
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Note".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    project
}

fn saved_project() -> (Scratch, PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("ferrule-keyboard-{}-{index}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("orders.json");
    std::fs::write(
        &path,
        mapping::project_file::encode_pretty(&order_project()).unwrap(),
    )
    .unwrap();
    (Scratch(directory), path)
}

/// The complete application UI. The window title command repaints every
/// frame, so callers advance a fixed number of frames instead of settling.
fn harness(app: FerruleApp) -> Harness<'static, FerruleApp> {
    let mut frame = eframe::Frame::_new_kittest();
    let mut initialized = false;
    let mut harness = Harness::builder()
        .with_size((1280.0, 800.0))
        // Unnamed controls are qualified explicitly below.
        .with_accessibility_check(false)
        .with_os(egui::os::OperatingSystem::Nix)
        .build_ui_state(
            move |ui, app: &mut FerruleApp| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.visuals.text_cursor.blink = false;
                        style.scroll_animation = egui::style::ScrollAnimation::none();
                        style.animation_time = 0.0;
                    });
                    initialized = true;
                }
                eframe::App::ui(app, ui, &mut frame);
            },
            app,
        );
    harness.run_steps(4);
    harness
}

fn editor(path: &Path) -> Harness<'static, FerruleApp> {
    let mut app = FerruleApp::default();
    app.load_project_from(path);
    assert_eq!(app.document.saved_path(), Some(path));
    harness(app)
}

fn command() -> egui::Modifiers {
    egui::Modifiers::CTRL | egui::Modifiers::COMMAND
}

fn press(harness: &mut Harness<'_, FerruleApp>, key: egui::Key) {
    harness.key_press(key);
    harness.run_steps(3);
}

fn press_with(harness: &mut Harness<'_, FerruleApp>, modifiers: egui::Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run_steps(3);
}

fn type_text(harness: &mut Harness<'_, FerruleApp>, text: &str) {
    harness.event(egui::Event::Text(text.into()));
    harness.run_steps(3);
}

/// The widget holding keyboard focus, as egui and AccessKit report it.
#[derive(Clone, Debug)]
struct Stop {
    id: egui::Id,
    name: String,
    role: String,
    /// Bounds reported to assistive technology.
    bounds: egui::Rect,
    layer: egui::LayerId,
}

impl Stop {
    fn on_canvas(&self) -> bool {
        self.layer.order == egui::Order::Background && self.layer != egui::LayerId::background()
    }
}

fn focus(harness: &Harness<'_, FerruleApp>) -> Option<Stop> {
    let id = harness.ctx.memory(|memory| memory.focused())?;
    let layer = harness.ctx.read_response(id)?.layer_id;
    let (name, role, bounds) = harness
        .root()
        .children_recursive()
        .find(|node| node.is_focused())
        .map(|node| {
            let accesskit = node.accesskit_node();
            (
                accesskit
                    .label()
                    .or_else(|| accesskit.value())
                    .unwrap_or_default(),
                format!("{:?}", accesskit.role()),
                node.rect(),
            )
        })
        .unwrap_or_else(|| (String::new(), "None".into(), egui::Rect::NAN));
    Some(Stop {
        id,
        name,
        role,
        bounds,
        layer,
    })
}

fn focused_name(harness: &Harness<'_, FerruleApp>) -> Option<String> {
    focus(harness).map(|stop| stop.name)
}

fn tab_until(
    harness: &mut Harness<'_, FerruleApp>,
    wanted: impl Fn(&str) -> bool,
) -> Option<String> {
    for _ in 0..MAX_STOPS {
        press(harness, egui::Key::Tab);
        if let Some(name) = focused_name(harness).filter(|name| wanted(name)) {
            return Some(name);
        }
    }
    None
}

fn tab_to(harness: &mut Harness<'_, FerruleApp>, wanted: &str) -> bool {
    tab_until(harness, |name| name == wanted).is_some()
}

/// Forward stops from a fresh editor through the canvas Layout menu, the last
/// control before the canvas graph.
fn stops_before_canvas(harness: &mut Harness<'_, FerruleApp>) -> Vec<Option<Stop>> {
    let mut stops = Vec::new();
    for _ in 0..MAX_STOPS {
        press(harness, egui::Key::Tab);
        let stop = focus(harness);
        let done = stop.as_ref().is_some_and(|stop| stop.name == "Layout");
        stops.push(stop);
        if done {
            return stops;
        }
    }
    panic!("Tab never reached the canvas Layout menu");
}

fn bindings(harness: &Harness<'_, FerruleApp>) -> Vec<(String, NodeId)> {
    harness
        .state()
        .project
        .root
        .bindings
        .iter()
        .map(|binding| (binding.target_field.clone(), binding.node))
        .collect()
}

/// Retargets the constant's binding from Note to Name through the inspector's
/// target-field combo box.
fn retarget_binding_to_name(harness: &mut Harness<'_, FerruleApp>) {
    assert!(
        tab_to(harness, "Note"),
        "target-field combo box is a Tab stop"
    );
    press(harness, egui::Key::Enter);
    press(harness, egui::Key::Tab);
    assert_eq!(focused_name(harness).as_deref(), Some("Name"));
    press(harness, egui::Key::Enter);
    assert_eq!(bindings(harness), [("Name".to_owned(), 0)]);
    assert!(harness.state().observed_dirty());
}

/// Ctrl+O with unsaved work stops at Ferrule's confirmation; a clean project
/// would start the native chooser instead.
fn open_shortcut_on_unsaved_work(harness: &mut Harness<'_, FerruleApp>) {
    assert!(harness.state().observed_dirty());
    press_with(harness, command(), egui::Key::O);
    assert_eq!(
        harness.state().pending_destructive_action,
        Some(DestructiveAction::OpenProject)
    );
    assert!(harness.state().pending_dialog.is_none());
}

fn open_auto_connect_from_menu(harness: &mut Harness<'_, FerruleApp>) {
    assert!(tab_to(harness, "Mapping"));
    press(harness, egui::Key::Enter);
    assert!(tab_to(harness, "Auto-connect fields…"));
    press(harness, egui::Key::Enter);
    assert!(harness.state().pending_auto_connect.is_some());
}

fn project_key(harness: &Harness<'_, FerruleApp>) -> String {
    crate::project_state::project_snapshot_key(&harness.state().project)
}

fn collect_rects(shape: &egui::Shape, rects: &mut Vec<egui::epaint::RectShape>) {
    match shape {
        egui::Shape::Rect(rect) => rects.push(rect.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_rects(shape, rects);
            }
        }
        _ => {}
    }
}

/// Whether the last frame painted the focused widget in egui's focused
/// (active) style, which this theme distinguishes from the resting style.
fn focus_indicator_drawn(harness: &Harness<'_, FerruleApp>, id: egui::Id) -> bool {
    let Some(rect) = harness.ctx.read_response(id).map(|response| response.rect) else {
        return false;
    };
    let widgets = harness.ctx.global_style().visuals.widgets.clone();
    let (active, inactive) = (widgets.active, widgets.inactive);
    let mut rects = Vec::new();
    for clipped in &harness.output().shapes {
        collect_rects(&clipped.shape, &mut rects);
    }
    rects.iter().any(|shape| {
        rect.expand(2.0).contains_rect(shape.rect)
            && ((shape.fill == active.weak_bg_fill && active.weak_bg_fill != inactive.weak_bg_fill)
                || (shape.fill == active.bg_fill && active.bg_fill != inactive.bg_fill)
                || (shape.stroke == active.bg_stroke && active.bg_stroke != inactive.bg_stroke))
    })
}

#[test]
fn keyboard_accessibility_opens_a_recent_project_from_the_start_view() {
    let (_scratch, path) = saved_project();
    let mut harness = harness(FerruleApp {
        recent_projects: vec![path.display().to_string()],
        ..FerruleApp::default()
    });
    assert!(harness.state().is_start_state());
    assert!(
        tab_to(&mut harness, "orders.json"),
        "recent project is a Tab stop"
    );
    press(&mut harness, egui::Key::Enter);
    assert_eq!(harness.state().document.saved_path(), Some(path.as_path()));
    assert_eq!(
        project_key(&harness),
        crate::project_state::project_snapshot_key(&order_project())
    );
    assert!(!harness.state().observed_dirty());
    assert!(harness.state().pending_dialog.is_none());
}

#[test]
fn keyboard_accessibility_open_shortcut_keeps_unsaved_work_when_cancelled() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    retarget_binding_to_name(&mut harness);
    let edited = project_key(&harness);
    open_shortcut_on_unsaved_work(&mut harness);

    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..8 {
        press(&mut harness, egui::Key::Tab);
        let stop = focus(&harness).expect("Tab focuses a dialog control");
        assert!(!stop.on_canvas() && stop.layer != egui::LayerId::background());
        visited.insert(stop.name);
    }
    assert_eq!(
        visited,
        UNSAVED_DIALOG_BUTTONS.map(str::to_owned).into(),
        "Tab stays inside the modal dialog"
    );

    assert!(tab_to(&mut harness, "Cancel"));
    press(&mut harness, egui::Key::Enter);
    assert_eq!(harness.state().pending_destructive_action, None);
    assert!(harness.state().pending_dialog.is_none());
    assert!(harness.state().observed_dirty());
    assert!(harness.state().can_undo());
    assert_eq!(project_key(&harness), edited);
    assert_eq!(harness.state().document.saved_path(), Some(path.as_path()));
}

#[test]
fn keyboard_accessibility_tab_order_is_regional_and_shift_tab_reverses_it() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let forward = stops_before_canvas(&mut harness)
        .into_iter()
        .map(|stop| stop.expect("every stop before the canvas holds focus"))
        .collect::<Vec<_>>();
    let position = |name: &str| {
        forward
            .iter()
            .position(|stop| stop.name == name)
            .unwrap_or_else(|| panic!("{name} is not a Tab stop"))
    };
    // Menu bar, toolbar, mapping tabs, source pane, inspector, then the canvas.
    let regions = [
        "File",
        "View",
        "Open (Ctrl+O)",
        PREVIEW_BUTTON,
        "Main mapping",
        "Filter by field, group, path, or scalar type",
        "Target",
        "Iterate source collection",
        "Note",
        "Add binding",
        "Layout",
    ]
    .map(position);
    assert!(regions.is_sorted(), "{regions:?}");
    assert_eq!(regions[0], 0);

    let mut backward = vec![forward[forward.len() - 1].id];
    for _ in 1..forward.len() {
        press_with(&mut harness, egui::Modifiers::SHIFT, egui::Key::Tab);
        backward.push(focus(&harness).expect("Shift+Tab holds focus").id);
    }
    backward.reverse();
    assert_eq!(
        backward,
        forward.iter().map(|stop| stop.id).collect::<Vec<_>>()
    );
}

#[test]
fn keyboard_accessibility_focused_controls_draw_a_focus_indicator() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    for name in [
        "File",
        "Open (Ctrl+O)",
        PREVIEW_BUTTON,
        "Target",
        "Iterate source collection",
        "Note",
        "Add binding",
    ] {
        assert!(tab_to(&mut harness, name), "{name} is a Tab stop");
        let stop = focus(&harness).expect("focused control");
        assert!(
            stop.bounds.is_finite() && harness.ctx.content_rect().contains_rect(stop.bounds),
            "{name} is on screen: {:?}",
            stop.bounds
        );
        assert!(
            focus_indicator_drawn(&harness, stop.id),
            "{name} is painted as focused"
        );
    }
}

#[test]
fn keyboard_accessibility_edits_a_binding_with_undo_and_redo_shortcuts() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let original = project_key(&harness);
    retarget_binding_to_name(&mut harness);
    let edited = project_key(&harness);

    press_with(&mut harness, command(), egui::Key::Z);
    assert_eq!(project_key(&harness), original);
    assert!(!harness.state().observed_dirty());
    press_with(
        &mut harness,
        command() | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    assert_eq!(project_key(&harness), edited);
    assert!(harness.state().observed_dirty());
    press_with(&mut harness, command(), egui::Key::Z);
    assert_eq!(project_key(&harness), original);
    press_with(&mut harness, command(), egui::Key::Y);
    assert_eq!(project_key(&harness), edited);
}

#[test]
fn keyboard_accessibility_saves_and_reopens_a_keyboard_edit() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    retarget_binding_to_name(&mut harness);
    let edited = project_key(&harness);
    press_with(&mut harness, command(), egui::Key::S);
    assert!(!harness.state().observed_dirty());
    assert!(harness.state().pending_dialog.is_none());
    let saved = mapping::project_file::decode_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(crate::project_state::project_snapshot_key(&saved), edited);

    let mut reopened = self::harness(FerruleApp {
        recent_projects: vec![path.display().to_string()],
        ..FerruleApp::default()
    });
    assert!(tab_to(&mut reopened, "orders.json"));
    press(&mut reopened, egui::Key::Enter);
    assert_eq!(project_key(&reopened), edited);
    assert_eq!(bindings(&reopened), [("Name".to_owned(), 0)]);
    assert!(!reopened.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_adds_connections_from_the_inspector_and_mapping_menu() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, "Add binding"));
    press(&mut harness, egui::Key::Enter);
    assert_eq!(harness.state().project.root.bindings.len(), 2);
    assert!(harness.state().observed_dirty());

    let mut harness = editor(&path);
    open_auto_connect_from_menu(&mut harness);
    assert!(tab_to(&mut harness, "Apply connections"));
    press(&mut harness, egui::Key::Enter);
    assert!(harness.state().pending_auto_connect.is_none());
    let connected = bindings(&harness);
    assert_eq!(connected.len(), 2, "{connected:?}");
    assert!(connected.contains(&("Note".to_owned(), 0)));
    let (_, name_node) = connected
        .iter()
        .find(|(field, _)| field == "Name")
        .expect("auto-connect bound Name");
    assert_ne!(*name_node, 0);
    assert!(cli::validate(&harness.state().project).is_empty());
    assert!(harness.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_previews_the_unsaved_mapping() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, PREVIEW_BUTTON));
    press(&mut harness, egui::Key::Enter);
    assert!(harness.state().preview_draft.is_some());
    assert!(tab_to(
        &mut harness,
        "Selects the input format; no file is opened"
    ));
    type_text(&mut harness, "input.json");
    press(&mut harness, egui::Key::Tab);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Selects the output format; no file is written")
    );
    type_text(&mut harness, "output.json");
    press(&mut harness, egui::Key::Tab);
    assert_eq!(focused_name(&harness).as_deref(), Some("Primary input"));
    type_text(&mut harness, r#"{"Name":"Ada","Note":"ignored"}"#);
    assert!(tab_to(&mut harness, "Preview"));
    press(&mut harness, egui::Key::Enter);
    for _ in 0..500 {
        harness.step();
        if harness.state().run_report.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    harness.run_steps(4);
    let report = &harness
        .state()
        .run_report
        .as_ref()
        .expect("preview finished")
        .report;
    assert_eq!(report.kind, crate::run_report::RunReportKind::Preview);
    assert_eq!(report.records_written, 1);
    assert!(harness.state().show_run_report);
    assert!(
        harness
            .query_all_by_label_contains(r#""Note": "Keyboard""#)
            .next()
            .is_some(),
        "preview output is shown"
    );
}

#[test]
fn keyboard_accessibility_gap_selects_a_canvas_node_for_f2() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    for _ in 0..MAX_STOPS {
        press(&mut harness, egui::Key::Tab);
        if focus(&harness).is_some_and(|stop| stop.on_canvas()) {
            press(&mut harness, egui::Key::F2);
            if GraphViewer::properties_open(&harness.ctx) {
                return;
            }
        }
    }
    panic!("no keyboard stop selects the Const node so that F2 opens its settings");
}

#[test]
fn keyboard_accessibility_gap_tab_reaches_every_canvas_control() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let mut visited = std::collections::BTreeSet::new();
    let mut lost = 0;
    for _ in 0..MAX_STOPS {
        press(&mut harness, egui::Key::Tab);
        match focused_name(&harness) {
            Some(name) => {
                visited.insert(name);
            }
            None => lost += 1,
        }
    }
    let missing = [EDIT_CONST, "Zoom out", "Fit all nodes"]
        .into_iter()
        .filter(|name| !visited.contains(*name))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty() && lost == 0,
        "forward Tab missed {missing:?} and left focus on nothing {lost} times"
    );
}

#[test]
fn keyboard_accessibility_gap_shift_tab_leaves_the_canvas() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..MAX_STOPS {
        press_with(&mut harness, egui::Modifiers::SHIFT, egui::Key::Tab);
        visited.extend(focused_name(&harness));
    }
    for name in ["Add binding", PREVIEW_BUTTON, "File"] {
        assert!(
            visited.contains(name),
            "Shift+Tab never reached {name}; visited {visited:?}"
        );
    }
}

#[test]
fn keyboard_accessibility_gap_every_tab_stop_is_visible() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let screen = harness.ctx.content_rect();
    let invisible = stops_before_canvas(&mut harness)
        .into_iter()
        .filter(|stop| {
            stop.as_ref().is_none_or(|stop| {
                !stop.bounds.is_finite()
                    || !stop.bounds.is_positive()
                    || !screen.intersects(stop.bounds)
            })
        })
        .collect::<Vec<_>>();
    assert!(
        invisible.is_empty(),
        "focus without visible bounds: {invisible:?}"
    );
}

#[test]
fn keyboard_accessibility_gap_escape_closes_modal_dialogs() {
    let (_scratch, path) = saved_project();
    let mut failures = Vec::new();

    let mut harness = editor(&path);
    retarget_binding_to_name(&mut harness);
    let edited = project_key(&harness);
    open_shortcut_on_unsaved_work(&mut harness);
    press(&mut harness, egui::Key::Escape);
    if harness.state().pending_destructive_action.is_some() {
        failures.push("Unsaved changes stayed open");
    }
    assert!(harness.state().observed_dirty());
    assert_eq!(project_key(&harness), edited);

    let mut harness = editor(&path);
    open_auto_connect_from_menu(&mut harness);
    press(&mut harness, egui::Key::Escape);
    if harness.state().pending_auto_connect.is_some() {
        failures.push("Auto-connect fields stayed open");
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn keyboard_accessibility_gap_opening_a_dialog_focuses_it() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    retarget_binding_to_name(&mut harness);
    assert!(tab_to(&mut harness, "Open (Ctrl+O)"));
    assert!(harness.state().observed_dirty());
    press(&mut harness, egui::Key::Enter);
    assert_eq!(
        harness.state().pending_destructive_action,
        Some(DestructiveAction::OpenProject)
    );
    let focused = focused_name(&harness);
    assert!(
        focused
            .as_deref()
            .is_some_and(|name| UNSAVED_DIALOG_BUTTONS.contains(&name)),
        "focus after opening Unsaved changes: {focused:?}"
    );
}

#[test]
fn keyboard_accessibility_gap_closing_a_dialog_restores_focus() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    retarget_binding_to_name(&mut harness);
    assert!(tab_to(&mut harness, "Open (Ctrl+O)"));
    assert!(harness.state().observed_dirty());
    press(&mut harness, egui::Key::Enter);
    assert!(tab_to(&mut harness, "Cancel"));
    press(&mut harness, egui::Key::Enter);
    assert_eq!(harness.state().pending_destructive_action, None);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Open (Ctrl+O)"),
        "focus returns to the control that opened the dialog"
    );
}

#[test]
fn keyboard_accessibility_gap_dialog_footer_tab_order_is_visual() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    retarget_binding_to_name(&mut harness);
    open_shortcut_on_unsaved_work(&mut harness);
    let mut order = Vec::new();
    for _ in 0..4 {
        press(&mut harness, egui::Key::Tab);
        order.extend(focused_name(&harness));
    }
    // The footer shows Discard changes on the left, then Cancel and Save.
    assert_eq!(order, ["Close", "Discard changes", "Cancel", "Save"]);
}

fn open_preview_input(harness: &mut Harness<'_, FerruleApp>) {
    assert!(tab_to(harness, PREVIEW_BUTTON));
    press(harness, egui::Key::Enter);
    assert!(tab_to(harness, "Primary input"));
}

fn preview_input(harness: &Harness<'_, FerruleApp>) -> Option<String> {
    harness
        .state()
        .preview_draft
        .as_ref()
        .map(|draft| draft.input_text.clone())
}

#[test]
fn keyboard_accessibility_gap_tab_leaves_the_preview_input() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    open_preview_input(&mut harness);
    type_text(&mut harness, "{}");
    press(&mut harness, egui::Key::Tab);
    let input = preview_input(&harness);
    assert_eq!(input.as_deref(), Some("{}"), "Tab edited the input");
    assert_ne!(focused_name(&harness).as_deref(), Some("Primary input"));
}

#[test]
fn keyboard_accessibility_ctrl_tab_types_a_tab_in_the_preview_input() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    open_preview_input(&mut harness);
    type_text(&mut harness, "Name");
    press_with(&mut harness, egui::Modifiers::CTRL, egui::Key::Tab);
    type_text(&mut harness, "Note");
    assert_eq!(preview_input(&harness).as_deref(), Some("Name\tNote"));
    assert_eq!(focused_name(&harness).as_deref(), Some("Primary input"));
}

/// Collects named Tab stops forward to the canvas, then backward from the
/// end of the Tab order.
fn tab_stops(path: &Path) -> Vec<Stop> {
    let mut harness = editor(path);
    let mut stops = stops_before_canvas(&mut harness)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut harness = editor(path);
    for _ in 0..30 {
        press_with(&mut harness, egui::Modifiers::SHIFT, egui::Key::Tab);
        stops.extend(focus(&harness));
    }
    stops
}

/// Stops without a readable name; icon glyphs from the private use area are
/// not readable names.
fn unnamed(stops: impl IntoIterator<Item = Stop>) -> Vec<String> {
    let readable = |name: &str| {
        name.chars()
            .any(|c| c.is_alphanumeric() && !('\u{e000}'..='\u{f8ff}').contains(&c))
    };
    let mut unnamed = stops
        .into_iter()
        .filter(|stop| !readable(&stop.name))
        .map(|stop| format!("{} {:?} at {:?}", stop.role, stop.name, stop.bounds))
        .collect::<Vec<_>>();
    unnamed.sort();
    unnamed.dedup();
    unnamed
}

#[test]
fn keyboard_accessibility_gap_tab_stops_have_readable_names() {
    let (_scratch, path) = saved_project();
    let unnamed = unnamed(
        tab_stops(&path)
            .into_iter()
            .filter(|stop| !stop.on_canvas()),
    );
    assert!(unnamed.is_empty(), "unnamed Tab stops: {unnamed:#?}");
}

#[test]
fn keyboard_accessibility_gap_canvas_tab_stops_have_readable_names() {
    let (_scratch, path) = saved_project();
    let unnamed = unnamed(tab_stops(&path).into_iter().filter(Stop::on_canvas));
    assert!(unnamed.is_empty(), "unnamed canvas Tab stops: {unnamed:#?}");
}

#[test]
fn keyboard_accessibility_gap_add_binding_keeps_focus() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, "Add binding"));
    press(&mut harness, egui::Key::Enter);
    assert_eq!(
        bindings(&harness),
        [("Note".to_owned(), 0), ("Name".to_owned(), 0)]
    );
    // Focus moves to the new binding's target field, the next control to
    // set. The new binding takes the last unbound field, which disables Add
    // binding, and a disabled control cannot hold focus.
    assert_eq!(focused_name(&harness).as_deref(), Some("Name"));
    press(&mut harness, egui::Key::Tab);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some(r#"Text · "Keyboard" · #0"#),
        "Tab continues to the new binding's expression"
    );
}

#[test]
fn keyboard_accessibility_gap_preview_has_a_shortcut() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, "Run"));
    press(&mut harness, egui::Key::Enter);
    let item = tab_until(&mut harness, |name| name.starts_with("Preview…"))
        .expect("Run menu lists Preview");
    // Menu items append their shortcut, as in "Run Ctrl+R".
    assert_ne!(item, "Preview…", "Preview advertises no shortcut");
}

#[test]
fn keyboard_accessibility_preview_shortcut_opens_the_preview() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    press_with(
        &mut harness,
        command() | egui::Modifiers::SHIFT,
        egui::Key::P,
    );
    assert!(harness.state().preview_draft.is_some());
}

const SOURCE_NAME_PIN: &str = "Output Name · Source: Order";
const TARGET_NAME_PIN: &str = "Input Name · Target: Order";

#[test]
fn keyboard_accessibility_connects_two_fields_on_the_canvas() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let original = project_key(&harness);
    assert!(
        tab_to(&mut harness, SOURCE_NAME_PIN),
        "source pin is a Tab stop"
    );
    press(&mut harness, egui::Key::Enter);
    assert_eq!(bindings(&harness), [("Note".to_owned(), 0)]);
    assert!(
        tab_to(&mut harness, TARGET_NAME_PIN),
        "target pin is a Tab stop"
    );
    press(&mut harness, egui::Key::Enter);

    let connected = bindings(&harness);
    assert_eq!(connected.len(), 2, "{connected:?}");
    let (_, name_node) = connected
        .iter()
        .find(|(field, _)| field == "Name")
        .expect("the keyboard wire bound Name");
    assert!(matches!(
        harness.state().project.graph.nodes.get(name_node),
        Some(Node::SourceField { .. })
    ));
    assert!(cli::validate(&harness.state().project).is_empty());
    assert!(harness.state().observed_dirty());
    assert_eq!(focused_name(&harness).as_deref(), Some(TARGET_NAME_PIN));

    press_with(&mut harness, command(), egui::Key::Z);
    assert_eq!(project_key(&harness), original);
}

#[test]
fn keyboard_accessibility_escape_cancels_a_canvas_connection() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let original = project_key(&harness);
    assert!(tab_to(&mut harness, SOURCE_NAME_PIN));
    press(&mut harness, egui::Key::Enter);
    press(&mut harness, egui::Key::Escape);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some(SOURCE_NAME_PIN),
        "Escape returns focus to the pin the connection started from"
    );
    // With the connection cancelled, Enter on the input starts a new one.
    assert!(tab_to(&mut harness, TARGET_NAME_PIN));
    press(&mut harness, egui::Key::Enter);
    assert_eq!(project_key(&harness), original);
    press(&mut harness, egui::Key::Escape);
    assert_eq!(focused_name(&harness).as_deref(), Some(TARGET_NAME_PIN));
    assert!(!harness.state().observed_dirty());
}
