use super::*;
use crate::scope_editor::{ScopeOutputProfile, output_profile};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, IterationOutput, NamedTarget, Scope, ScopeConstruction, ScopeIteration,
    SequenceWindow,
};

fn xml_app(document: MappingDocument) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    app.project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group(
            "Entry",
            vec![SchemaNode::scalar("Value", ScalarType::Int)],
        )],
    );
    app.project.source_options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    app.project.target_options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.data".into());
    app.project.graph.nodes.clear();
    for (id, value) in [(1, 2), (2, 3)] {
        app.project.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::Int(value),
            },
        );
    }
    app.project.graph.nodes.insert(
        10,
        Node::SourceField {
            path: vec!["Rows".into(), "Value".into()],
            frame: None,
        },
    );
    app.project.root = Scope {
        children: vec![Scope {
            target_field: "Entry".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            iteration_output: IterationOutput::MappedSequence,
            sort_by: Some(10),
            windows: vec![
                SequenceWindow::SkipFirst { count: 1 },
                SequenceWindow::First { count: 2 },
            ],
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 10,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    app.project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some("other.csv".into()),
        schema: app.project.target.clone(),
        options: app.project.target_options.clone(),
        root: app.project.root.clone(),
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    if let MappingDocument::Target(index) = document {
        app.open_target_tab(index);
    }
    app.selected_scope = vec![0];
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn active_scope(app: &FerruleApp) -> &Scope {
    match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root.children[0],
        _ => &app.project.root.children[0],
    }
}

fn source(values: &[i64]) -> Instance {
    Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(
            values
                .iter()
                .map(|value| {
                    Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(*value)))])
                })
                .collect(),
        ),
    )])
}

fn source_json(values: &[i64]) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"Rows": values.iter().map(|value| serde_json::json!({"Value": value})).collect::<Vec<_>>() })).unwrap()
}

fn output_values(instance: &Instance) -> Vec<i64> {
    let entry = instance.field("Entry").expect("Entry output");
    let entries: Vec<&Instance> = match entry {
        Instance::MappedSequence(items) => items.iter().collect(),
        Instance::Group(_) => vec![entry],
        other => panic!("XML element output, got {other:?}"),
    };
    entries
        .into_iter()
        .filter_map(
            |entry| match entry.field("Value").and_then(Instance::as_scalar) {
                Some(Value::Int(value)) => Some(*value),
                None => None,
                other => panic!("integer Value output, got {other:?}"),
            },
        )
        .collect()
}

fn expected_xml(values: &[i64], empty_group: bool) -> Vec<u8> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Target>");
    for value in values {
        xml.push_str(&format!(
            "\n  <Entry>\n    <Value>{value}</Value>\n  </Entry>"
        ));
    }
    if empty_group {
        xml.push_str("\n  <Entry>\n  </Entry>");
    }
    xml.push('\n');
    xml.push_str("</Target>");
    xml.into_bytes()
}

fn assert_outputs(
    app: &FerruleApp,
    input: &[i64],
    active: &[i64],
    other: &[i64],
    empty_group: bool,
) {
    assert!(
        cli::validate(&app.project).is_empty(),
        "valid authored mapping"
    );
    let outputs = engine::run_outputs(&app.project, &source(input)).expect("interpreter output");
    let named_active = matches!(app.mapping_workspace.active, MappingDocument::Target(_));
    let (selected, unchanged) = if named_active {
        (&outputs.extras[0].instance, &outputs.primary)
    } else {
        (&outputs.primary, &outputs.extras[0].instance)
    };
    assert_eq!(output_values(selected), active);
    assert_eq!(output_values(unchanged), other);
    assert_eq!(
        matches!(selected.field("Entry"), Some(Instance::Group(_))),
        active_scope(app).iteration_output == IterationOutput::First
    );
    let bytes = source_json(input);
    let payload = cli::run_project_value_payloads(
        &app.project,
        app.document
            .saved_path()
            .unwrap_or(Path::new("/tmp/ferrule-xml-output-controls/project.json")),
        &cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(Path::new("input.json"), &bytes).unwrap(),
        ),
    )
    .expect("actual format_xml payloads");
    assert_eq!(payload.artifacts.len(), 2);
    let (selected, unchanged) = if named_active {
        (&payload.artifacts[1], &payload.artifacts[0])
    } else {
        (&payload.artifacts[0], &payload.artifacts[1])
    };
    assert_eq!(selected.bytes, expected_xml(active, empty_group));
    assert_eq!(unchanged.bytes, expected_xml(other, false));
}

fn choose_output(app: &mut FerruleApp, context: &egui::Context, first: bool) {
    let selected = match active_scope(app).iteration_output {
        IterationOutput::Repeated => "Repeated output",
        IterationOutput::First => "First selected element",
        IterationOutput::MappedSequence => "Every selected element",
    };
    choose(
        app,
        context,
        selected,
        if first {
            "First selected element"
        } else {
            "Every selected element"
        },
    );
}

