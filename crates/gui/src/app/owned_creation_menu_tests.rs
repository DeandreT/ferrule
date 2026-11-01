//! Pointer-driven creation at ordinary desktop sizes, including edge popups.
use super::*;
use egui_kittest::Harness;

/// A context menu can reflow for several passes while its constrained list
/// height and placement settle. A fixed frame count can leave a stale click
/// point. Wait for the painted action to remain still, including after hover.
fn stable_action_position(
    label: &str,
    mut render: impl FnMut(Vec<egui::Event>) -> Vec<egui::Pos2>,
) -> egui::Pos2 {
    let mut previous = None;
    let mut stable = 0;
    for _ in 0..32 {
        let points = render(vec![]);
        let Some(&point) = points.first() else {
            continue;
        };
        assert_eq!(points.len(), 1, "one visible creation action: {label}");
        if previous == Some(point) {
            stable += 1;
        } else {
            stable = 0;
        }
        previous = Some(point);
        if stable >= 2 {
            let hovered = render(vec![egui::Event::PointerMoved(point)]);
            if hovered == [point] && render(vec![]) == [point] {
                return point;
            }
            previous = None;
            stable = 0;
        }
    }
    panic!("creation action {label:?} did not settle");
}

pub(in crate::app) fn click_action(app: &mut FerruleApp, ctx: &egui::Context, label: &str) {
    let point = stable_action_position(label, |events| {
        visible(&frame(app, ctx, true, events), label)
    });
    pointer(app, ctx, true, point, egui::PointerButton::Primary);
}

#[test]
fn lookup_and_all_direct_aggregates_are_pointer_reachable_on_main_and_named_canvases() {
    for size in [(1400.0, 1000.0), (1280.0, 800.0)] {
        for document in [MappingDocument::Main, MappingDocument::Target(0)] {
            for (label, op) in std::iter::once(("Lookup from one input".to_owned(), None)).chain(
                crate::path_picker::OWNED_AGGREGATE_OPS
                    .into_iter()
                    .map(|(op, label)| (format!("{label} from one input"), Some(op))),
            ) {
                let app = fixture(document);
                let original = app.project.graph.nodes.clone();
                let mut harness = Harness::builder()
                    .with_size(size)
                    .with_accessibility_check(false)
                    .with_max_steps(80)
                    .with_context_setup(|ctx| {
                        crate::icons::install(ctx);
                        ctx.add_font_provider(std::sync::Arc::new(
                            egui_system_font_provider::SystemFontProvider::new(),
                        ));
                    })
                    .build_ui_state(
                        |ui, app: &mut FerruleApp| {
                            app.show_mapping_workspace_canvas(ui, true);
                            app.observe_editor_history(std::time::Instant::now(), false);
                        },
                        app,
                    );
                harness.run();
                // Open near the lower-right edge, rather than avoiding popup
                // constraints by moving the menu into the middle of the canvas.
                let anchor = egui::pos2(size.0 - 300.0, size.1 - 150.0);
                for event in [
                    egui::Event::PointerMoved(anchor),
                    egui::Event::PointerButton {
                        pos: anchor,
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: anchor,
                        button: egui::PointerButton::Secondary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ] {
                    harness.event(event);
                    harness.step();
                }
                let point = stable_action_position(&label, |events| {
                    for event in events {
                        harness.event(event);
                    }
                    harness.step();
                    visible(harness.output(), &label)
                });
                assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()).contains(point));
                for action in std::iter::once("Lookup from one input".to_owned()).chain(
                    crate::path_picker::OWNED_AGGREGATE_OPS
                        .into_iter()
                        .map(|(_, label)| format!("{label} from one input")),
                ) {
                    assert_eq!(
                        visible(harness.output(), &action).len(),
                        1,
                        "{size:?}/{document:?}: {action}"
                    );
                }
                if op.is_none()
                    && let Some(path) = std::env::var_os("FERRULE_MENU_SHOTS")
                {
                    harness
                        .render()
                        .unwrap()
                        .save(PathBuf::from(path).join(format!(
                            "creation-menu-{}x{}-{document:?}.png",
                            size.0, size.1
                        )))
                        .unwrap();
                }
                for pressed in [true, false] {
                    harness.event(egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                    harness.step();
                }
                let app = harness.state();
                for (id, node) in &original {
                    assert_eq!(
                        crate::project_state::try_model_key(&app.project.graph.nodes[id]),
                        crate::project_state::try_model_key(node)
                    );
                }
                let created = match op {
                    None => {
                        assert_eq!(app.project.graph.nodes.len(), 4);
                        assert!(matches!(app.project.graph.nodes[&8], Node::Unconnected));
                        let Node::OwnedLookup(lookup) = &app.project.graph.nodes[&9] else {
                            panic!("{label}: wrong created node")
                        };
                        assert_eq!(lookup.collection.owner, SourceOwner::Primary);
                        assert_eq!(lookup.collection.path, ["Rows"]);
                        assert_eq!(lookup.key, ["Code"]);
                        assert_eq!(lookup.value, ["Code"]);
                        assert_eq!(lookup.matches, 8);
                        9
                    }
                    Some(op) => {
                        assert_eq!(app.project.graph.nodes.len(), 3);
                        let Node::OwnedAggregate(aggregate) = &app.project.graph.nodes[&8] else {
                            panic!("{label}: wrong created node")
                        };
                        assert_eq!(aggregate.function, op);
                        assert_eq!(aggregate.collection.owner, SourceOwner::Primary);
                        assert!(aggregate.arg.is_none());
                        let (collection, value) = if op == mapping::AggregateOp::Count {
                            (vec![], vec![])
                        } else {
                            (vec!["Rows".to_string()], vec!["Code".to_string()])
                        };
                        assert_eq!(aggregate.collection.path, collection);
                        assert_eq!(aggregate.value, mapping::OwnedAggregateValue::Path(value));
                        8
                    }
                };
                let Some(HistoryRecord::Command(record)) = app.history.next_undo() else {
                    panic!("typed creation")
                };
                assert!(
                    matches!(record.commands()[0].action(), editor_ui::GraphAction::InsertNode { node, .. } if *node == created)
                );
                assert!(canvas_node_visible(app, document, created));
            }
        }
    }
}

fn canvas_node_visible(app: &FerruleApp, document: MappingDocument, node: NodeId) -> bool {
    let canvas = match document {
        MappingDocument::Main => &app.main_canvas,
        MappingDocument::Target(index) => &app.mapping_workspace.target_canvases[&index],
        _ => panic!("mapping canvas"),
    };
    canvas
        .snarl
        .node_ids()
        .any(|(_, candidate)| *candidate == CanvasNode::Graph(node))
}
