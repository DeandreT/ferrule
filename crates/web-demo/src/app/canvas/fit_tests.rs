use super::super::{DemoApp, WorkspaceView, project_document, sample};
use super::*;

fn finite(transform: egui::emath::TSTransform) -> bool {
    transform.scaling.is_finite()
        && transform.scaling >= MIN_CANVAS_SCALE
        && transform.scaling <= 1.0
        && transform.translation.is_finite()
}

#[test]
fn fit_uses_complete_rectangles_screen_margin_and_finite_extremes() {
    let wide = egui::Rect::from_min_size(egui::pos2(300.0, 80.0), egui::vec2(600.0, 500.0));
    let compact = egui::Rect::from_min_size(egui::pos2(8.0, 140.0), egui::vec2(744.0, 630.0));
    let cases = [
        (
            Some(egui::Rect::from_min_max(
                egui::pos2(-20.0, -40.0),
                egui::pos2(920.0, 640.0),
            )),
            wide,
        ),
        (
            Some(egui::Rect::from_min_max(
                egui::pos2(20.0, 30.0),
                egui::pos2(800.0, 700.0),
            )),
            compact,
        ),
        (
            Some(egui::Rect::from_min_size(
                egui::pos2(100.0, -200.0),
                egui::Vec2::ZERO,
            )),
            wide,
        ),
        (
            Some(egui::Rect::from_min_max(
                egui::pos2(-1.0e20, -1.0e20),
                egui::pos2(1.0e20, 1.0e20),
            )),
            wide,
        ),
        (
            Some(egui::Rect::from_min_size(
                egui::pos2(1.0e20, 1.0e20),
                egui::Vec2::ZERO,
            )),
            wide,
        ),
        (None, wide),
    ];
    let originals = cases.map(|(bounds, viewport)| fit_transform(bounds, viewport));
    eprintln!("browser Fit complete geometry original: cases={cases:?}; transforms={originals:?}");
    for ((bounds, viewport), transform) in cases.into_iter().zip(originals) {
        let transform = transform.expect("finite authored viewport and bounds");
        assert!(finite(transform));
        assert!(transform.inverse().scaling.is_finite());
        assert!(transform.inverse().translation.is_finite());
        assert!((transform.inverse() * viewport).is_finite());
        if let Some(bounds) = bounds {
            assert!(
                viewport.shrink(10.0).contains_rect(transform * bounds),
                "{bounds:?} -> {transform:?}"
            );
        } else {
            assert!((transform * egui::Pos2::ZERO - viewport.center()).length() < 0.01);
        }
    }
    let actual = originals[0].unwrap();
    assert!(
        (actual.scaling - 576.0 / 940.0).abs() < 0.001,
        "full 940-unit width, not anchor span"
    );
    assert!(
        originals[3].unwrap().scaling < 0.2,
        "large finite bounds can fit below the widget's old minimum"
    );
    let invalid = [
        (Some(egui::Rect::NAN), wide),
        (
            Some(egui::Rect::from_min_max(
                egui::pos2(-f32::MAX, -f32::MAX),
                egui::pos2(f32::MAX, f32::MAX),
            )),
            wide,
        ),
        (
            Some(egui::Rect::from_min_size(
                egui::pos2(f32::MAX, f32::MAX),
                egui::Vec2::ZERO,
            )),
            wide,
        ),
        (
            Some(egui::Rect::from_min_max(
                egui::pos2(5.0, 0.0),
                egui::pos2(0.0, 5.0),
            )),
            wide,
        ),
        (None, egui::Rect::NOTHING),
        (
            None,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO),
        ),
    ];
    let originals = invalid.map(|(bounds, viewport)| fit_transform(bounds, viewport));
    eprintln!("browser Fit invalid geometry original: inputs={invalid:?}; outcomes={originals:?}");
    assert!(originals.into_iter().all(|result| result.is_none()));
}