fn project_state(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).expect("project state")
}

fn gui_context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_inspector(ui, editing_enabled),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn rendered_labels(shape: &egui::epaint::Shape, label: &str, positions: &mut Vec<egui::Pos2>) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            positions.push(text.visual_bounding_rect().center());
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                rendered_labels(shape, label, positions);
            }
        }
        _ => {}
    }
}

fn click(
    app: &mut FerruleApp,
    context: &egui::Context,
    editing_enabled: bool,
    label: &str,
    occurrence: usize,
) {
    let mut output = frame(app, context, editing_enabled, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, editing_enabled, Vec::new());
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        rendered_labels(&shape.shape, label, &mut positions);
    }
    // Dropdown items are painted after their selected text in the editor.
    let position = if occurrence == usize::MAX {
        positions.last()
    } else {
        positions.get(occurrence)
    }
    .copied()
    .unwrap_or_else(|| panic!("rendered Inspector control {label:?} occurrence {occurrence}"));
    for pressed in [true, false] {
        let _ = frame(
            app,
            context,
            editing_enabled,
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

fn choose(app: &mut FerruleApp, context: &egui::Context, selected: &str, desired: &str) {
    click(app, context, true, selected, 0);
    click(app, context, true, desired, usize::MAX);
}

#[test]
fn xml_output_pointer_selection_preserves_schema_controls_and_other_target() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = xml_app(document);
        let original = project_state(&app.project);
        let schema =
            serde_json::to_value((&app.project.target, &app.project.extra_targets[0].schema))
                .unwrap();
        let graph = serde_json::to_value(&app.project.graph).unwrap();
        let context = gui_context();
        for _ in 0..4 {
            let _ = frame(&mut app, &context, true, Vec::new());
        }
        assert_eq!(
            project_state(&app.project),
            original,
            "display retains imported mode"
        );
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert_outputs(
            &app,
            &[8, 2, 5, 1, 7, 3, 6, 4],
            &[3, 4, 5],
            &[3, 4, 5],
            false,
        );
        choose_output(&mut app, &context, true);
        assert_eq!(active_scope(&app).iteration_output, IterationOutput::First);
        assert_eq!(active_scope(&app).sort_by, Some(10));
        assert_eq!(
            active_scope(&app).windows,
            [
                SequenceWindow::SkipFirst { count: 1 },
                SequenceWindow::First { count: 2 }
            ]
        );
        assert_outputs(&app, &[8, 2, 5, 1, 7, 3, 6, 4], &[3], &[3, 4, 5], false);
        assert!(app.is_dirty());
        assert!(app.can_undo());
        choose_output(&mut app, &context, false);
        assert_outputs(
            &app,
            &[8, 2, 5, 1, 7, 3, 6, 4],
            &[3, 4, 5],
            &[3, 4, 5],
            false,
        );
        assert_eq!(project_state(&app.project), original);
        assert_eq!(
            serde_json::to_value((&app.project.target, &app.project.extra_targets[0].schema))
                .unwrap(),
            schema
        );
        assert_eq!(serde_json::to_value(&app.project.graph).unwrap(), graph);
    }
}

#[test]
fn xml_output_zero_survivors_keep_first_group_and_omit_mapped_elements() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = xml_app(document);
        let context = gui_context();
        assert_outputs(&app, &[1, 2], &[], &[], false);
        choose_output(&mut app, &context, true);
        assert_outputs(&app, &[1, 2], &[], &[], true);
        assert_outputs(&app, &[3, 2, 1], &[3], &[3], false);
        choose_output(&mut app, &context, false);
        assert_outputs(&app, &[1, 2], &[], &[], false);
    }
}

fn temporary_project_path() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "ferrule-gui-xml-output-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join("project.json")
}

#[test]
fn xml_output_undo_redo_and_actual_save_reopen_preserve_element_mode() -> anyhow::Result<()> {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = xml_app(document);
        let original = project_state(&app.project);
        let context = gui_context();
        choose_output(&mut app, &context, true);
        let changed = project_state(&app.project);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(app.history.can_redo());
        assert_outputs(
            &app,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[3, 4, 5],
            &[3, 4, 5],
            false,
        );
        app.redo_project();
        assert_eq!(project_state(&app.project), changed);
        assert!(app.is_dirty());
        assert!(app.can_undo());
        assert!(!app.history.can_redo());
        assert_outputs(&app, &[1, 2, 3, 4, 5, 6, 7, 8], &[3], &[3, 4, 5], false);
        let path = temporary_project_path();
        let saved = app.save_document_to(&path)?;
        assert!(saved.validation_issues.is_empty());
        assert!(saved.layout_warning.is_none());
        assert!(!app.is_dirty());
        let state = project_state(&app.project);
        let mut reopened = FerruleApp::default();
        reopened.load_project_from(&path);
        if let MappingDocument::Target(index) = document {
            reopened.open_target_tab(index);
        }
        reopened.selected_scope = vec![0];
        assert_eq!(project_state(&reopened.project), state);
        assert_eq!(
            active_scope(&reopened).iteration_output,
            IterationOutput::First
        );
        assert!(!reopened.is_dirty());
        assert!(!reopened.can_undo());
        assert_outputs(
            &reopened,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[3],
            &[3, 4, 5],
            false,
        );
        let context = gui_context();
        choose_output(&mut reopened, &context, false);
        assert_outputs(
            &reopened,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[3, 4, 5],
            &[3, 4, 5],
            false,
        );
        std::fs::remove_dir_all(path.parent().unwrap())?;
    }
    Ok(())
}

