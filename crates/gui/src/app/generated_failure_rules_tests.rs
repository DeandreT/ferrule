use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DynamicSourcePath, FormatOptions, NamedSource, NamedTarget};

const SUCCESS: &str = "0: constant String(\"success\")";
const FALSE: &str = "1: constant Bool(false)";

fn fixture() -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group("Source", Vec::new());
    app.project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    app.project.source_path = Some("input.json".into());
    app.project.target_path = Some("output.json".into());
    app.project.source_options.json_document = true;
    app.project.target_options.json_document = true;
    app.project.graph.nodes.clear();
    app.project.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::String("success".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            2,
            Node::Call {
                function: "divide".into(),
                args: vec![3, 4],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::Int(0),
            },
        ),
        (
            10,
            Node::Const {
                value: Value::String("red,green".into()),
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
    ]);
    app.project.root = Scope {
        bindings: vec![Binding {
            target_field: "Result".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    app.project.failure_rules.clear();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    app
}

fn source() -> Instance {
    Instance::Group(Vec::new().into())
}

fn success() -> Instance {
    Instance::Group(
        vec![(
            "Result".into(),
            Instance::Scalar(Value::String("success".into())),
        )]
        .into(),
    )
}

fn encoded(app: &FerruleApp) -> String {
    mapping::project_file::encode_pretty(&app.project).expect("project encoding")
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 1700.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_failure_rules(ui, true),
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    output
}

fn settle(app: &mut FerruleApp, context: &egui::Context) -> egui::FullOutput {
    let mut output = frame(app, context, Vec::new());
    for _ in 0..7 {
        output = frame(app, context, Vec::new());
    }
    output
}

fn text_positions(
    shape: &egui::epaint::Shape,
    clip: egui::Rect,
    label: &str,
    positions: &mut Vec<egui::Pos2>,
) {
    match shape {
        egui::epaint::Shape::Text(text) if text.galley.text() == label => {
            let center = text.visual_bounding_rect().center();
            if clip.contains(center) {
                positions.push(center);
            }
        }
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                text_positions(shape, clip, label, positions);
            }
        }
        _ => {}
    }
}

fn positions(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    let mut found = Vec::new();
    for shape in &output.shapes {
        text_positions(&shape.shape, shape.clip_rect, label, &mut found);
    }
    found
}

fn click_at(app: &mut FerruleApp, context: &egui::Context, position: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            context,
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

fn click_nth(app: &mut FerruleApp, context: &egui::Context, label: &str, occurrence: usize) {
    let output = settle(app, context);
    let visible = positions(&output, label);
    let position = *visible
        .get(occurrence)
        .unwrap_or_else(|| panic!("visible control {label:?} #{occurrence}"));
    click_at(app, context, position);
}

fn click(app: &mut FerruleApp, context: &egui::Context, label: &str) {
    click_nth(app, context, label, 0);
}

fn pick(
    app: &mut FerruleApp,
    context: &egui::Context,
    current: &str,
    occurrence: usize,
    wanted: &str,
) {
    click_nth(app, context, current, occurrence);
    let mut output = settle(app, context);
    if positions(&output, wanted).is_empty() {
        // The popup has its own bounded scroll area. Scroll over its last
        // visible upper entry, not an off-screen text galley or the editor.
        let anchor = *positions(&output, FALSE)
            .last()
            .expect("visible expression popup entry");
        frame(
            app,
            context,
            vec![
                egui::Event::PointerMoved(anchor),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -600.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        output = settle(app, context);
    }
    let point = *positions(&output, wanted)
        .last()
        .unwrap_or_else(|| panic!("visible popup choice {wanted:?}"));
    click_at(app, context, point);
}

fn item(app: &FerruleApp, index: usize) -> NodeId {
    match &app.project.failure_rules[index].iteration {
        FailureIteration::Sequence { sequence } => sequence.item(),
        other => panic!("generated rule: {other:?}"),
    }
}

fn item_message(app: &mut FerruleApp, context: &egui::Context, index: usize) {
    let item = item(app, index);
    click(app, context, "Custom message expression");
    pick(
        app,
        context,
        SUCCESS,
        0,
        &format!("{item}: field <current>"),
    );
    assert_eq!(app.project.failure_rules[index].message, Some(item));
}

#[test]
fn generated_integer_range_real_controls_use_first_item_and_skip_unselected_message() {
    let mut app = fixture();
    let context = context();
    click(&mut app, &context, "Add integer range rule");
    assert_eq!(app.project.graph.nodes.len(), 10);
    assert_eq!(item(&app, 0), 14);
    assert!(
        app.main_canvas
            .snarl
            .nodes()
            .any(|node| *node == CanvasNode::Graph(14))
    );
    item_message(&mut app, &context, 0);
    assert!(cli::validate(&app.project).is_empty());
    assert_eq!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("1".into()),
        })
    );
    pick(
        &mut app,
        &context,
        "13: constant Int(3)",
        0,
        "4: constant Int(0)",
    );
    assert_eq!(
        engine::run(&app.project, &source()),
        Ok(success()),
        "empty range has no matching item"
    );
    assert_eq!(item(&app, 0), 14);
    pick(
        &mut app,
        &context,
        "4: constant Int(0)",
        0,
        "13: constant Int(3)",
    );
    click(&mut app, &context, "All items");
    click(&mut app, &context, "Expression is true");
    pick(&mut app, &context, SUCCESS, 0, FALSE);
    pick(&mut app, &context, "14: field <current>", 0, "2: divide");
    assert_eq!(
        app.project.failure_rules[0].selection,
        FailureSelection::WhenTrue { predicate: 1 }
    );
    assert_eq!(app.project.failure_rules[0].message, Some(2));
    assert!(cli::validate(&app.project).is_empty());
    assert_eq!(
        engine::run(&app.project, &source()),
        Ok(success()),
        "nonmatching message remains lazy"
    );
    click(&mut app, &context, "Expression is true");
    click(&mut app, &context, "Expression is false");
    assert!(
        matches!(
            engine::run(&app.project, &source()),
            Err(engine::EngineError::Function(_))
        ),
        "matching message is now evaluated"
    );
    pick(&mut app, &context, "2: divide", 0, "14: field <current>");
    assert_eq!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("1".into()),
        })
    );
    assert!(app.is_dirty());
}

