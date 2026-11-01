//! Owned field, position and scope-iteration authoring for a primary input
//! whose field shares a named input's alias, compared with hand-written
//! owned projects after save and reopen (#929).

use super::*;
use crate::canvas::{SourceLeaf, TargetLeaf, project_source_blocks};
use editor_ui::WireSink;
use egui_snarl::ui::SnarlViewer as _;
use ir::Instance;
use mapping::{
    NamedSource, OwnedPosition, OwnedSourceField, OwnedSourceRef, ScopeIteration, SourceOwner,
};

fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn value_group(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![text("Value")])
}

fn rows() -> SchemaNode {
    let mut rows = value_group("Rows");
    rows.repeating = true;
    rows
}

fn ref_owner() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// Primary `Ref/Value` begins with the named alias `Ref`, so legacy text can
/// resolve in either endpoint; both endpoints also contain `Rows/Value`.
fn project(primary_alias: &str) -> Project {
    let mut lines = SchemaNode::group(
        "Lines",
        vec![
            text("Value"),
            SchemaNode::scalar("Position", ScalarType::Int),
            text("Primary"),
        ],
    );
    lines.repeating = true;
    let mut project = blank_project();
    project.source = SchemaNode::group("Input", vec![value_group(primary_alias), rows()]);
    project.target = SchemaNode::group("Output", vec![text("Head"), lines]);
    project.source_path = Some("input.json".into());
    project.extra_sources = vec![NamedSource {
        name: "Ref".into(),
        path: "ref.json".into(),
        schema: SchemaNode::group("Doc", vec![value_group("Head"), rows()]),
        options: Default::default(),
        dynamic_path: None,
    }];
    project.graph.nodes.clear();
    project.root = Scope {
        children: vec![Scope {
            target_field: "Lines".into(),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

fn owned_app(primary_alias: &str) -> FerruleApp {
    let mut app = FerruleApp {
        project: project(primary_alias),
        ..Default::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    app
}

fn source_pin(app: &FerruleApp, block: usize, label: &str) -> (CanvasNode, usize) {
    let blocks = project_source_blocks(&app.project);
    let pin = blocks[block]
        .leaves
        .iter()
        .position(|leaf: &SourceLeaf| leaf.label == label)
        .unwrap_or_else(|| panic!("source pin {label}"));
    (CanvasNode::SourceBlock(block), pin)
}

fn target_pin(app: &FerruleApp, chain: &[&str], field: &str) -> (CanvasNode, usize) {
    target_blocks(&app.project.target)
        .iter()
        .enumerate()
        .find_map(|(block, section)| {
            section
                .leaves
                .iter()
                .position(|leaf: &TargetLeaf| leaf.chain == strings(chain) && leaf.field == field)
                .map(|pin| (CanvasNode::TargetBlock(block), pin))
        })
        .unwrap_or_else(|| panic!("target pin {field}"))
}

fn connect(
    app: &mut FerruleApp,
    (from, output): (CanvasNode, usize),
    (to, input): (CanvasNode, usize),
) -> PlannedGraphEdit {
    let (mut planned, error) = edit_canvas(app, MappingDocument::Main, |viewer, snarl| {
        let from = snarl.out_pin(OutPinId {
            node: canvas_id(snarl, from),
            output,
        });
        let to = snarl.in_pin(InPinId {
            node: canvas_id(snarl, to),
            input,
        });
        viewer.connect(&from, &to, snarl);
    });
    assert_eq!(error, None);
    assert_eq!(planned.len(), 1, "one typed edit");
    planned.remove(0)
}

fn binding(scope: &Scope, field: &str) -> NodeId {
    scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == field)
        .map(|binding| binding.node)
        .unwrap_or_else(|| panic!("binding {field}"))
}

fn owned_field(app: &FerruleApp, node: NodeId) -> OwnedSourceField {
    match app.project.graph.nodes.get(&node) {
        Some(Node::OwnedSourceField(field)) => field.clone(),
        other => panic!("node {node} is an owned field, got {other:?}"),
    }
}

fn field(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> OwnedSourceField {
    OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: strings(path),
        },
        frame: frame.map(strings),
    }
}

fn shapes_with(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    fn collect(shape: &egui::epaint::Shape, label: &str, found: &mut Vec<egui::Pos2>) {
        match shape {
            egui::epaint::Shape::Text(text)
                if crate::test_ui::inspector_label_matches(text.galley.text(), label) =>
            {
                found.push(text.visual_bounding_rect().center());
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, label, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, label, &mut found);
    }
    found
}

/// Clicks the last text shape with `label` that `show` draws, as a pointer would.
fn click_label(context: &egui::Context, label: &str, mut show: impl FnMut(&mut egui::Ui)) {
    let mut frame = |events: Vec<egui::Event>| {
        context.run_test_ui(
            egui::RawInput {
                time: Some(context.cumulative_frame_nr() as f64 / 10.0),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1400.0),
                )),
                events,
                ..Default::default()
            },
            &mut show,
        )
    };
    let mut output = frame(Vec::new());
    for _ in 0..3 {
        output = frame(Vec::new());
    }
    let position = *shapes_with(&output, label)
        .last()
        .unwrap_or_else(|| panic!("missing choice {label:?}"));
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    frame(Vec::new());
}

/// Shows the `Lines` scope editor as the scope panel does.
fn show_lines_editor(ui: &mut egui::Ui, project: &mut Project) {
    let catalog = SourcePathCatalog::new(&project.source, &project.extra_sources)
        .with_project_ownership(project);
    let scope = &mut project.root.children[0];
    let _ = crate::scope_editor::show_scope_editor(
        ui,
        scope,
        &project.graph,
        &catalog,
        &["Value".into(), "Position".into(), "Primary".into()],
        crate::scope_editor::ScopeEditorOwner {
            lookup_context: true,
            nested: true,
            primary_root_bindings: false,
            target_xml_types: &[],
            target_xml_default: None,
            target_type_editable: false,
        },
        crate::scope_editor::ScopeOutputProfile::ReadOnly("test"),
    );
}

/// Chooses an owned collection for the `Lines` scope in the Inspector's
/// scope editor, which records one typed iteration change.
fn choose_lines_iteration(app: &mut FerruleApp, context: &egui::Context, labels: [&str; 2]) {
    app.selected_scope = vec![0];
    for label in labels {
        click_label(context, label, |ui| app.show_inspector(ui, true));
    }
    app.observe_editor_history(std::time::Instant::now(), false);
    assert_eq!(app.history.undo_label(), Some("Change scope iteration"));
    let command = next_undo_command(app);
    let lines = ScopeLocation::of_children(&[0]);
    assert_eq!(
        command.action(),
        &GraphAction::EditScope {
            scope: lines.clone(),
            setting: editor_ui::ScopeSetting::Iteration,
        }
    );
    let change = command
        .patch()
        .setting(&lines, editor_ui::ScopeSetting::Iteration)
        .expect("the iteration change");
    assert!(matches!(
        change.after(),
        editor_ui::ScopeSettingValue::Iteration(editor_ui::Iteration::Owned {
            source,
            output_path: None,
        }) if source.owner == ref_owner()
    ));
}

fn lines_editor_shows(project: &mut Project, label: &str) -> bool {
    let context = egui::Context::default();
    let output = context.run_test_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            ..Default::default()
        },
        |ui| show_lines_editor(ui, project),
    );
    !shapes_with(&output, label).is_empty()
}

