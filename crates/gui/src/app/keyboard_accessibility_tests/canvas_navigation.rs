//! Arrow keys, scrolling, resizing, panning and collapse on the canvas, from
//! the keyboard only.

use super::*;
use crate::canvas::CanvasNode;

const SOURCE: CanvasNode = CanvasNode::SourceBlock(0);
const FIELDS: usize = 20;

fn saved(project: &Project) -> (Scratch, PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-keyboard-canvas-{}-{index}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("orders.json");
    std::fs::write(
        &path,
        mapping::project_file::encode_pretty(project).unwrap(),
    )
    .unwrap();
    (Scratch(directory), path)
}

/// Order with more fields than an endpoint shows at once.
fn wide_project() -> Project {
    let mut project = order_project();
    let schema = SchemaNode::group(
        "Order",
        (0..FIELDS)
            .map(|field| SchemaNode::scalar(format!("F{field:02}"), ScalarType::String))
            .collect(),
    );
    project.source = schema.clone();
    project.target = schema;
    project.root.bindings[0].target_field = "F00".into();
    project
}

/// Order with Note computed by a concat node, which has pins on both sides.
fn concat_project() -> Project {
    let mut project = order_project();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 0],
        },
    );
    project.root.bindings[0].node = 1;
    project
}

fn shift_tab_to(harness: &mut Harness<'_, FerruleApp>, wanted: &str) -> bool {
    for _ in 0..MAX_STOPS {
        press_with(harness, egui::Modifiers::SHIFT, egui::Key::Tab);
        if focused_name(harness).as_deref() == Some(wanted) {
            return true;
        }
    }
    false
}

fn tab_to_stop(harness: &mut Harness<'_, FerruleApp>, wanted: &str) -> Stop {
    assert!(tab_to(harness, wanted), "{wanted} is a Tab stop");
    focus(harness).expect("a Tab stop has focus")
}

fn press_times(harness: &mut Harness<'_, FerruleApp>, key: egui::Key, times: usize) {
    for _ in 0..times {
        press(harness, key);
    }
}

/// A canvas control's bounds on screen.
fn on_screen(harness: &Harness<'_, FerruleApp>, stop: &Stop) -> egui::Rect {
    let rect = harness
        .ctx
        .read_response(stop.id)
        .expect("the control is drawn")
        .rect;
    harness
        .ctx
        .layer_transform_to_global(stop.layer)
        .map_or(rect, |transform| transform * rect)
}

/// The visible canvas on screen: the surface's clipped interaction bounds.
fn viewport(harness: &Harness<'_, FerruleApp>, surface: &Stop) -> egui::Rect {
    let rect = harness
        .ctx
        .read_response(surface.id)
        .expect("the canvas is drawn")
        .interact_rect;
    harness
        .ctx
        .layer_transform_to_global(surface.layer)
        .map_or(rect, |transform| transform * rect)
}

fn source_offset(harness: &Harness<'_, FerruleApp>) -> usize {
    harness
        .state()
        .main_canvas
        .endpoint_scroll
        .offset(SOURCE, FIELDS)
}

fn node_open(harness: &Harness<'_, FerruleApp>, node: CanvasNode) -> bool {
    let snarl = &harness.state().main_canvas.snarl;
    let (id, _) = snarl
        .node_ids()
        .find(|(_, candidate)| **candidate == node)
        .expect("the node is on the canvas");
    snarl.get_node_info(id).expect("node info").open
}