#[test]
fn generated_split_text_real_argument_picker_reads_first_token_and_order_is_lazy() {
    let mut app = fixture();
    let context = context();
    click(&mut app, &context, "Add split text rule");
    pick(
        &mut app,
        &context,
        "12: constant String(\"first,second\")",
        0,
        "10: constant String(\"red,green\")",
    );
    pick(
        &mut app,
        &context,
        "13: constant String(\",\")",
        0,
        "11: constant String(\",\")",
    );
    item_message(&mut app, &context, 0);
    let first = app.project.failure_rules[0].clone();
    assert!(matches!(
        &first.iteration,
        FailureIteration::Sequence {
            sequence: SequenceExpr::Tokenize {
                input: 10,
                delimiter: 11,
                item: 14
            }
        }
    ));
    assert_eq!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("red".into()),
        })
    );
    click(&mut app, &context, "Add integer range rule");
    click(&mut app, &context, "Custom message expression");
    pick(&mut app, &context, SUCCESS, 0, "2: divide");
    assert!(cli::validate(&app.project).is_empty());
    assert_eq!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("red".into()),
        }),
        "a later rule's failing message is not evaluated"
    );
    click(&mut app, &context, "Move up");
    assert!(matches!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::Function(_))
    ));
    app.undo_project();
    assert_eq!(app.project.failure_rules[0], first);
    assert_eq!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("red".into()),
        })
    );
    app.redo_project();
    assert!(matches!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::Function(_))
    ));
}

fn open_named_canvas(app: &mut FerruleApp) {
    app.project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some("other.json".into()),
        schema: app.project.target.clone(),
        options: app.project.target_options.clone(),
        root: app.project.root.clone(),
    });
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0));
    app.mapping_workspace.active = MappingDocument::Main;
    app.mapping_workspace.focused = MappingDocument::Main;
    for (snarl, offset) in [
        (&mut app.main_canvas.snarl, egui::vec2(0.0, 0.0)),
        (
            &mut app
                .mapping_workspace
                .target_canvases
                .get_mut(&0)
                .unwrap()
                .snarl,
            egui::vec2(300.0, 100.0),
        ),
    ] {
        let id = snarl
            .node_ids()
            .find_map(|(id, node)| (*node == CanvasNode::Graph(0)).then_some(id))
            .unwrap();
        snarl.get_node_info_mut(id).unwrap().pos = egui::pos2(125.0, 85.0) + offset;
    }
    app.mark_clean();
    app.rebase_history();
}