/// Picks a position's owned collection in its canvas property editor,
/// which records one typed position edit.
fn choose_position_collection(
    app: &mut FerruleApp,
    context: &egui::Context,
    node: NodeId,
    labels: [&str; 2],
) {
    let mut edits = Vec::new();
    for label in labels {
        let (planned, error) = edit_canvas(app, MappingDocument::Main, |viewer, snarl| {
            let pin = snarl.out_pin(OutPinId {
                node: canvas_id(snarl, CanvasNode::Graph(node)),
                output: 0,
            });
            click_label(context, label, |ui| {
                viewer.show_properties_for_test(&pin, ui, snarl);
            });
        });
        assert_eq!(error, None);
        edits.extend(planned);
    }
    let [edit] = <[PlannedGraphEdit; 1]>::try_from(edits)
        .unwrap_or_else(|edits| panic!("one typed edit, got {}", edits.len()));
    assert_eq!(
        edit.action,
        GraphAction::EditNode {
            node,
            property: editor_ui::NodeProperty::Position,
        }
    );
    assert!(observe_planned(app, MappingDocument::Main, edit));
    assert_eq!(app.history.undo_label(), Some("Edit position"));
    assert_eq!(
        next_undo_command(app)
            .patch()
            .changed_nodes()
            .collect::<Vec<_>>(),
        [node]
    );
}

