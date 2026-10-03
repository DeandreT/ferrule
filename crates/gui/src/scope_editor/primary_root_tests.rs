use super::*;
use ir::Value;
use mapping::Node;

fn graph() -> Graph {
    let mut graph = Graph::default();
    graph.nodes.extend([
        (
            1,
            Node::SourceRootField {
                path: vec!["Code".into()],
                required: false,
            },
        ),
        (
            2,
            Node::If {
                condition: 4,
                then: 1,
                else_: 8,
            },
        ),
        (
            3,
            Node::Call {
                function: "concat".into(),
                args: vec![2, 8],
            },
        ),
        (
            4,
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: "Base".into(),
            },
        ),
        (7, Node::Unconnected),
        (
            8,
            Node::Const {
                value: Value::String("safe".into()),
            },
        ),
    ]);
    graph
}

fn scope() -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "value".into(),
            node: 8,
        }],
        ..Scope::default()
    }
}

fn frame(
    context: &egui::Context,
    scope: &mut Scope,
    graph: &Graph,
    primary_root_bindings: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_ui(
        egui::RawInput {
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            show_scope_editor(
                ui,
                scope,
                graph,
                &SourcePathCatalog::new(&crate::primary_root_authoring::test_schema(), &[]),
                &["value".into(), "other".into()],
                ScopeEditorOwner {
                    nested: false,
                    primary_root_bindings,
                },
                ScopeOutputProfile::ReadOnly(
                    "XML element output selection is available on child scopes",
                ),
            );
        },
    )
}

fn click(
    context: &egui::Context,
    scope: &mut Scope,
    graph: &Graph,
    primary_root_bindings: bool,
    label: &str,
    last: bool,
) {
    fn collect(shape: &egui::epaint::Shape, label: &str, found: &mut Vec<egui::Pos2>) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                found.push(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, label, found);
                }
            }
            _ => {}
        }
    }
    let mut output = frame(context, scope, graph, primary_root_bindings, Vec::new());
    for _ in 0..3 {
        output = frame(context, scope, graph, primary_root_bindings, Vec::new());
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, label, &mut found);
    }
    let position = *if last { found.last() } else { found.first() }
        .unwrap_or_else(|| panic!("missing scope choice {label:?}"));
    for pressed in [true, false] {
        let _ = frame(
            context,
            scope,
            graph,
            primary_root_bindings,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn named_and_control_defaults_skip_direct_and_derived_root_nodes() {
    let graph = graph();
    assert_eq!(first_node_id(&graph), Some(8));
    assert_eq!(binding_node_ids(&graph, false).collect::<Vec<_>>(), vec![8]);
    assert_eq!(
        binding_node_ids(&graph, true).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 8]
    );
    let mut root_only = graph.clone();
    root_only.nodes.remove(&8);
    assert_eq!(first_node_id(&root_only), None);
    assert_eq!(binding_node_ids(&root_only, false).next(), None);
}

#[test]
fn named_binding_picker_rejects_direct_and_if_call_root_choices() {
    for expression in [1, 2, 3, 4] {
        let graph = graph();
        let mut scope = scope();
        let before = serde_json::to_value(&scope).unwrap();
        let context = egui::Context::default();
        click(
            &context,
            &mut scope,
            &graph,
            false,
            "8: constant String(\"safe\")",
            false,
        );
        let label = format!("{expression}: {}", node_label(&graph.nodes[&expression]));
        click(&context, &mut scope, &graph, false, &label, true);
        assert_eq!(serde_json::to_value(&scope).unwrap(), before);
    }
}

#[test]
fn imported_invalid_binding_is_preserved_during_rendering_and_can_be_repaired() {
    let graph = graph();
    let mut scope = scope();
    scope.bindings[0].node = 3;
    let before = serde_json::to_value(&scope).unwrap();
    let context = egui::Context::default();
    for _ in 0..4 {
        let _ = frame(&context, &mut scope, &graph, false, Vec::new());
    }
    assert_eq!(serde_json::to_value(&scope).unwrap(), before);
    click(&context, &mut scope, &graph, false, "3: concat", false);
    click(
        &context,
        &mut scope,
        &graph,
        false,
        "8: constant String(\"safe\")",
        true,
    );
    assert_eq!(scope.bindings[0].node, 8);
    assert_eq!(scope.bindings[0].target_field, "value");
    assert_eq!(scope.bindings.len(), 1);
}

#[test]
fn eligible_primary_binding_picker_accepts_derived_root_expression() {
    let graph = graph();
    let mut scope = scope();
    let context = egui::Context::default();
    click(
        &context,
        &mut scope,
        &graph,
        true,
        "8: constant String(\"safe\")",
        false,
    );
    click(&context, &mut scope, &graph, true, "3: concat", true);
    assert_eq!(scope.bindings[0].node, 3);
}

#[test]
fn add_named_binding_uses_an_ordinary_expression_and_root_only_graph_is_disabled() {
    let graph = graph();
    let mut scope = Scope::default();
    let context = egui::Context::default();
    click(&context, &mut scope, &graph, false, "+ binding", false);
    assert_eq!(scope.bindings.len(), 1);
    assert_eq!(scope.bindings[0].node, 8);
    let mut root_only = graph.clone();
    root_only.nodes.remove(&8);
    let mut scope = Scope::default();
    let context = egui::Context::default();
    let before = serde_json::to_value(&scope).unwrap();
    click(&context, &mut scope, &root_only, false, "+ binding", false);
    assert_eq!(serde_json::to_value(&scope).unwrap(), before);
}

#[test]
fn root_expression_is_not_offered_after_a_scope_control_changes_its_owner() {
    let graph = graph();
    let mut scope = scope();
    scope.filter = Some(8);
    let before = serde_json::to_value(&scope).unwrap();
    let context = egui::Context::default();
    click(
        &context,
        &mut scope,
        &graph,
        true,
        "8: constant String(\"safe\")",
        false,
    );
    click(&context, &mut scope, &graph, true, "3: concat", true);
    assert_eq!(serde_json::to_value(&scope).unwrap(), before);
}