fn assert_surviving_positions(
    before: &[CanvasNodeLayout],
    snarl: &egui_snarl::Snarl<CanvasNode>,
    retired: Option<NodeId>,
) {
    let after = CanvasLayout::capture_nodes(snarl);
    for node in before {
        if retired.is_some_and(|item| node.node == (PersistedCanvasNode::Graph { id: item })) {
            continue;
        }
        assert_eq!(
            after.iter().find(|current| current.node == node.node),
            Some(node)
        );
    }
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule-generated-failure-editor-{}-{index}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!(
                "retained generated failure-rule artifacts: {}",
                self.0.display()
            );
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn generated_rule_removal_retires_only_item_across_canvases_and_undo_save_reopen() {
    let directory = Directory::new();
    let origin = directory.0.join("origin");
    let destination = directory.0.join("saved");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    for name in ["input.json", "output.json", "other.json"] {
        std::fs::write(origin.join(name), b"{}").unwrap();
    }
    let mut app = fixture();
    app.document = DocumentLocation::untitled(origin.join("mapping.json"));
    open_named_canvas(&mut app);
    let original = encoded(&app);
    let main_positions = CanvasLayout::capture_nodes(&app.main_canvas.snarl);
    let named_positions =
        CanvasLayout::capture_nodes(&app.mapping_workspace.target_canvases[&0].snarl);
    let context = context();
    click(&mut app, &context, "Add integer range rule");
    let generated = encoded(&app);
    let generated_layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    let owned = item(&app, 0);
    for snarl in [
        &app.main_canvas.snarl,
        &app.mapping_workspace.target_canvases[&0].snarl,
    ] {
        assert!(snarl.nodes().any(|node| *node == CanvasNode::Graph(owned)));
    }
    assert_surviving_positions(&main_positions, &app.main_canvas.snarl, None);
    assert_surviving_positions(
        &named_positions,
        &app.mapping_workspace.target_canvases[&0].snarl,
        None,
    );
    click(&mut app, &context, "Remove rule");
    assert!(app.project.failure_rules.is_empty());
    assert!(!app.project.graph.nodes.contains_key(&owned));
    assert!(app.project.graph.nodes.contains_key(&12));
    assert!(app.project.graph.nodes.contains_key(&13));
    for snarl in [
        &app.main_canvas.snarl,
        &app.mapping_workspace.target_canvases[&0].snarl,
    ] {
        assert!(snarl.nodes().all(|node| *node != CanvasNode::Graph(owned)));
    }
    assert_surviving_positions(&main_positions, &app.main_canvas.snarl, None);
    assert_surviving_positions(
        &named_positions,
        &app.mapping_workspace.target_canvases[&0].snarl,
        None,
    );
    assert!(cli::validate(&app.project).is_empty());
    assert_eq!(engine::run(&app.project, &source()), Ok(success()));
    let removed = encoded(&app);
    assert_ne!(
        removed, original,
        "ordinary default argument nodes are retained"
    );
    app.undo_project();
    assert_eq!(encoded(&app), generated);
    assert_eq!(
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
        generated_layout
    );
    app.redo_project();
    assert_eq!(encoded(&app), removed);
    let before_save = app.project.clone();
    let path = destination.join("mapping.json");
    app.save_document_to(&path).unwrap();
    let saved = encoded(&app);
    let saved_file = std::fs::read_to_string(&path).unwrap();
    eprintln!("generated removal Save As original saved={saved} file={saved_file}");
    assert_eq!(
        saved_file, saved,
        "saved bytes match the rebased editor snapshot"
    );
    let mut unchanged = app.project.clone();
    unchanged.source_path = before_save.source_path.clone();
    unchanged.target_path = before_save.target_path.clone();
    unchanged.extra_targets[0].path = before_save.extra_targets[0].path.clone();
    assert_eq!(
        mapping::project_file::encode_pretty(&unchanged).unwrap(),
        removed,
        "Save As changes only the three boundary paths, preserving graph, rules and bindings"
    );
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    assert_eq!(encoded(&reopened), saved);
    for (stored, name) in [
        (
            reopened.project.source_path.as_deref().unwrap(),
            "input.json",
        ),
        (
            reopened.project.target_path.as_deref().unwrap(),
            "output.json",
        ),
        (
            reopened.project.extra_targets[0].path.as_deref().unwrap(),
            "other.json",
        ),
    ] {
        assert!(
            !Path::new(stored).is_absolute(),
            "Save As retains relative boundary paths"
        );
        assert_eq!(
            std::fs::canonicalize(destination.join(stored)).unwrap(),
            std::fs::canonicalize(origin.join(name)).unwrap(),
            "{name} still names its original physical boundary after Save As"
        );
        assert_eq!(std::fs::read(origin.join(name)).unwrap(), b"{}");
    }
    assert!(!reopened.is_dirty());
    assert_eq!(engine::run(&reopened.project, &source()), Ok(success()));
    assert!(reopened.project.graph.nodes.contains_key(&12));
    assert!(reopened.project.graph.nodes.contains_key(&13));
    assert!(!reopened.project.graph.nodes.contains_key(&owned));
}

#[test]
fn imported_supported_generated_rule_keeps_item_identity_kind_and_default_start() {
    let mut app = fixture();
    app.project.graph.nodes.insert(
        42,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        51,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        50,
        Node::SequenceAggregate {
            function: mapping::AggregateOp::Sum,
            sequence: SequenceExpr::Generate {
                from: None,
                to: 3,
                item: 51,
            },
            predicate: None,
            expression: Some(51),
            arg: None,
        },
    );
    app.project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 50,
                item: 42,
            },
        },
        selection: FailureSelection::All,
        message: None,
    });
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.selected_failure_rule = Some(0);
    app.mark_clean();
    app.rebase_history();
    assert!(cli::validate(&app.project).is_empty());
    assert!(
        editable_generated_rule(&app.project, &app.project.failure_rules[0]),
        "a nested reducer's private expression does not leak into the parent argument context"
    );
    let context = context();
    click(&mut app, &context, "Rule 1: generated sequence");
    item_message(&mut app, &context, 0);
    assert!(matches!(
        &app.project.failure_rules[0].iteration,
        FailureIteration::Sequence {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 50,
                item: 42
            }
        }
    ));
    assert_eq!(
        engine::run(&app.project, &source()),
        Err(engine::EngineError::MappingFailure {
            rule: 1,
            message: Some("1".into()),
        })
    );
    let directory = Directory::new();
    let path = directory.0.join("imported.json");
    app.save_document_to(&path).unwrap();
    let mut reopened = FerruleApp::default();
    reopened.load_project_from(&path);
    assert_eq!(reopened.project.failure_rules, app.project.failure_rules);
    assert_eq!(
        serde_json::to_value(reopened.project.graph.nodes.get(&42)).unwrap(),
        serde_json::to_value(app.project.graph.nodes.get(&42)).unwrap(),
    );
}