#[test]
fn measured_fit_requires_every_current_node_rectangle() {
    let mut snarl = Snarl::new();
    let left = snarl.insert_node(egui::pos2(20.0, 30.0), CanvasNode::Graph(0));
    let target = snarl.insert_node(egui::pos2(330.0, 60.0), CanvasNode::Target);
    let left_rect = egui::Rect::from_min_size(egui::pos2(14.0, 24.0), egui::vec2(120.0, 50.0));
    let target_rect = egui::Rect::from_min_size(egui::pos2(324.0, 54.0), egui::vec2(440.0, 200.0));
    let complete: BTreeMap<_, _> = [(left, left_rect), (target, target_rect)].into();
    let incomplete = [(left, left_rect)].into();
    let invalid = [(left, left_rect), (target, egui::Rect::NAN)].into();
    let results = [
        measured_bounds(&snarl, &complete),
        measured_bounds(&snarl, &incomplete),
        measured_bounds(&snarl, &invalid),
    ];
    let empty = measured_bounds(&Snarl::new(), &BTreeMap::new());
    eprintln!(
        "browser Fit measured originals: complete={complete:?}; partial={incomplete:?}; invalid={invalid:?}; outcomes={results:?}; empty={empty:?}"
    );
    assert_eq!(
        results,
        [Some(Some(left_rect.union(target_rect))), None, None]
    );
    assert_eq!(empty, Some(None));
}

#[test]
fn fit_stabilizes_then_keeps_manual_view_and_same_layout_resize() {
    let mut snarl = Snarl::new();
    let node = snarl.insert_node(egui::pos2(330.0, 60.0), CanvasNode::Target);
    let rect = egui::Rect::from_min_size(egui::pos2(324.0, 54.0), egui::vec2(440.0, 200.0));
    let rectangles: BTreeMap<_, _> = [(node, rect)].into();
    let viewport = egui::Rect::from_min_size(egui::pos2(300.0, 80.0), egui::vec2(600.0, 500.0));
    let mut view = CanvasView::default();
    let initial = view.prepare(&snarl, viewport);
    let measuring = view.finish(
        &snarl,
        rectangles.clone(),
        viewport,
        Some(egui::emath::TSTransform::IDENTITY),
    );
    let measured = view.prepare(&snarl, viewport);
    let completed = view.finish(&snarl, rectangles.clone(), viewport, measured);
    let resized = viewport.shrink2(egui::vec2(20.0, 10.0));
    let idle = view.prepare(&snarl, resized);
    let manual = egui::emath::TSTransform {
        scaling: 0.8,
        translation: egui::vec2(-120.0, 40.0),
    };
    let manual_repaint = view.finish(&snarl, rectangles.clone(), resized, Some(manual));
    let retained = view.transform;
    view.request_fit();
    let explicit = view.prepare(&snarl, resized);
    eprintln!(
        "browser Fit state originals: initial={initial:?}; measuring={measuring}; measured={measured:?}; complete_repaint={completed}; resized_idle={idle:?}; manual_repaint={manual_repaint}; retained={retained:?}; explicit={explicit:?}; view={view:?}"
    );
    assert!(initial.is_none() && measuring && !completed);
    assert!(finite(measured.unwrap()));
    assert!(idle.is_none() && !manual_repaint);
    assert_eq!(retained, Some(manual));
    assert!(finite(explicit.unwrap()));
    let mut unstable = CanvasView::default();
    let outcomes: Vec<_> = (0..MAX_FIT_PASSES)
        .map(|_| unstable.finish(&snarl, rectangles.clone(), viewport, None))
        .collect();
    eprintln!(
        "browser Fit finite stabilization budget original: outcomes={outcomes:?}; view={unstable:?}"
    );
    assert_eq!(outcomes.len(), usize::from(MAX_FIT_PASSES));
    assert_eq!(outcomes.last(), Some(&false));
    assert!(unstable.prepare(&snarl, viewport).is_none());
}