#[test]
fn locked_xml_output_selector_preserves_project_history_and_payloads() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = xml_app(document);
        app.begin_preview();
        assert!(app.preview_draft.is_some());
        let original = project_state(&app.project);
        let context = gui_context();
        click(&mut app, &context, false, "Every selected element", 0);
        assert_eq!(project_state(&app.project), original);
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(!app.history.can_redo());
        assert!(app.pending_history.is_none());
        assert!(app.preview_draft.is_some());
        assert_outputs(
            &app,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[3, 4, 5],
            &[3, 4, 5],
            false,
        );
    }
}

#[test]
fn own_whole_group_copy_can_select_xml_output_without_becoming_constructed() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = xml_app(document);
        let scope = match document {
            MappingDocument::Target(index) => {
                &mut app.project.extra_targets[index].root.children[0]
            }
            _ => &mut app.project.root.children[0],
        };
        scope.construction = ScopeConstruction::CopyCurrentSource;
        scope.bindings.clear();
        app.mark_clean();
        app.rebase_history();
        let original = project_state(&app.project);
        let context = gui_context();
        assert_outputs(
            &app,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[3, 4, 5],
            &[3, 4, 5],
            false,
        );
        choose_output(&mut app, &context, true);
        assert_eq!(
            active_scope(&app).construction,
            ScopeConstruction::CopyCurrentSource
        );
        assert!(active_scope(&app).bindings.is_empty());
        assert!(active_scope(&app).children.is_empty());
        assert_outputs(&app, &[1, 2, 3, 4, 5, 6, 7, 8], &[3], &[3, 4, 5], false);
        app.undo_project();
        assert_eq!(project_state(&app.project), original);
    }
}

#[test]
fn xml_output_profile_follows_adapter_precedence_and_active_target() {
    for document in [MappingDocument::Main, MappingDocument::Target(0)] {
        let mut app = xml_app(document);
        // Only the other target uses JSON, although its filename still ends in XML.
        match document {
            MappingDocument::Main => {
                app.project.extra_targets[0].options = FormatOptions {
                    json_document: true,
                    ..FormatOptions::default()
                };
                app.project.extra_targets[0].path = Some("other.xml".into());
            }
            MappingDocument::Target(_) => {
                app.project.target_options = FormatOptions {
                    json_document: true,
                    ..FormatOptions::default()
                };
                app.project.target_path = Some("main.xml".into());
            }
            _ => unreachable!(),
        }
        app.mark_clean();
        app.rebase_history();
        let context = gui_context();
        choose_output(&mut app, &context, true);
        assert_eq!(active_scope(&app).iteration_output, IterationOutput::First);
        match document {
            MappingDocument::Main => {
                assert_eq!(
                    app.project.extra_targets[0].root.children[0].iteration_output,
                    IterationOutput::MappedSequence
                );
                app.open_target_tab(0);
            }
            _ => {
                assert_eq!(
                    app.project.root.children[0].iteration_output,
                    IterationOutput::MappedSequence
                );
                app.mapping_workspace.active = MappingDocument::Main;
                app.mapping_workspace.focused = MappingDocument::Main;
            }
        }
        app.selected_scope = vec![0];
        app.mark_clean();
        app.rebase_history();
        let original = project_state(&app.project);
        click(&mut app, &context, true, "Every selected element", 0);
        assert_eq!(project_state(&app.project), original);
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
    }
    let app = xml_app(MappingDocument::Main);
    let check = |options: &FormatOptions, path| {
        output_profile(&app.project.root, &app.project.target, &[0], options, path)
    };
    assert_eq!(
        check(&FormatOptions::default(), Some("legacy.XML")),
        ScopeOutputProfile::XmlElements
    );
    assert_eq!(
        check(&app.project.target_options, None),
        ScopeOutputProfile::XmlElements
    );
    assert_eq!(
        check(&app.project.target_options, Some("output.csv")),
        ScopeOutputProfile::XmlElements
    );
    for options in [
        FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            json5: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            json_lines: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            xml_document: true,
            delimiter: Some(','),
            ..FormatOptions::default()
        },
        FormatOptions {
            edi_kind: Some(mapping::EdiBoundaryKind::X12),
            ..FormatOptions::default()
        },
        FormatOptions {
            fixed_width: Some(
                mapping::FixedWidthLayout::new(
                    vec![mapping::FixedFieldWidth::new(4).unwrap()],
                    ' ',
                    true,
                    true,
                )
                .unwrap(),
            ),
            ..FormatOptions::default()
        },
    ] {
        assert!(matches!(
            check(&options, Some("output.xml")),
            ScopeOutputProfile::ReadOnly(_)
        ));
    }
    assert!(matches!(
        check(&FormatOptions::default(), Some("output.csv")),
        ScopeOutputProfile::ReadOnly(_)
    ));
}