/// The equivalent project written by hand with explicit owned forms.
fn hand_written() -> Project {
    let mut project = project("Ref");
    let nodes = [
        (
            10,
            Node::OwnedSourceField(field(ref_owner(), &["Head", "Value"], None)),
        ),
        (
            11,
            Node::OwnedSourceField(field(ref_owner(), &["Rows", "Value"], Some(&["Rows"]))),
        ),
        (
            12,
            Node::OwnedPosition(OwnedPosition {
                owner: ref_owner(),
                collection: Some(strings(&["Rows"])),
            }),
        ),
        (
            13,
            Node::OwnedSourceField(field(SourceOwner::Primary, &["Ref", "Value"], None)),
        ),
    ];
    project.graph.nodes.extend(nodes);
    project.root.bindings = vec![Binding {
        target_field: "Head".into(),
        node: 10,
    }];
    let lines = &mut project.root.children[0];
    lines.iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: ref_owner(),
            path: strings(&["Rows"]),
        },
        output_path: None,
    };
    lines.bindings = [("Value", 11), ("Position", 12), ("Primary", 13)]
        .into_iter()
        .map(|(target_field, node)| Binding {
            target_field: target_field.into(),
            node,
        })
        .collect();
    project
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn scalar(text: &str) -> Instance {
    Instance::Scalar(Value::String(text.into()))
}

fn value(text: &str) -> Instance {
    group(vec![("Value", scalar(text))])
}

/// Colliding data: legacy fallback would read primary rows or values here.
fn run(project: &Project) -> Instance {
    let primary = group(vec![
        ("Ref", value("primary-ref")),
        (
            "Rows",
            Instance::Repeated(vec![value("P1"), value("P2"), value("P3")]),
        ),
    ]);
    let named = group(vec![
        ("Head", value("named-head")),
        ("Rows", Instance::Repeated(vec![value("N1"), value("N2")])),
    ]);
    assert!(
        cli::validate(project).is_empty(),
        "{:?}",
        cli::validate(project)
    );
    engine::run_with_sources(project, &primary, vec![("Ref".into(), named)]).unwrap()
}

/// Each binding's node, keyed by target field, in stable wire form.
fn bound_forms(project: &Project) -> Vec<(String, serde_json::Value)> {
    let scopes = [&project.root, &project.root.children[0]];
    scopes
        .iter()
        .flat_map(|scope| &scope.bindings)
        .map(|binding| {
            (
                binding.target_field.clone(),
                serde_json::to_value(&project.graph.nodes[&binding.node]).unwrap(),
            )
        })
        .collect()
}

#[test]
fn colliding_named_pin_connects_one_owned_node_command_with_undo_and_redo() {
    let mut app = owned_app("Ref");
    let saved = key(&app);
    let from = source_pin(&app, 1, "Head/Value");
    let to = target_pin(&app, &[], "Head");
    let planned = connect(&mut app, from, to);
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    assert_eq!(app.history.undo_label(), Some("Connect wire"));
    let read = binding(&app.project.root, "Head");
    assert_eq!(
        owned_field(&app, read),
        field(ref_owner(), &["Head", "Value"], None)
    );
    let command = next_undo_command(&app);
    assert_eq!(
        command.action(),
        &GraphAction::ConnectWire {
            source: read,
            sink: WireSink::Binding {
                scope: ScopeLocation::root(),
                field: "Head".into(),
            },
        }
    );
    assert_eq!(super::wires::ids(command.patch().added_nodes()), [read]);
    // The owned read is a visible node that names its owner.
    assert!(position(&app.main_canvas.snarl, CanvasNode::Graph(read)).is_some());
    let title = edit_canvas(&mut app, MappingDocument::Main, |viewer, _| {
        assert_eq!(
            viewer.title(&CanvasNode::Graph(read)),
            "Owned Ref: Head/Value"
        );
    });
    assert_eq!(title.1, None);
    let connected = key(&app);

    app.undo_project();
    assert_eq!(key(&app), saved);
    assert!(app.project.graph.nodes.is_empty());
    assert!(position(&app.main_canvas.snarl, CanvasNode::Graph(read)).is_none());
    app.redo_project();
    assert_eq!(key(&app), connected);
    assert!(position(&app.main_canvas.snarl, CanvasNode::Graph(read)).is_some());
}