fn project(value: &str, incomplete: bool) -> mapping::Project {
    let mut project = sample::demo_project();
    project.source = ir::SchemaNode::group(
        "Input",
        vec![ir::SchemaNode::scalar("Unused", ir::ScalarType::String)],
    );
    let names: &[&str] = if incomplete {
        &["Value", "Missing", "Unconnected", "Again"]
    } else {
        &["Value"]
    };
    project.target = ir::SchemaNode::group(
        "Result",
        names
            .iter()
            .map(|name| ir::SchemaNode::scalar(*name, ir::ScalarType::String))
            .collect(),
    );
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph = Graph::default();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String(value.into()),
        },
    );
    if incomplete {
        project.graph.nodes.insert(2, Node::Unconnected);
    }
    project.root = Scope {
        bindings: if incomplete {
            [
                ("Value", 0),
                ("Missing", 999),
                ("Unconnected", 2),
                ("Again", 0),
            ]
            .into_iter()
            .map(|(field, node)| mapping::Binding {
                target_field: field.into(),
                node,
            })
            .collect()
        } else {
            vec![mapping::Binding {
                target_field: "Value".into(),
                node: 0,
            }]
        },
        ..Scope::default()
    };
    project
}

fn nodes(app: &DemoApp) -> Vec<(CanvasNodeId, String, egui::Pos2, bool)> {
    app.snarl
        .node_ids()
        .map(|(id, node)| {
            let info = app.snarl.get_node_info(id).unwrap();
            let name = match node {
                CanvasNode::Graph(node) => format!("Graph({node})"),
                CanvasNode::Target => "Target".into(),
            };
            (id, name, info.pos, info.open)
        })
        .collect()
}

fn wires(app: &DemoApp) -> Vec<(egui_snarl::OutPinId, egui_snarl::InPinId)> {
    let mut wires: Vec<_> = app.snarl.wires().collect();
    wires.sort_by_key(|(from, to)| (from.node, from.output, to.node, to.input));
    wires
}

fn frame(
    app: &mut DemoApp,
    context: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        time: Some(context.cumulative_frame_nr() as f64 / 60.0),
        events,
        ..Default::default()
    };
    eprintln!("browser Fit actual frame input original: {input:#?}");
    let output = context.run_ui(input, |ui| app.show_workspace(ui));
    eprintln!(
        "browser Fit actual frame original: project={:?}; bindings={:?}; nodes={:?}; wires={:?}; history={:?}; view={:?}; shapes={:#?}",
        serde_json::to_value(&app.project),
        app.bindings,
        nodes(app),
        wires(app),
        app.history.retained(),
        app.canvas_view,
        output.shapes
    );
    output
}

fn settle(app: &mut DemoApp, context: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
    let mut output = frame(app, context, size, Vec::new());
    for _ in 0..15 {
        output = frame(app, context, size, Vec::new());
    }
    output
}

fn button_point(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        label: &str,
        points: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.visual_bounding_rect();
                if clip.contains_rect(rect) {
                    points.push(rect.center());
                }
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, label, points);
                }
            }
            _ => {}
        }
    }
    let mut points = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, label, &mut points);
    }
    eprintln!("browser Fit actual control original: label={label:?}; points={points:?}");
    assert_eq!(points.len(), 1, "one complete actual painted control");
    points[0]
}