#[test]
fn unsupported_saved_output_modes_stay_visible_without_render_normalization() {
    for case in [
        "repeating",
        "no iteration",
        "merge",
        "copied ancestor",
        "generic",
        "recursive",
        "alternative",
        "scalar",
        "output path",
        "concatenated",
        "type alternative",
        "root",
    ] {
        let mut app = xml_app(MappingDocument::Main);
        let SchemaKind::Group { children, .. } = &mut app.project.target.kind else {
            unreachable!()
        };
        match case {
            "repeating" => children[0].repeating = true,
            "no iteration" => app.project.root.children[0].iteration = ScopeIteration::None,
            "merge" => app.project.root.children[0].merge_dynamic_fields = true,
            "copied ancestor" => {
                app.project.root.construction = ScopeConstruction::CopyCurrentSource
            }
            "generic" => {
                children[0].name = ir::XML_ELEMENTS_FIELD.into();
                app.project.root.children[0].target_field = ir::XML_ELEMENTS_FIELD.into();
            }
            "recursive" => {
                children[0] = SchemaNode::group("Entry", Vec::new());
                children[0].recursive_ref = Some("Target".into());
            }
            "scalar" => children[0] = SchemaNode::scalar("Entry", ScalarType::Int),
            "output path" => {
                app.project.root.children[0].set_output_path(Some(1));
            }
            "concatenated" => {
                let mut segment = app.project.root.children[0].clone();
                segment.target_field.clear();
                app.project.root.children[0].iteration =
                    ScopeIteration::Concatenate(mapping::ScopeSequence::new(segment, Vec::new()));
            }
            "type alternative" => {
                children[0] = children[0]
                    .clone()
                    .with_alternatives(vec![ir::GroupAlternative {
                        name: "SelectedType".into(),
                        members: vec!["Value".into()],
                        required: vec![],
                        constraints: vec![],
                    }])
                    .expect("valid alternative schema");
            }
            "alternative" => {
                children[0] = children[0]
                    .clone()
                    .xml_qualified("urn:ferrule:primary")
                    .and_then(|node| {
                        node.with_xml_name_alternatives(vec![
                            ir::XmlNamespace::qualified("urn:ferrule:alternate")
                                .expect("qualified name"),
                        ])
                    })
                    .expect("valid qualified name alternatives");
            }
            "root" => {
                app.project.root.iteration = ScopeIteration::Source(vec!["Rows".into()]);
                app.project.root.iteration_output = IterationOutput::MappedSequence;
                app.selected_scope.clear();
            }
            _ => unreachable!(),
        }
        let profile = output_profile(
            &app.project.root,
            &app.project.target,
            &app.selected_scope,
            &app.project.target_options,
            app.project.target_path.as_deref(),
        );
        assert!(matches!(profile, ScopeOutputProfile::ReadOnly(_)), "{case}");
        app.mark_clean();
        app.rebase_history();
        if case != "root" {
            app.selected_scope = vec![0];
        }
        let original = project_state(&app.project);
        let context = gui_context();
        click(&mut app, &context, true, "Every selected element", 0);
        assert_eq!(project_state(&app.project), original, "unsupported {case}");
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(!app.history.can_redo());
        assert!(app.pending_history.is_none());
    }
    let mut app = xml_app(MappingDocument::Main);
    app.project.root.children[0].iteration_output = IterationOutput::Repeated;
    app.project.target_options.json_document = true;
    app.mark_clean();
    app.rebase_history();
    let original = project_state(&app.project);
    let context = gui_context();
    click(&mut app, &context, true, "Repeated output", 0);
    assert_eq!(project_state(&app.project), original);
    assert!(!app.can_undo());
    assert!(matches!(
        output_profile(
            &app.project.root,
            &app.project.target,
            &[99],
            &app.project.target_options,
            None
        ),
        ScopeOutputProfile::ReadOnly(_)
    ));
}