#[test]
fn generated_rule_other_kinds_missing_invalid_and_shared_items_remain_unchanged() {
    for case in 0..12 {
        let mut app = fixture();
        app.project.graph.nodes.insert(
            42,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
        let sequence = if case == 0 {
            SequenceExpr::TokenizeByLength {
                input: 10,
                length: 3,
                item: 42,
            }
        } else {
            SequenceExpr::Generate {
                from: None,
                to: 3,
                item: 42,
            }
        };
        app.project.failure_rules.push(FailureRule {
            iteration: FailureIteration::Sequence { sequence },
            selection: FailureSelection::WhenTrue { predicate: 1 },
            message: Some(42),
        });
        match case {
            0 => assert!(cli::validate(&app.project).is_empty()),
            1 => {
                app.project.graph.nodes.remove(&42);
            }
            2 => {
                app.project.graph.nodes.insert(
                    42,
                    Node::SourceField {
                        path: vec!["existing".into()],
                        frame: None,
                    },
                );
            }
            3 => {
                app.project.graph.nodes.insert(
                    42,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: Some(Vec::new()),
                    },
                );
            }
            4 => {
                app.project.graph.nodes.insert(
                    42,
                    Node::Const {
                        value: Value::Int(9),
                    },
                );
            }
            5 => {
                app.project
                    .failure_rules
                    .push(app.project.failure_rules[0].clone());
            }
            6 => {
                app.project.graph.nodes.remove(&3);
            }
            7 => {
                app.project.graph.nodes.insert(
                    50,
                    Node::SequenceExists {
                        sequence: SequenceExpr::Generate {
                            from: None,
                            to: 3,
                            item: 42,
                        },
                        predicate: 1,
                    },
                );
            }
            8 => {
                app.project.extra_targets.push(NamedTarget {
                    name: "Shared".into(),
                    path: Some("shared.json".into()),
                    schema: app.project.target.clone(),
                    options: app.project.target_options.clone(),
                    root: Scope {
                        iteration: mapping::ScopeIteration::Sequence(SequenceExpr::Generate {
                            from: None,
                            to: 3,
                            item: 42,
                        }),
                        ..Scope::default()
                    },
                });
            }
            9 => {
                app.project.failure_rules[0].iteration = FailureIteration::Sequence {
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 42,
                        item: 42,
                    },
                };
            }
            10 => {
                app.project.graph.nodes.insert(
                    50,
                    Node::Call {
                        function: "string".into(),
                        args: vec![42],
                    },
                );
                app.project.failure_rules[0].iteration = FailureIteration::Sequence {
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 50,
                        item: 42,
                    },
                };
            }
            11 => {
                app.project.graph.nodes.insert(
                    50,
                    Node::Call {
                        function: "add".into(),
                        args: vec![50, 3],
                    },
                );
                app.project.failure_rules[0].iteration = FailureIteration::Sequence {
                    sequence: SequenceExpr::Generate {
                        from: None,
                        to: 50,
                        item: 42,
                    },
                };
            }
            _ => unreachable!(),
        }
        app.main_canvas = CanvasDocumentState::main(&app.project);
        app.selected_failure_rule = Some(0);
        app.mark_clean();
        app.rebase_history();
        let original = encoded(&app);
        let layout =
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        let context = context();
        click(&mut app, &context, "Rule 1: generated sequence");
        click(&mut app, &context, "Remove rule");
        app.apply_failure_rule_action(RuleAction::Remove(0), true);
        assert_eq!(encoded(&app), original, "case {case}");
        assert_eq!(
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
            layout
        );
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        if case == 0 {
            assert_eq!(engine::run(&app.project, &source()), Ok(success()));
        }
    }
}