fn click(app: &mut DemoApp, context: &egui::Context, size: egui::Vec2, label: &str) {
    let output = settle(app, context, size);
    let pos = button_point(&output, label);
    frame(app, context, size, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            context,
            size,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

fn contained(app: &DemoApp) {
    let viewport = app
        .canvas_view
        .viewport
        .expect("actual measured canvas viewport");
    let transform = app.canvas_view.transform.expect("actual widget transform");
    let bounds = measured_bounds(&app.snarl, &app.canvas_view.rectangles);
    eprintln!(
        "browser Fit complete measured containment original: viewport={viewport:?}; transform={transform:?}; nodes={:?}; full_rectangles={:?}; complete_bounds={bounds:?}; passes={}",
        nodes(app),
        app.canvas_view.rectangles,
        app.canvas_view.fit_passes
    );
    assert!(finite(transform));
    assert_eq!(app.canvas_view.fit_passes, 0, "settled finite Fit");
    let bounds = bounds.expect("all actual current node rectangles measured");
    if let Some(bounds) = bounds {
        assert!(
            viewport.shrink(9.0).contains_rect(transform * bounds),
            "{viewport:?}, {transform:?}, {bounds:?}"
        );
    }
    for rect in app.canvas_view.rectangles.values() {
        assert!(viewport.contains_rect(transform * *rect));
    }
}

#[test]
fn demo_ui_fit_contains_sample_and_incomplete_custom_nodes_without_model_or_position_edits() {
    for size in [egui::vec2(1200.0, 850.0), egui::vec2(760.0, 850.0)] {
        for incomplete in [false, true] {
            let mut app = DemoApp::new();
            if incomplete {
                app.install_project(project("one", true));
            }
            app.live_run = false;
            app.run_pending = false;
            let context = egui::Context::default();
            settle(&mut app, &context, size);
            contained(&app);
            let viewport = app.canvas_view.viewport.unwrap();
            if size.x > 900.0 {
                assert!(
                    viewport.width() < size.x - 400.0,
                    "actual side panels excluded"
                );
            } else {
                assert!(
                    viewport.width() > size.x - 80.0,
                    "actual compact mapping viewport"
                );
            }
            let target = app
                .snarl
                .node_ids()
                .find_map(|(id, node)| matches!(node, CanvasNode::Target).then_some(id))
                .unwrap();
            app.snarl.get_node_info_mut(target).unwrap().pos = egui::pos2(850.0, 600.0);
            let original_model = serde_json::to_value(&app.project).unwrap();
            let original_nodes = nodes(&app);
            let original_wires = wires(&app);
            let original_bindings = app.bindings.clone();
            let original_history = app.history.retained();
            let generation = app.canvas_view_generation;
            click(&mut app, &context, size, "Fit");
            settle(&mut app, &context, size);
            eprintln!(
                "browser Fit identity originals before compare: wanted_project={original_model:?}; actual_project={:?}; wanted_nodes={original_nodes:?}; actual_nodes={:?}; wanted_wires={original_wires:?}; actual_wires={:?}",
                serde_json::to_value(&app.project),
                nodes(&app),
                wires(&app)
            );
            contained(&app);
            assert_eq!(serde_json::to_value(&app.project).unwrap(), original_model);
            assert_eq!(nodes(&app), original_nodes);
            assert_eq!(wires(&app), original_wires);
            assert_eq!(app.bindings, original_bindings);
            assert_eq!(app.history.retained(), original_history);
            assert_eq!(
                app.canvas_view_generation, generation,
                "Fit changes only the transform"
            );
            if incomplete {
                assert_eq!(
                    app.bindings
                        .iter()
                        .map(|(_, node)| *node)
                        .collect::<Vec<_>>(),
                    vec![0, 999, 2, 0]
                );
                assert_eq!(original_wires.len(), 2);
            }
        }
    }
}

#[test]
fn demo_ui_fit_is_available_after_apply_undo_redo_reset_and_same_layout_resize() {
    let mut app = DemoApp::new();
    app.live_run = false;
    app.run_pending = false;
    let context = egui::Context::default();
    let wide = egui::vec2(1200.0, 850.0);
    let compact = egui::vec2(760.0, 850.0);
    settle(&mut app, &context, wide);
    app.project_json = project_document::to_json(&project("one", false)).unwrap();
    app.project_json_dirty = true;
    app.active_view = WorkspaceView::Project;
    click(&mut app, &context, wide, "Apply");
    settle(&mut app, &context, wide);
    contained(&app);
    let applied = serde_json::to_value(&app.project).unwrap();
    click(&mut app, &context, wide, "Undo");
    settle(&mut app, &context, wide);
    contained(&app);
    assert_eq!(
        serde_json::to_value(&app.project).unwrap(),
        serde_json::to_value(sample::demo_project()).unwrap()
    );
    click(&mut app, &context, wide, "Redo");
    settle(&mut app, &context, wide);
    contained(&app);
    assert_eq!(serde_json::to_value(&app.project).unwrap(), applied);
    let before_resize = app.canvas_view.transform;
    settle(&mut app, &context, egui::vec2(1180.0, 850.0));
    let after_resize = app.canvas_view.transform;
    eprintln!(
        "browser Fit ordinary resize original: before={before_resize:?}; after={after_resize:?}; viewport={:?}",
        app.canvas_view.viewport
    );
    assert_eq!(
        after_resize, before_resize,
        "ordinary resize keeps the settled manual transform"
    );
    click(&mut app, &context, egui::vec2(1180.0, 850.0), "Fit");
    settle(&mut app, &context, egui::vec2(1180.0, 850.0));
    contained(&app);
    settle(&mut app, &context, compact);
    contained(&app);
    click(&mut app, &context, compact, "Reset");
    settle(&mut app, &context, compact);
    contained(&app);
    assert_eq!(
        serde_json::to_value(&app.project).unwrap(),
        serde_json::to_value(sample::demo_project()).unwrap()
    );
}

#[test]
fn demo_ui_fit_empty_and_single_target_canvases_keep_finite_views() {
    for single in [false, true] {
        let mut app = DemoApp::new();
        app.install_project(project("one", false));
        app.snarl = Snarl::new();
        if single {
            app.snarl
                .insert_node(egui::pos2(-200.0, 100.0), CanvasNode::Target);
        }
        app.live_run = false;
        app.run_pending = false;
        let original_model = serde_json::to_value(&app.project).unwrap();
        let original_nodes = nodes(&app);
        let context = egui::Context::default();
        let size = egui::vec2(1200.0, 850.0);
        settle(&mut app, &context, size);
        click(&mut app, &context, size, "Fit");
        settle(&mut app, &context, size);
        contained(&app);
        eprintln!(
            "browser Fit empty/single identity original: single={single}; wanted={original_model:?}; actual={:?}; wanted_nodes={original_nodes:?}; actual_nodes={:?}; wires={:?}",
            serde_json::to_value(&app.project),
            nodes(&app),
            wires(&app)
        );
        assert_eq!(app.canvas_view.rectangles.len(), usize::from(single));
        assert_eq!(serde_json::to_value(&app.project).unwrap(), original_model);
        assert_eq!(nodes(&app), original_nodes);
        assert!(wires(&app).is_empty());
    }
}

#[test]
fn fit_zoom_floor_changes_only_for_a_valid_requested_fit() {
    let mut snarl = Snarl::new();
    let node = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Target);
    let viewport = egui::Rect::from_min_size(egui::pos2(300.0, 80.0), egui::vec2(600.0, 500.0));
    let small = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
    let large = egui::Rect::from_min_max(egui::pos2(-1.0e20, -1.0e20), egui::pos2(1.0e20, 1.0e20));
    let mut view = CanvasView::default();
    view.finish(&snarl, [(node, small)].into(), viewport, None);
    let small_fit = view.prepare(&snarl, viewport);
    let small_floor = view.minimum_scale();
    view.finish(&snarl, [(node, small)].into(), viewport, small_fit);
    view.finish(&snarl, [(node, large)].into(), viewport, small_fit);
    let idle = view.prepare(&snarl, viewport);
    let idle_floor = view.minimum_scale();
    view.request_fit();
    let large_fit = view.prepare(&snarl, viewport);
    let large_floor = view.minimum_scale();
    view.finish(&snarl, [(node, large)].into(), viewport, large_fit);
    let manual = egui::emath::TSTransform {
        scaling: large_fit.unwrap().scaling * 2.0,
        translation: egui::vec2(10.0, 20.0),
    };
    view.finish(&snarl, [(node, large)].into(), viewport, Some(manual));
    let retained_floor = view.minimum_scale();
    let resized_idle = view.prepare(&snarl, viewport.shrink(10.0));
    view.finish(&snarl, [(node, small)].into(), viewport, Some(manual));
    view.request_fit();
    let restored_fit = view.prepare(&snarl, viewport);
    let restored_floor = view.minimum_scale();
    eprintln!(
        "browser Fit zoom policy originals: viewport={viewport:?}; small={small:?}; large={large:?}; small_fit={small_fit:?}; small_floor={small_floor}; idle={idle:?}; idle_floor={idle_floor}; large_fit={large_fit:?}; large_floor={large_floor}; manual={manual:?}; retained_floor={retained_floor}; resized_idle={resized_idle:?}; restored_fit={restored_fit:?}; restored_floor={restored_floor}; state={view:?}"
    );
    assert_eq!(small_floor, 0.2);
    assert!(idle.is_none());
    assert_eq!(
        idle_floor, 0.2,
        "ordinary measured changes do not alter manual zoom policy"
    );
    let large_fit = large_fit.unwrap();
    assert!(valid_canvas_transform(large_fit, viewport));
    assert!((MIN_CANVAS_SCALE..0.2).contains(&large_floor));
    assert_eq!(
        large_floor, large_fit.scaling,
        "only the valid Fit scale lowers the floor"
    );
    assert!(valid_canvas_transform(manual, viewport));
    assert_eq!(retained_floor, large_floor);
    assert!(
        resized_idle.is_none(),
        "manual zoom and ordinary resize do not refit or raise the floor"
    );
    assert!(valid_canvas_transform(restored_fit.unwrap(), viewport));
    assert_eq!(
        restored_floor, 0.2,
        "the next explicit small Fit restores the ordinary floor"
    );
}

fn callback_transform(
    current: egui::emath::TSTransform,
    retained: Option<egui::emath::TSTransform>,
    requested: Option<egui::emath::TSTransform>,
    viewport: Option<egui::Rect>,
) -> (egui::emath::TSTransform, Option<egui::emath::TSTransform>) {
    let mut app = DemoApp::new();
    app.install_project(project("manual", false));
    let original_model = serde_json::to_value(&app.project).unwrap();
    let original_nodes = nodes(&app);
    let original_wires = wires(&app);
    let original_history = app.history.retained();
    let mut actual = current;
    let reported;
    {
        let mut viewer = DemoViewer::new(
            &mut app.project.graph,
            &app.project.target.name,
            &app.bindings,
            &mut app.run_pending,
            &mut app.project_changed,
            &mut app.edited_constant,
            &mut app.focused_constant,
        );
        viewer.viewport = viewport;
        viewer.previous_transform = retained;
        viewer.requested_transform = requested;
        SnarlViewer::current_transform(&mut viewer, &mut actual, &mut app.snarl);
        reported = viewer.rendered_transform;
    }
    eprintln!(
        "browser Fit public transform callback complete originals: current={current:?}; retained={retained:?}; requested={requested:?}; viewport={viewport:?}; actual={actual:?}; reported={reported:?}; wanted_project={original_model:?}; actual_project={:?}; wanted_nodes={original_nodes:?}; actual_nodes={:?}; wanted_wires={original_wires:?}; actual_wires={:?}; wanted_history={original_history:?}; actual_history={:?}",
        serde_json::to_value(&app.project),
        nodes(&app),
        wires(&app),
        app.history.retained()
    );
    assert_eq!(serde_json::to_value(&app.project).unwrap(), original_model);
    assert_eq!(nodes(&app), original_nodes);
    assert_eq!(wires(&app), original_wires);
    assert_eq!(app.history.retained(), original_history);
    (actual, reported)
}

#[test]
fn public_transform_callback_keeps_valid_manual_views_and_rechecks_resized_fallbacks() {
    let viewport = egui::Rect::from_min_size(egui::pos2(300.0, 80.0), egui::vec2(600.0, 500.0));
    let manual = egui::emath::TSTransform {
        scaling: 0.8,
        translation: egui::vec2(-120.0, 40.0),
    };
    let retained = egui::emath::TSTransform {
        scaling: 0.6,
        translation: egui::vec2(20.0, 30.0),
    };
    let unsafe_zoom = egui::emath::TSTransform {
        scaling: f32::MIN_POSITIVE,
        translation: egui::Vec2::ZERO,
    };
    let unsafe_translation = egui::emath::TSTransform {
        scaling: 0.2,
        translation: egui::Vec2::splat(f32::MAX),
    };
    let huge_viewport = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0e38, 1.0e38));
    let old_view = egui::emath::TSTransform {
        scaling: 0.2,
        translation: egui::Vec2::ZERO,
    };
    let preserved = callback_transform(manual, Some(retained), None, Some(viewport));
    let restored_zoom = callback_transform(unsafe_zoom, Some(retained), None, Some(viewport));
    let restored_translation =
        callback_transform(unsafe_translation, Some(retained), None, Some(viewport));
    let requested = callback_transform(manual, Some(manual), Some(retained), Some(viewport));
    let resized = callback_transform(unsafe_zoom, Some(old_view), None, Some(huge_viewport));
    let unavailable = callback_transform(unsafe_zoom, Some(retained), None, None);
    eprintln!(
        "browser Fit manual-branch complete originals before compare: preserved={preserved:?}; restored_zoom={restored_zoom:?}; restored_translation={restored_translation:?}; requested={requested:?}; resized={resized:?}; unavailable={unavailable:?}"
    );
    assert_eq!(preserved, (manual, Some(manual)));
    assert!(!valid_canvas_transform(unsafe_zoom, viewport));
    assert!(!valid_canvas_transform(unsafe_translation, viewport));
    assert_eq!(restored_zoom, (retained, Some(retained)));
    assert_eq!(restored_translation, (retained, Some(retained)));
    assert_eq!(requested, (retained, Some(retained)));
    assert!(valid_canvas_transform(old_view, viewport));
    assert!(
        !valid_canvas_transform(old_view, huge_viewport),
        "retained fallback must be rechecked after resize"
    );
    let expected = egui::emath::TSTransform {
        scaling: 1.0,
        translation: egui::vec2(5.0e37, 5.0e37),
    };
    assert_eq!(resized, (expected, Some(expected)));
    assert!(valid_canvas_transform(resized.0, huge_viewport));
    assert_eq!(
        unavailable,
        (egui::emath::TSTransform::IDENTITY, None),
        "missing viewport stays an unqualified negative outcome"
    );
}

