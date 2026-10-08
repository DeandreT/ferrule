use super::super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, DynamicBinding, FormatOptions, Graph, NamedTarget};

pub(super) const INPUT: &str =
    r#"{"KeyA":"display_name","ValueA":"Ada","KeyB":"city","ValueB":"Oslo"}"#;
pub(super) const FIXED: &str = "{\n  \"fixed\": \"kept\"\n}\n";
pub(super) const FULL: &str =
    "{\n  \"fixed\": \"kept\",\n  \"display_name\": \"Ada\",\n  \"city\": \"Oslo\"\n}\n";
pub(super) const CITY: &str = "{\n  \"fixed\": \"kept\",\n  \"city\": \"Oslo\"\n}\n";
pub(super) const CITY_ADA: &str =
    "{\n  \"fixed\": \"kept\",\n  \"display_name\": \"Ada\",\n  \"city\": \"Ada\"\n}\n";

pub(super) struct Retained {
    pub(super) path: PathBuf,
    sequence: std::cell::Cell<usize>,
    pub(super) complete: bool,
}
impl Retained {
    pub(super) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-computed-properties-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("input.json"), INPUT).unwrap();
        Self {
            path,
            sequence: std::cell::Cell::new(0),
            complete: false,
        }
    }
    pub(super) fn record(&self, label: &str, value: impl std::fmt::Debug) {
        let ordinal = self.sequence.get();
        self.sequence.set(ordinal + 1);
        std::fs::write(
            self.path.join(format!("{ordinal:04}-{label}.txt")),
            format!("{value:#?}\n"),
        )
        .unwrap();
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained computed-property originals: {}",
                self.path.display()
            );
        }
    }
}

fn project(retained: &Retained, populated: bool) -> Project {
    let names = ["KeyA", "ValueA", "KeyB", "ValueB"];
    let source = SchemaNode::group(
        "Source",
        names
            .iter()
            .map(|name| SchemaNode::scalar(*name, ScalarType::String))
            .collect(),
    )
    .with_required_fields(names.iter().map(|name| (*name).to_owned()).collect());
    retained.record("source-schema-construction", &source);
    let target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("fixed", ScalarType::String)],
    )
    .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
    .and_then(|target| target.with_required_fields(vec!["fixed".into()]));
    retained.record("target-schema-construction", &target);
    let mut project = Project {
        source: source.expect("ordinary source schema"),
        target: target.expect("scalar open target schema"),
        source_path: Some(retained.path.join("input.json").display().to_string()),
        target_path: Some(
            retained
                .path
                .join("must-not-publish.json")
                .display()
                .to_string(),
        ),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("kept".into()),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["KeyA".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["ValueA".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["KeyB".into()],
                        frame: None,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["ValueB".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
                (6, Node::Const { value: Value::Null }),
                (
                    7,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![8, 9],
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    9,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "fixed".into(),
                node: 0,
            }],
            dynamic_bindings: if populated {
                vec![
                    DynamicBinding { key: 1, value: 2 },
                    DynamicBinding { key: 3, value: 4 },
                ]
            } else {
                Vec::new()
            },
            ..Default::default()
        },
    };
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some(
            retained
                .path
                .join("other-must-not-publish.json")
                .display()
                .to_string(),
        ),
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    retained.record("initial-project", &project);
    project
}

pub(super) fn setup(
    retained: &Retained,
    populated: bool,
    document: MappingDocument,
) -> (FerruleApp, egui::Context) {
    let mut app = FerruleApp {
        project: project(retained, populated),
        ..Default::default()
    };
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.open_target_tab(0);
    assert!(app.ensure_target_canvas(0), "real named canvas initialized");
    app.mapping_workspace.active = document;
    app.selected_scope.clear();
    app.mark_clean();
    app.rebase_history();
    let context = egui::Context::default();
    crate::icons::install(&context);
    settle(&mut app, &context, retained);
    (app, context)
}

pub(super) fn raw_project(app: &FerruleApp, retained: &Retained, label: &str) -> String {
    retained.record(
        label,
        (
            &app.project,
            &app.status,
            app.is_dirty(),
            app.can_undo(),
            app.history.can_redo(),
        ),
    );
    let encoded = mapping::project_file::encode_pretty(&app.project);
    retained.record("project-codec-result", &encoded);
    encoded.expect("complete public project codec")
}