#[test]
fn unambiguous_named_pin_keeps_authoring_the_hidden_legacy_field() {
    let mut app = owned_app("Other");
    let from = source_pin(&app, 1, "Head/Value");
    let to = target_pin(&app, &[], "Head");
    let planned = connect(&mut app, from, to);
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    let read = binding(&app.project.root, "Head");
    assert!(matches!(
        app.project.graph.nodes.get(&read),
        Some(Node::SourceField { frame: None, path }) if path == &strings(&["Ref", "Head", "Value"])
    ));
    assert!(position(&app.main_canvas.snarl, CanvasNode::Graph(read)).is_none());
    // Collection choices stay legacy-only without an ambiguous path.
    let catalog = SourcePathCatalog::new(&app.project.source, &app.project.extra_sources)
        .with_project_ownership(&app.project);
    assert!(!catalog.ownership().active());
}

#[test]
fn owned_field_position_and_iteration_edits_round_trip_and_match_hand_written_execution() {
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut app = owned_app("Ref");
    let saved = key(&app);
    let mut states = vec![saved.clone()];

    let (from, to) = (
        source_pin(&app, 1, "Head/Value"),
        target_pin(&app, &[], "Head"),
    );
    let planned = connect(&mut app, from, to);
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    states.push(key(&app));

    choose_lines_iteration(
        &mut app,
        &context,
        ["Iterate one input's collection", "Ref: Rows"],
    );
    assert!(matches!(
        &app.project.root.children[0].iteration,
        ScopeIteration::OwnedSource { source, output_path: None }
            if source.owner == ref_owner() && source.path == strings(&["Rows"])
    ));
    states.push(key(&app));

    let (from, to) = (
        source_pin(&app, 1, "Rows/Value"),
        target_pin(&app, &["Lines"], "Value"),
    );
    let planned = connect(&mut app, from, to);
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    states.push(key(&app));

    let (mut planned, error) = edit_canvas(&mut app, MappingDocument::Main, |viewer, snarl| {
        viewer
            .insert_position_for_test(snarl, egui::pos2(300.0, 400.0))
            .unwrap();
    });
    assert_eq!(error, None);
    assert!(observe_planned(
        &mut app,
        MappingDocument::Main,
        planned.remove(0)
    ));
    assert_eq!(app.history.undo_label(), Some("Insert node"));
    let GraphAction::InsertNode {
        node: position_node,
        ..
    } = *next_undo_command(&app).action()
    else {
        panic!("a position insertion");
    };
    assert!(matches!(
        app.project.graph.nodes.get(&position_node),
        Some(Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None
        }))
    ));
    states.push(key(&app));

    choose_position_collection(
        &mut app,
        &context,
        position_node,
        ["primary: <latest item>", "Ref: Rows"],
    );
    assert!(matches!(
        app.project.graph.nodes.get(&position_node),
        Some(Node::OwnedPosition(OwnedPosition { owner, collection: Some(collection) }))
            if *owner == ref_owner() && *collection == strings(&["Rows"])
    ));
    states.push(key(&app));

    let (from, to) = (
        (CanvasNode::Graph(position_node), 0),
        target_pin(&app, &["Lines"], "Position"),
    );
    let planned = connect(&mut app, from, to);
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    states.push(key(&app));

    // The primary path begins with the named alias, so it is owned as well.
    let (from, to) = (
        source_pin(&app, 0, "Ref/Value"),
        target_pin(&app, &["Lines"], "Primary"),
    );
    let planned = connect(&mut app, from, to);
    assert!(observe_planned(&mut app, MappingDocument::Main, planned));
    states.push(key(&app));

    let lines = &app.project.root.children[0];
    assert_eq!(
        owned_field(&app, binding(lines, "Value")),
        field(ref_owner(), &["Rows", "Value"], Some(&["Rows"]))
    );
    assert_eq!(
        owned_field(&app, binding(lines, "Primary")),
        field(SourceOwner::Primary, &["Ref", "Value"], None)
    );

    // Every edit is one history step in both directions.
    for state in states.iter().rev().skip(1) {
        app.undo_project();
        assert_eq!(&key(&app), state);
    }
    for state in states.iter().skip(1) {
        app.redo_project();
        assert_eq!(&key(&app), state);
    }

    let saved_text = mapping::project_file::encode_pretty(&app.project).unwrap();
    let reopened = mapping::project_file::decode_str(&saved_text).unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&reopened).unwrap(),
        saved_text
    );
    let expected = hand_written();
    assert_eq!(bound_forms(&reopened), bound_forms(&expected));
    assert_eq!(
        format!("{:?}", reopened.root.children[0].iteration),
        format!("{:?}", expected.root.children[0].iteration)
    );
    let output = run(&reopened);
    assert_eq!(output, run(&expected));
    let line = |text: &str, position: i64| {
        group(vec![
            ("Value", scalar(text)),
            ("Position", Instance::Scalar(Value::Int(position))),
            ("Primary", scalar("primary-ref")),
        ])
    };
    assert_eq!(
        output,
        group(vec![
            ("Head", scalar("named-head")),
            (
                "Lines",
                Instance::Repeated(vec![line("N1", 1), line("N2", 2)])
            ),
        ])
    );

    // A reopened canvas shows each owned read with its owner.
    let canvas = build_snarl(&reopened);
    for node in [
        binding(&reopened.root, "Head"),
        binding(&reopened.root.children[0], "Value"),
        position_node,
    ] {
        assert!(position(&canvas, CanvasNode::Graph(node)).is_some());
    }
}