#[test]
fn generated_rule_remaining_cross_canvas_graph_rule_and_dynamic_references_block_removal() {
    for case in 0..8 {
        let mut app = fixture();
        open_named_canvas(&mut app);
        app.apply_failure_rule_action(
            RuleAction::AddSequence(GeneratedRuleKind::IntegerRange),
            true,
        );
        let item = item(&app, 0);
        match case {
            0 => {
                app.project.graph.nodes.insert(
                    30,
                    Node::Call {
                        function: "string".into(),
                        args: vec![item],
                    },
                );
            }
            1 => {
                app.project.root.bindings[0].node = item;
            }
            2 => {
                app.project.extra_targets[0].root.bindings[0].node = item;
            }
            3 => {
                app.project.failure_rules.push(FailureRule {
                    iteration: FailureIteration::Source {
                        collection: Vec::new(),
                    },
                    selection: FailureSelection::All,
                    message: Some(item),
                });
            }
            4 => {
                app.project.extra_sources.push(NamedSource {
                    name: "Dynamic".into(),
                    path: "reference.json".into(),
                    schema: app.project.source.clone(),
                    options: FormatOptions::default(),
                    dynamic_path: Some(DynamicSourcePath {
                        node: item,
                        iteration: Vec::new(),
                    }),
                });
            }
            5 => {
                app.project.root.filter = Some(item);
            }
            6 => {
                app.project
                    .root
                    .dynamic_bindings
                    .push(mapping::DynamicBinding {
                        key: item,
                        value: 0,
                    });
            }
            7 => {
                app.project
                    .root
                    .dynamic_children
                    .push(mapping::DynamicChild {
                        key: item,
                        scope: Scope::default(),
                    });
            }
            _ => unreachable!(),
        }
        app.rebuild_mapping_canvases_after_retirement();
        app.mark_clean();
        app.rebase_history();
        let original = encoded(&app);
        let original_layout =
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
        let context = context();
        click(&mut app, &context, "Rule 1: generated sequence");
        click(&mut app, &context, "Remove rule");
        assert_eq!(encoded(&app), original, "case {case}");
        assert_eq!(
            CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
            original_layout
        );
        assert!(app.project.graph.nodes.contains_key(&item));
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert!(
            app.diagnostics
                .items()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("disconnect those references")),
            "case {case}"
        );
        if case == 0 {
            // A user disconnects the remaining consumer before retrying.
            app.project.graph.nodes.remove(&30);
            app.rebuild_mapping_canvases_after_retirement();
            app.mark_clean();
            app.rebase_history();
            click(&mut app, &context, "Rule 1: generated sequence");
            click(&mut app, &context, "Remove rule");
            assert!(app.project.failure_rules.is_empty());
            assert!(!app.project.graph.nodes.contains_key(&item));
            assert!(app.project.graph.nodes.contains_key(&12));
            assert!(app.project.graph.nodes.contains_key(&13));
            assert!(cli::validate(&app.project).is_empty());
        }
    }
}