pub(super) fn canvas_state(app: &FerruleApp) -> String {
    let snapshot = |snarl: &egui_snarl::Snarl<CanvasNode>| {
        let mut nodes = snarl
            .node_ids()
            .map(|(id, node)| (*node, id.0))
            .collect::<Vec<_>>();
        nodes.sort_unstable();
        let mut wires = snarl
            .wires()
            .map(|(from, to)| {
                (
                    snarl[from.node],
                    from.node.0,
                    from.output,
                    snarl[to.node],
                    to.node.0,
                    to.input,
                )
            })
            .collect::<Vec<_>>();
        // Keep every endpoint and duplicate; rebuilds may change enumeration order.
        wires.sort_unstable();
        (nodes, wires, CanvasLayout::capture_nodes(snarl))
    };
    let main = snapshot(&app.main_canvas.snarl);
    let named = app
        .mapping_workspace
        .target_canvases
        .iter()
        .map(|(index, canvas)| (*index, snapshot(&canvas.snarl)))
        .collect::<Vec<_>>();
    format!("{main:#?}\n{named:#?}")
}

pub(super) fn active_scope(app: &FerruleApp) -> &Scope {
    let root = match app.mapping_workspace.active {
        MappingDocument::Target(index) => &app.project.extra_targets[index].root,
        _ => &app.project.root,
    };
    crate::auto_connect::scope_at(root, &app.selected_scope).expect("exact selected scope")
}
pub(super) fn expected(properties: &[(&str, Value)]) -> Instance {
    let mut fields = vec![(
        "fixed".into(),
        Instance::Scalar(Value::String("kept".into())),
    )];
    fields.extend(
        properties
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Instance::Scalar(value.clone()))),
    );
    Instance::Group(fields.into())
}
pub(super) fn full() -> Instance {
    expected(&[
        ("display_name", Value::String("Ada".into())),
        ("city", Value::String("Oslo".into())),
    ])
}
pub(super) fn input(app: &FerruleApp, retained: &Retained, text: &str) -> Instance {
    let decoded = format_json::from_str(text, &app.project.source);
    retained.record("full-input-parse-result", &decoded);
    decoded.expect("literal valid input")
}
pub(super) fn outputs(
    app: &FerruleApp,
    retained: &Retained,
    primary: &Instance,
    primary_text: &str,
    named: &Instance,
    named_text: &str,
) {
    let issues = cli::validate(&app.project);
    retained.record("complete-validation", &issues);
    let source = input(app, retained, INPUT);
    let actual = engine::run_outputs(&app.project, &source);
    retained.record("complete-engine-output-result", &actual);
    assert!(issues.is_empty(), "{issues:#?}");
    let actual = actual.expect("execute authored properties");
    let primary_json = format_json::to_string(&app.project.target, &actual.primary);
    retained.record("complete-primary-writer-result", &primary_json);
    let named_json = format_json::to_string(
        &app.project.extra_targets[0].schema,
        &actual.extras[0].instance,
    );
    retained.record("complete-named-writer-result", &named_json);
    assert_eq!(actual.primary, *primary);
    assert_eq!(
        actual.extras,
        vec![engine::NamedOutput {
            name: "Other".into(),
            instance: named.clone()
        }]
    );
    assert_eq!(primary_json.unwrap(), primary_text);
    assert_eq!(named_json.unwrap(), named_text);
}

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    retained.record("widget-input-events", &events);
    let enabled = app.ui_project_editing_enabled();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.handle_history_shortcuts(ui.ctx(), enabled);
            app.show_inspector(ui, enabled);
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    retained.record(
        "widget-shapes-events-and-scale",
        (
            &output.shapes,
            &output.platform_output.events,
            output.pixels_per_point,
        ),
    );
    output
}
pub(super) fn settle(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
) -> egui::FullOutput {
    let mut output = frame(app, context, retained, Vec::new());
    for _ in 0..3 {
        output = frame(app, context, retained, Vec::new());
    }
    output
}
pub(super) fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    fn visit(shape: &egui::epaint::Shape, clip: egui::Rect, out: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::epaint::Shape::Text(text) => {
                let bounds = text.visual_bounding_rect();
                if clip.contains_rect(bounds) {
                    out.push((text.galley.text().to_owned(), bounds.center()));
                }
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, &mut out);
    }
    out
}
pub(super) fn visible(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
    let mut out = texts(output)
        .into_iter()
        .filter_map(|(text, point)| (text == label).then_some(point))
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.y.total_cmp(&b.y));
    out
}
pub(super) fn click(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    point: egui::Pos2,
) {
    frame(
        app,
        context,
        retained,
        vec![egui::Event::PointerMoved(point)],
    );
    for pressed in [true, false] {
        frame(
            app,
            context,
            retained,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    settle(app, context, retained);
    raw_project(app, retained, "after-actual-click");
}
pub(super) fn click_label(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    label: &str,
) {
    let output = settle(app, context, retained);
    let points = visible(&output, label);
    retained.record("actual-control-label-points", (&label, &points));
    assert_eq!(points.len(), 1, "unique rendered control {label}");
    click(app, context, retained, points[0]);
}
pub(super) fn choose(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    row: usize,
    name: bool,
    wanted: NodeId,
    wanted_label: &str,
) {
    let output = settle(app, context, retained);
    let caption = if name {
        "Name expression"
    } else {
        "Value expression"
    };
    let captions = visible(&output, caption);
    retained.record("actual-row-captions", (&caption, &captions, row));
    let point = captions[row];
    let binding = &active_scope(app).dynamic_bindings[row];
    let current = if name { binding.key } else { binding.value };
    let prefix = format!("{current}: ");
    let selected = texts(&output)
        .into_iter()
        .filter_map(|(label, center)| {
            (label.starts_with(&prefix) && (center.y - point.y).abs() < 4.0 && center.x > point.x)
                .then_some(center)
        })
        .collect::<Vec<_>>();
    retained.record("actual-selected-expression-button", (&current, &selected));
    assert_eq!(selected.len(), 1, "real expression ComboBox");
    let closed = visible(&output, wanted_label);
    click(app, context, retained, selected[0]);
    let open = settle(app, context, retained);
    let options = visible(&open, wanted_label)
        .into_iter()
        .filter(|point| !closed.iter().any(|old| old.distance(*point) < 1.0))
        .collect::<Vec<_>>();
    retained.record(
        "actual-popup-only-options",
        (wanted, wanted_label, &closed, &options),
    );
    assert_eq!(options.len(), 1, "actual popup option {wanted_label}");
    click(app, context, retained, options[0]);
    let binding = &active_scope(app).dynamic_bindings[row];
    assert_eq!(if name { binding.key } else { binding.value }, wanted);
}
pub(super) fn add_two(app: &mut FerruleApp, context: &egui::Context, retained: &Retained) {
    click_label(app, context, retained, "+ property");
    choose(app, context, retained, 0, true, 1, "1: field KeyA");
    choose(app, context, retained, 0, false, 2, "2: field ValueA");
    click_label(app, context, retained, "+ property");
    choose(app, context, retained, 1, true, 3, "3: field KeyB");
    choose(app, context, retained, 1, false, 4, "4: field ValueB");
}
pub(super) fn keyboard_history(
    app: &mut FerruleApp,
    context: &egui::Context,
    retained: &Retained,
    redo: bool,
) {
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        shift: redo,
        ..Default::default()
    };
    frame(
        app,
        context,
        retained,
        vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
    );
    frame(
        app,
        context,
        retained,
        vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        }],
    );
    settle(app, context, retained);
    raw_project(app, retained, "actual-history-shortcut-result");
}

pub(super) fn failure(app: &FerruleApp, retained: &Retained, text: &str) -> engine::EngineError {
    raw_project(app, retained, "failure-original-project");
    let source = input(app, retained, text);
    let actual = engine::run(&app.project, &source);
    retained.record("complete-original-engine-error-result", &actual);
    let error = actual.expect_err("literal selected failure");
    retained.record("error-display", error.to_string());
    let mut cause = std::error::Error::source(&error);
    while let Some(actual) = cause {
        retained.record("complete-error-cause", format!("{actual:#?}\n{actual}"));
        cause = actual.source();
    }
    error
}