#[test]
fn demo_ui_manual_zoom_reaches_the_ordinary_floor_without_refitting_or_model_edits() {
    for size in [egui::vec2(1200.0, 850.0), egui::vec2(760.0, 850.0)] {
        let mut app = DemoApp::new();
        app.live_run = false;
        app.run_pending = false;
        let context = egui::Context::default();
        settle(&mut app, &context, size);
        contained(&app);
        let original_model = serde_json::to_value(&app.project).unwrap();
        let original_nodes = nodes(&app);
        let original_wires = wires(&app);
        let original_history = app.history.retained();
        let generation = app.canvas_view_generation;
        let viewport = app.canvas_view.viewport.unwrap();
        // Fit has a 12-point margin; this point lies on its empty background.
        let pointer = viewport.min + egui::vec2(4.0, 4.0);
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::PointerMoved(pointer)],
        );
        frame(
            &mut app,
            &context,
            size,
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::Zoom(1.0e-30),
            ],
        );
        let at_floor = app.canvas_view.transform;
        settle(&mut app, &context, size);
        let retained = app.canvas_view.transform;
        eprintln!(
            "browser Fit actual manual zoom original before compare: size={size:?}; pointer={pointer:?}; viewport={viewport:?}; at_floor={at_floor:?}; retained={retained:?}; floor={}; passes={}; wanted_project={original_model:?}; actual_project={:?}; wanted_nodes={original_nodes:?}; actual_nodes={:?}; wanted_wires={original_wires:?}; actual_wires={:?}; wanted_history={original_history:?}; actual_history={:?}",
            app.canvas_view.minimum_scale(),
            app.canvas_view.fit_passes,
            serde_json::to_value(&app.project),
            nodes(&app),
            wires(&app),
            app.history.retained()
        );
        let actual = at_floor.expect("actual widget manual transform");
        assert_eq!(app.canvas_view.minimum_scale(), 0.2);
        assert_eq!(
            actual.scaling, 0.2,
            "finite manual gesture reaches the existing ordinary floor"
        );
        assert!(valid_canvas_transform(actual, viewport));
        assert_eq!(
            retained,
            Some(actual),
            "idle frames preserve the valid manual view"
        );
        assert_eq!(app.canvas_view.fit_passes, 0);
        assert_eq!(app.canvas_view_generation, generation);
        assert_eq!(serde_json::to_value(&app.project).unwrap(), original_model);
        assert_eq!(nodes(&app), original_nodes);
        assert_eq!(wires(&app), original_wires);
        assert_eq!(app.history.retained(), original_history);
    }
}