#[test]
fn keyboard_accessibility_arrows_move_between_canvas_nodes_in_layout_order() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, "Source: Order"));
    let mut visited = Vec::new();
    for key in [
        egui::Key::ArrowRight,
        egui::Key::ArrowRight,
        egui::Key::ArrowRight,
        egui::Key::ArrowLeft,
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
        egui::Key::ArrowLeft,
    ] {
        press(&mut harness, key);
        visited.extend(focused_name(&harness));
    }
    assert_eq!(
        visited,
        [
            "Const: Keyboard",
            "Target: Order",
            "Target: Order",
            "Const: Keyboard",
            "Const: Keyboard",
            "Const: Keyboard",
            "Source: Order",
        ]
    );

    // A node's header controls move between nodes as its frame does.
    assert!(tab_to(&mut harness, "Collapse Const: Keyboard"));
    press(&mut harness, egui::Key::ArrowRight);
    assert_eq!(focused_name(&harness).as_deref(), Some("Target: Order"));
    assert!(tab_to(&mut harness, EDIT_CONST));
    press(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(focused_name(&harness).as_deref(), Some("Source: Order"));

    // The surface takes arrows without moving focus.
    assert!(tab_to(&mut harness, "Mapping canvas"));
    for key in [
        egui::Key::ArrowDown,
        egui::Key::ArrowRight,
        egui::Key::ArrowUp,
        egui::Key::ArrowLeft,
    ] {
        press(&mut harness, key);
        assert_eq!(focused_name(&harness).as_deref(), Some("Mapping canvas"));
    }
    assert!(!harness.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_arrows_move_between_pins_of_a_node() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, SOURCE_NAME_PIN));
    let mut visited = Vec::new();
    for key in [
        egui::Key::ArrowDown,
        egui::Key::ArrowDown,
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
        egui::Key::ArrowUp,
        egui::Key::ArrowUp,
    ] {
        press(&mut harness, key);
        visited.extend(focused_name(&harness));
    }
    let note = "Output Note · Source: Order";
    assert_eq!(
        visited,
        [note, note, note, note, SOURCE_NAME_PIN, SOURCE_NAME_PIN]
    );

    let (_scratch, path) = saved(&concat_project());
    let mut harness = editor(&path);
    // Variadic inputs share a name, so these steps compare widgets.
    let concat = |name: &str| name.starts_with("Input ") && name.ends_with(" · Call: Concat");
    assert!(tab_until(&mut harness, concat).is_some());
    let first = focus(&harness).expect("the first input has focus");
    press(&mut harness, egui::Key::ArrowUp);
    assert_eq!(focus(&harness).map(|stop| stop.id), Some(first.id));
    press(&mut harness, egui::Key::ArrowDown);
    let second = focus(&harness).expect("focus stays on the canvas");
    assert!(concat(&second.name) && second.id != first.id, "{second:?}");
    assert!(on_screen(&harness, &second).top() > on_screen(&harness, &first).top());
    press(&mut harness, egui::Key::ArrowRight);
    let output = focus(&harness).expect("focus stays on the canvas");
    assert_eq!(output.name, "Output · Call: Concat");
    press(&mut harness, egui::Key::ArrowRight);
    assert_eq!(focus(&harness).map(|stop| stop.id), Some(output.id));
    press(&mut harness, egui::Key::ArrowLeft);
    let back = focus(&harness).expect("focus stays on the canvas");
    assert!(back.id == first.id || back.id == second.id, "{back:?}");
    assert!(!harness.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_arrows_scroll_hidden_endpoint_fields_into_view() {
    let (_scratch, path) = saved(&wide_project());
    let mut harness = editor(&path);
    assert!(tab_to(&mut harness, "Output F00 · Source: Order"));
    press_times(&mut harness, egui::Key::ArrowDown, 11);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Output F11 · Source: Order")
    );
    assert_eq!(source_offset(&harness), 0);

    // The next field is behind the hidden-fields pin: the list scrolls.
    press(&mut harness, egui::Key::ArrowDown);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Output F12 · Source: Order")
    );
    assert_eq!(source_offset(&harness), 1);
    press_times(&mut harness, egui::Key::ArrowDown, 7);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Output F19 · Source: Order")
    );
    assert_eq!(source_offset(&harness), 8);

    press_times(&mut harness, egui::Key::ArrowUp, 19);
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Output F00 · Source: Order")
    );
    assert_eq!(source_offset(&harness), 0);
    assert!(!harness.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_endpoint_scrollbar_takes_scroll_keys() {
    let (_scratch, path) = saved(&wide_project());
    let mut harness = editor(&path);
    let scrollbar = "Scroll fields · Source: Order";
    assert!(tab_to(&mut harness, scrollbar));
    let mut offsets = Vec::new();
    for key in [
        egui::Key::ArrowDown,
        egui::Key::PageDown,
        egui::Key::ArrowUp,
        egui::Key::Home,
        egui::Key::End,
        egui::Key::PageUp,
    ] {
        press(&mut harness, key);
        offsets.push(source_offset(&harness));
        assert_eq!(focused_name(&harness).as_deref(), Some(scrollbar));
    }
    assert_eq!(offsets, [1, 8, 7, 0, 8, 0]);
    assert!(!harness.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_endpoint_resize_handles_take_arrow_keys() {
    let (_scratch, path) = saved(&wide_project());
    let mut harness = editor(&path);
    let width =
        |harness: &Harness<'_, FerruleApp>| harness.state().main_canvas.node_sizes[&SOURCE].x;
    let rows = |harness: &Harness<'_, FerruleApp>| {
        harness
            .state()
            .main_canvas
            .endpoint_scroll
            .visible_limit(SOURCE, FIELDS)
    };

    let handle = "Resize width · Source: Order";
    assert!(tab_to(&mut harness, handle));
    let original = width(&harness);
    // The source's width handle is on its left edge: Left widens it.
    press(&mut harness, egui::Key::ArrowLeft);
    let wider = width(&harness);
    assert!(
        (wider - original - 10.0).abs() < 0.5,
        "{original} -> {wider}"
    );
    press_times(&mut harness, egui::Key::ArrowRight, 2);
    let narrower = width(&harness);
    assert!(
        (wider - narrower - 20.0).abs() < 0.5,
        "{wider} -> {narrower}"
    );
    press(&mut harness, egui::Key::ArrowDown);
    assert_eq!(focused_name(&harness).as_deref(), Some(handle));
    assert!((width(&harness) - narrower).abs() < 0.5);

    let handle = "Resize visible fields · Source: Order";
    assert!(tab_to(&mut harness, handle));
    assert_eq!(rows(&harness), 12);
    press(&mut harness, egui::Key::ArrowDown);
    assert_eq!(rows(&harness), 13);
    press_times(&mut harness, egui::Key::ArrowUp, 3);
    assert_eq!(rows(&harness), 10);
    press(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(focused_name(&harness).as_deref(), Some(handle));
    assert_eq!(rows(&harness), 10);
    assert!(!harness.state().observed_dirty());

    // The unnamed corner grip, which combines both handles, is not a Tab stop.
    let mut stops = Vec::new();
    for _ in 0..MAX_STOPS {
        press(&mut harness, egui::Key::Tab);
        stops.extend(focus(&harness).filter(Stop::on_canvas));
    }
    let unnamed = unnamed(stops);
    assert!(unnamed.is_empty(), "unnamed canvas Tab stops: {unnamed:#?}");
}

#[test]
fn keyboard_accessibility_focus_pans_to_an_off_screen_node() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let surface = tab_to_stop(&mut harness, "Mapping canvas");
    let source = tab_to_stop(&mut harness, "Source: Order");
    let target = tab_to_stop(&mut harness, "Target: Order");
    assert!(tab_to(&mut harness, "Zoom in"));
    press_times(&mut harness, egui::Key::Enter, 10);
    let view = viewport(&harness, &surface);
    assert!(view.is_positive(), "{view:?}");
    assert!(
        !view.contains_rect(on_screen(&harness, &source)),
        "zooming in moves the source node out of view"
    );
    assert!(!view.contains_rect(on_screen(&harness, &target)));

    // Shift+Tab back through the canvas lands on the source node in view.
    assert!(shift_tab_to(&mut harness, "Source: Order"));
    assert!(
        view.contains_rect(on_screen(&harness, &source)),
        "{view:?} {:?}",
        on_screen(&harness, &source)
    );
    assert!(!view.contains_rect(on_screen(&harness, &target)));

    // Arrow keys reveal the node they move to as well.
    press_times(&mut harness, egui::Key::ArrowRight, 2);
    assert_eq!(focused_name(&harness).as_deref(), Some("Target: Order"));
    assert!(view.contains_rect(on_screen(&harness, &target)));
    assert!(!harness.state().observed_dirty());
}

#[test]
fn keyboard_accessibility_enter_and_space_toggle_collapse() {
    let (_scratch, path) = saved_project();
    let mut harness = editor(&path);
    let constant = CanvasNode::Graph(0);
    assert!(node_open(&harness, constant));
    assert!(tab_to(&mut harness, "Collapse Const: Keyboard"));
    press(&mut harness, egui::Key::Enter);
    assert!(!node_open(&harness, constant));
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Expand Const: Keyboard")
    );
    press(&mut harness, egui::Key::Space);
    assert!(node_open(&harness, constant));
    assert_eq!(
        focused_name(&harness).as_deref(),
        Some("Collapse Const: Keyboard")
    );
    assert!(!harness.state().observed_dirty());
}