#[test]
fn scope_editor_offers_owners_only_for_ambiguous_projects_and_keeps_special_owned_scopes_read_only()
{
    let offer = "Iterate one input's collection";
    let mut plain = project("Other");
    assert!(!lines_editor_shows(&mut plain, offer));
    let before = mapping::project_file::encode_pretty(&plain).unwrap();
    assert!(lines_editor_shows(&mut plain, "iterates"));
    assert_eq!(
        mapping::project_file::encode_pretty(&plain).unwrap(),
        before
    );

    let mut ambiguous = project("Ref");
    assert!(lines_editor_shows(&mut ambiguous, offer));
    ambiguous.root.children[0].iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: ref_owner(),
            path: strings(&["Rows"]),
        },
        output_path: None,
    };
    assert!(lines_editor_shows(&mut ambiguous, "Ref: Rows"));
    // Per-item document outputs of owned iteration are not authored here.
    ambiguous.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::String("out.json".into()),
        },
    );
    ambiguous.root.children[0].set_output_path(Some(5));
    let before = mapping::project_file::encode_pretty(&ambiguous).unwrap();
    assert!(lines_editor_shows(
        &mut ambiguous,
        "This owned source iteration's construction is not authorable; this saved scope is read-only."
    ));
    assert!(!lines_editor_shows(&mut ambiguous, offer));
    assert_eq!(
        mapping::project_file::encode_pretty(&ambiguous).unwrap(),
        before
    );
}

#[test]
fn legacy_positions_convert_to_an_owner_only_on_an_explicit_ambiguous_choice() {
    let context = egui::Context::default();
    crate::icons::install(&context);
    for (alias, owned) in [("Other", false), ("Ref", true)] {
        let mut app = owned_app(alias);
        let (mut planned, error) = edit_canvas(&mut app, MappingDocument::Main, |viewer, snarl| {
            viewer
                .insert_position_for_test(snarl, egui::pos2(300.0, 400.0))
                .unwrap();
        });
        assert_eq!(error, None);
        assert!(observe_planned(
            &mut app,
            MappingDocument::Main,
            planned.remove(0)
        ));
        let GraphAction::InsertNode { node, .. } = *next_undo_command(&app).action() else {
            panic!("a position insertion");
        };
        // Without existing ownership the palette keeps the legacy position.
        assert!(matches!(
            app.project.graph.nodes.get(&node),
            Some(Node::Position { collection }) if collection.is_empty()
        ));
        if owned {
            let inserted = key(&app);
            choose_position_collection(
                &mut app,
                &context,
                node,
                ["Count one input's items", "Ref: Rows"],
            );
            assert!(matches!(
                app.project.graph.nodes.get(&node),
                Some(Node::OwnedPosition(OwnedPosition { owner, collection: Some(collection) }))
                    if *owner == ref_owner() && *collection == strings(&["Rows"])
            ));
            app.undo_project();
            assert_eq!(key(&app), inserted);
        } else {
            let (_, error) = edit_canvas(&mut app, MappingDocument::Main, |viewer, snarl| {
                let pin = snarl.out_pin(OutPinId {
                    node: canvas_id(snarl, CanvasNode::Graph(node)),
                    output: 0,
                });
                let output = context.run_test_ui(Default::default(), |ui| {
                    viewer.show_properties_for_test(&pin, ui, snarl);
                });
                assert!(shapes_with(&output, "Count one input's items").is_empty());
            });
            assert_eq!(error, None);
        }
    }
}