#[test]
fn generated_rule_id_exhaustion_and_missing_owned_ids_reserve_atomically() {
    for kind in [
        GeneratedRuleKind::IntegerRange,
        GeneratedRuleKind::SplitText,
    ] {
        for maximum in [NodeId::MAX, NodeId::MAX - 2] {
            let mut app = fixture();
            app.project.graph.nodes.insert(
                maximum,
                Node::Const {
                    value: Value::Int(0),
                },
            );
            app.main_canvas = CanvasDocumentState::main(&app.project);
            app.mark_clean();
            app.rebase_history();
            let before = encoded(&app);
            let layout =
                CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
            let context = context();
            click(
                &mut app,
                &context,
                match kind {
                    GeneratedRuleKind::IntegerRange => "Add integer range rule",
                    GeneratedRuleKind::SplitText => "Add split text rule",
                },
            );
            app.apply_failure_rule_action(RuleAction::AddSequence(kind), true);
            assert_eq!(encoded(&app), before);
            assert_eq!(
                CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
                layout
            );
            assert!(!app.is_dirty());
            assert!(!app.can_undo());
            assert!(
                app.diagnostics
                    .items()
                    .iter()
                    .any(|item| item.message == "mapping node IDs are exhausted")
            );
        }
        let mut app = fixture();
        app.project.graph.nodes.insert(
            NodeId::MAX - 4,
            Node::Const {
                value: Value::Int(0),
            },
        );
        app.project.failure_rules.push(FailureRule {
            iteration: FailureIteration::Sequence {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 3,
                    item: NodeId::MAX - 1,
                },
            },
            selection: FailureSelection::All,
            message: None,
        });
        let retained = app.project.failure_rules[0].clone();
        app.apply_failure_rule_action(RuleAction::AddSequence(kind), true);
        assert_eq!(app.project.failure_rules[0], retained);
        assert!(
            !app.project.graph.nodes.contains_key(&(NodeId::MAX - 1)),
            "missing owned identity is reserved"
        );
        assert!(app.project.graph.nodes.contains_key(&(NodeId::MAX - 3)));
        assert!(app.project.graph.nodes.contains_key(&(NodeId::MAX - 2)));
        assert_eq!(item(&app, 1), NodeId::MAX);
    }
}

#[test]
fn generated_rule_locked_widgets_and_direct_actions_preserve_graph_history_and_canvases() {
    let mut app = fixture();
    open_named_canvas(&mut app);
    let context = context();
    click(&mut app, &context, "Add integer range rule");
    app.mark_clean();
    app.rebase_history();
    click(&mut app, &context, "Rule 1: generated sequence");
    app.begin_preview();
    assert!(app.preview_draft.is_some());
    let before = encoded(&app);
    let layout =
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace);
    for label in [
        "Add integer range rule",
        "Add split text rule",
        "Remove rule",
        "Custom start expression",
        "Custom message expression",
    ] {
        click(&mut app, &context, label);
        assert_eq!(encoded(&app), before, "locked {label}");
    }
    for action in [
        RuleAction::AddSequence(GeneratedRuleKind::IntegerRange),
        RuleAction::AddSequence(GeneratedRuleKind::SplitText),
        RuleAction::Remove(0),
    ] {
        app.apply_failure_rule_action(action, true);
        assert_eq!(encoded(&app), before);
    }
    assert_eq!(
        CanvasLayout::capture(&app.project, &app.main_canvas.snarl, &app.mapping_workspace),
        layout
    );
    assert!(app.preview_draft.is_some());
    assert!(!app.is_dirty());
    assert!(!app.can_undo());
    assert!(!app.history.can_redo());
}
