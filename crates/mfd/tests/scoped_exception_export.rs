use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use engine::EngineError;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph, IterationOutput,
    NamedSource, Node, Pipeline, PipelineInput, PipelineStage, Project, Scope, ScopeIteration,
    SequenceExpr, SequenceWindow, UserFunction,
};
use mfd::{ExportProfile, MfdError};

struct Directory(PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_scoped_exception_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn child(&self, label: &str) -> PathBuf {
        let path = self.0.join(label);
        std::fs::create_dir(&path).unwrap();
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!("retained scoped-exception originals: {}", self.0.display());
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn retain(path: &Path, label: &str, value: &impl Debug) {
    let text = format!("{value:#?}\n");
    std::fs::write(path.join(format!("{label}.debug.txt")), &text).unwrap();
    eprintln!("{label}: {text}");
}
fn optional(mut node: SchemaNode) -> SchemaNode {
    assert!(node.set_xml_optional(true));
    node
}
fn field(path: &str) -> Node {
    Node::SourceField {
        path: vec![path.into()],
        frame: Some(vec!["Item".into()]),
    }
}
fn project(message: Option<u32>, raise_on_true: bool) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::Int),
                        SchemaNode::scalar("Blocked", ScalarType::Bool),
                        optional(SchemaNode::scalar("Message", ScalarType::String)),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Result", ScalarType::Int),
                        optional(SchemaNode::scalar("Rank", ScalarType::Int)),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (0, field("Value")),
                (2, field("Blocked")),
                (3, field("Message")),
                (4, Node::Raise { message }),
                (
                    5,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    6,
                    Node::If {
                        condition: 2,
                        then: if raise_on_true { 4 } else { 5 },
                        else_: if raise_on_true { 5 } else { 4 },
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                filter: Some(6),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn item(value: i64, blocked: Value, message: Value) -> Instance {
    group(vec![
        ("Value", Instance::Scalar(Value::Int(value))),
        ("Blocked", Instance::Scalar(blocked)),
        ("Message", Instance::Scalar(message)),
    ])
}
fn input(items: Vec<Instance>) -> Instance {
    group(vec![("Item", Instance::Repeated(items))])
}
fn rows(values: &[i64]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            values
                .iter()
                .map(|value| group(vec![("Result", Instance::Scalar(Value::Int(*value)))]))
                .collect(),
        ),
    )])
}
fn text(value: &str) -> Value {
    Value::String(value.into())
}
fn capture(
    path: &Path,
    label: &str,
    project: &Project,
    source: &Instance,
) -> Result<Instance, EngineError> {
    std::fs::write(
        path.join(format!("{label}-project.json")),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    std::fs::write(
        path.join(format!("{label}-input.json")),
        serde_json::to_vec_pretty(source).unwrap(),
    )
    .unwrap();
    let input_xml = format_xml::to_string(&project.source, source);
    retain(path, &format!("{label}-input-xml"), &input_xml);
    if let Ok(bytes) = input_xml {
        std::fs::write(path.join(format!("{label}-input.xml")), bytes).unwrap();
    }
    let result = engine::run(project, source);
    retain(path, label, &result);
    if let Ok(output) = &result {
        std::fs::write(
            path.join(format!("{label}-output.json")),
            serde_json::to_vec_pretty(output).unwrap(),
        )
        .unwrap();
        let xml = format_xml::to_string(&project.target, output);
        retain(path, &format!("{label}-output-xml"), &xml);
        if let Ok(xml) = xml {
            std::fs::write(path.join(format!("{label}-output.xml")), xml).unwrap();
        }
    }
    result
}
fn export(path: &Path, project: &Project, profile: ExportProfile) -> PathBuf {
    let validation = engine::validate(project);
    retain(path, "validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    std::fs::write(
        path.join("original-project.json"),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    let design = path.join("design.mfd");
    let result = mfd::export_with_profile(project, &design, profile);
    retain(path, "export-result", &result);
    let report = result.unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(path.join("design-source.xsd").is_file());
    assert!(path.join("design-target.xsd").is_file());
    design
}
fn component<'a>(doc: &'a roxmltree::Document<'a>, name: &str) -> roxmltree::Node<'a, 'a> {
    let nodes = doc
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1, "exactly one {name} component");
    nodes[0]
}
fn pin(node: roxmltree::Node<'_, '_>, side: &str, pos: usize) -> String {
    node.children()
        .find(|node| node.has_tag_name(side))
        .unwrap()
        .children()
        .filter(|node| node.has_tag_name("datapoint"))
        .enumerate()
        .find(|(index, node)| {
            node.attribute("pos")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(*index)
                == pos
        })
        .and_then(|(_, node)| node.attribute("key"))
        .unwrap()
        .into()
}
fn edge(doc: &roxmltree::Document<'_>, from: &str, to: &str) -> bool {
    doc.descendants().any(|node| {
        node.has_tag_name("vertex")
            && node.attribute("vertexkey") == Some(from)
            && node
                .descendants()
                .any(|node| node.has_tag_name("edge") && node.attribute("vertexkey") == Some(to))
    })
}
fn entry_pin(node: roxmltree::Node<'_, '_>, name: &str) -> String {
    node.descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some(name))
        .and_then(|node| {
            node.attribute("outkey")
                .or_else(|| node.attribute("inpkey"))
        })
        .unwrap()
        .into()
}
fn branch_wires(design: &Path, raise_on_true: bool, with_message: bool) {
    let xml = std::fs::read_to_string(design).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let source = component(&doc, "Input");
    let target = component(&doc, "Output");
    let filter = component(&doc, "filter");
    let exception = component(&doc, "exception");
    assert_eq!(exception.attribute("kind"), Some("18"));
    assert_eq!(filter.attribute("kind"), Some("3"));
    assert!(edge(
        &doc,
        &entry_pin(source, "Item"),
        &pin(filter, "sources", 0)
    ));
    assert!(
        edge(
            &doc,
            &entry_pin(source, "Blocked"),
            &pin(filter, "sources", 1)
        ),
        "native filter retains original predicate"
    );
    let selected = usize::from(!raise_on_true);
    let kept = usize::from(raise_on_true);
    assert!(edge(
        &doc,
        &pin(filter, "targets", selected),
        &pin(exception, "sources", 0)
    ));
    assert!(!edge(
        &doc,
        &pin(filter, "targets", kept),
        &pin(exception, "sources", 0)
    ));
    assert!(edge(
        &doc,
        &pin(filter, "targets", kept),
        &entry_pin(target, "Row")
    ));
    assert!(!edge(
        &doc,
        &pin(filter, "targets", selected),
        &entry_pin(target, "Row")
    ));
    if with_message {
        assert!(edge(
            &doc,
            &entry_pin(source, "Message"),
            &pin(exception, "sources", 1)
        ));
    } else {
        assert!(!doc.descendants().any(|node| node.has_tag_name("edge")
            && node.attribute("vertexkey") == Some(pin(exception, "sources", 1).as_str())));
    }
    assert!(edge(
        &doc,
        &entry_pin(source, "Value"),
        &entry_pin(target, "Result")
    ));
    assert!(!doc.descendants().any(|node| node.has_tag_name("component") && node.attribute("name") == Some("if-else")), "owned guard is not an ordinary component");
    assert_eq!(
        doc.descendants()
            .filter(|node| node.has_tag_name("component")
                && node.attribute("name") != Some("defaultmap"))
            .count(),
        4
    );
}

#[test]
fn both_polarities_keep_original_predicate_and_exact_native_branches() {
    let dir = Directory::new("branches");
    for polarity in [true, false] {
        let project = project(Some(3), polarity);
        let source = input(vec![
            item(10, Value::Bool(!polarity), text("unused-first")),
            item(20, Value::Bool(!polarity), text("unused-second")),
        ]);
        assert_eq!(
            capture(&dir.0, &format!("success-{polarity}"), &project, &source).unwrap(),
            rows(&[10, 20])
        );
        let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Result>10</Result>\n  </Row>\n  <Row>\n    <Result>20</Result>\n  </Row>\n</Output>";
        std::fs::write(dir.0.join(format!("hand-output-{polarity}.xml")), expected).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.0.join(format!("success-{polarity}-output.xml"))).unwrap(),
            expected
        );
        for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
            .into_iter()
            .enumerate()
        {
            branch_wires(
                &export(
                    &dir.child(&format!("{polarity}-{index}")),
                    &project,
                    profile,
                ),
                polarity,
                true,
            );
        }
        for (label, source, message) in [
            (
                "first",
                input(vec![
                    item(1, Value::Bool(polarity), text("first")),
                    item(2, Value::Bool(polarity), text("second")),
                ]),
                "first",
            ),
            (
                "late",
                input(vec![
                    item(1, Value::Bool(!polarity), text("unselected")),
                    item(2, Value::Bool(polarity), text("late")),
                    item(3, Value::Bool(polarity), text("third")),
                ]),
                "late",
            ),
        ] {
            assert!(
                matches!(capture(&dir.0, &format!("{label}-{polarity}"), &project, &source), Err(EngineError::MappingException { node: 4, message: Some(value) }) if value == message)
            );
        }
    }
}

#[test]
fn empty_absent_null_empty_and_throwing_messages_are_lazy_and_typed() {
    let dir = Directory::new("lazy");
    let empty = input(Vec::new());
    let project_with_message = project(Some(3), true);
    assert_eq!(
        capture(&dir.0, "empty", &project_with_message, &empty).unwrap(),
        rows(&[])
    );
    for (label, message, input_message, expected) in [
        ("absent", None, text("ignored"), None),
        ("null", Some(3), Value::Null, Some("")),
        ("empty-string", Some(3), text(""), Some("")),
    ] {
        let candidate = project(message, true);
        let source = input(vec![item(1, Value::Bool(true), input_message)]);
        let result = capture(&dir.0, label, &candidate, &source);
        assert!(
            matches!(result, Err(EngineError::MappingException { node: 4, message: actual }) if actual.as_deref() == expected)
        );
        let path = dir.child(label);
        let design = export(&path, &candidate, ExportProfile::NativeMfd);
        branch_wires(&design, true, message.is_some());
    }
    let mut throwing = project(Some(9), true);
    throwing.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(0),
        },
    );
    throwing.graph.nodes.insert(
        9,
        Node::Call {
            function: "divide".into(),
            args: vec![0, 8],
        },
    );
    let safe = input(vec![item(1, Value::Bool(false), text("ignored"))]);
    assert_eq!(
        capture(&dir.0, "unselected-throw", &throwing, &safe).unwrap(),
        rows(&[1])
    );
    let selected = input(vec![item(1, Value::Bool(true), text("ignored"))]);
    let error = capture(&dir.0, "selected-throw", &throwing, &selected).unwrap_err();
    assert!(matches!(error, EngineError::Function(_)));
    assert_eq!(error.to_string(), "division by zero");
    export(
        &dir.child("throw-design"),
        &throwing,
        ExportProfile::NativeMfd,
    );
    for (label, value, found) in [
        ("null-predicate", Value::Null, "null"),
        ("int-predicate", Value::Int(1), "int"),
    ] {
        let bad = input(vec![item(1, value, text("unread"))]);
        assert!(
            matches!(capture(&dir.0, label, &project_with_message, &bad), Err(EngineError::NotABool { node: 2, found: actual }) if actual == found)
        );
    }
}

#[test]
fn earlier_target_error_precedes_later_raise_but_global_rules_stay_pre_target() {
    let dir = Directory::new("order");
    let mut candidate = project(Some(3), true);
    candidate.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(0),
        },
    );
    candidate.graph.nodes.insert(
        9,
        Node::Call {
            function: "divide".into(),
            args: vec![0, 8],
        },
    );
    candidate.root.children[0].bindings[0].node = 9;
    let source = input(vec![
        item(1, Value::Bool(false), text("ignored")),
        item(2, Value::Bool(true), text("late-selected")),
    ]);
    let error = capture(&dir.0, "item-ordered", &candidate, &source).unwrap_err();
    assert!(matches!(error, EngineError::Function(_)));
    assert_eq!(error.to_string(), "division by zero");
    export(
        &dir.child("item-design"),
        &candidate,
        ExportProfile::NativeMfd,
    );
    let mut global = candidate.clone();
    global.graph.nodes.retain(|id, _| ![4, 5, 6].contains(id));
    global.graph.nodes.insert(
        7,
        Node::Call {
            function: "not".into(),
            args: vec![2],
        },
    );
    global.root.children[0].filter = Some(7);
    global.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Item".into()],
        },
        selection: FailureSelection::WhenTrue { predicate: 2 },
        message: Some(3),
    });
    assert!(
        matches!(capture(&dir.0, "global-pre-target", &global, &source), Err(EngineError::MappingFailure { rule: 1, message: Some(message) }) if message == "late-selected")
    );
    let first = input(vec![
        item(1, Value::Bool(true), text("first-selected")),
        item(2, Value::Bool(false), text("ignored")),
    ]);
    assert!(
        matches!(capture(&dir.0, "raise-first", &candidate, &first), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "first-selected")
    );
}

#[test]
fn raw_message_and_target_positions_have_distinct_native_contexts() {
    let dir = Directory::new("position");
    let mut candidate = project(Some(10), true);
    candidate.graph.nodes.insert(
        10,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    candidate.graph.nodes.insert(
        11,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    candidate.root.children[0].bindings.push(Binding {
        target_field: "Rank".into(),
        node: 11,
    });
    let source = input(vec![
        item(1, Value::Bool(false), text("ignored")),
        item(2, Value::Bool(true), text("ignored")),
    ]);
    assert!(
        matches!(capture(&dir.0, "late-position", &candidate, &source), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "2")
    );
    let path = dir.child("separate");
    let design = export(&path, &candidate, ExportProfile::NativeMfd);
    let xml = std::fs::read_to_string(&design).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let autos = doc
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some("position"))
        .collect::<Vec<_>>();
    assert_eq!(autos.len(), 2);
    let filter = component(&doc, "filter");
    let exception = component(&doc, "exception");
    let message_auto = autos
        .iter()
        .find(|auto| {
            edge(
                &doc,
                &pin(**auto, "targets", 0),
                &pin(exception, "sources", 1),
            )
        })
        .unwrap();
    let target_auto = autos
        .iter()
        .find(|auto| {
            edge(
                &doc,
                &pin(**auto, "targets", 0),
                &entry_pin(component(&doc, "Output"), "Rank"),
            )
        })
        .unwrap();
    assert_ne!(message_auto.id(), target_auto.id());
    assert!(edge(
        &doc,
        &entry_pin(component(&doc, "Input"), "Item"),
        &pin(*message_auto, "sources", 0)
    ));
    let variable = component(&doc, "scope-sequence");
    let item = variable
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Item"))
        .unwrap();
    let variable_input = item.attribute("inpkey").unwrap();
    let variable_output = item.attribute("outkey").unwrap();
    assert!(edge(&doc, &pin(filter, "targets", 1), variable_input));
    assert!(edge(
        &doc,
        variable_output,
        &pin(*target_auto, "sources", 0)
    ));
    assert!(edge(
        &doc,
        variable_output,
        &entry_pin(component(&doc, "Output"), "Row")
    ));
    assert!(!edge(
        &doc,
        variable_output,
        &pin(*message_auto, "sources", 0)
    ));
    let mut ambiguous = candidate.clone();
    ambiguous.root.children[0].bindings.last_mut().unwrap().node = 10;
    ambiguous.graph.nodes.remove(&11);
    refusal(&dir.child("shared-position"), &ambiguous);
}

fn refusal(path: &Path, project: &Project) {
    std::fs::write(
        path.join("original-project.json"),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    let design = path.join("existing.mfd");
    let source_schema = path.join("existing-source.xsd");
    let target_schema = path.join("existing-target.xsd");
    for (path, bytes) in [
        (&design, b"original-design".as_slice()),
        (&source_schema, b"original-source-schema".as_slice()),
        (&target_schema, b"original-target-schema".as_slice()),
    ] {
        std::fs::write(path, bytes).unwrap();
    }
    let before = [&design, &source_schema, &target_schema].map(|path| std::fs::read(path).unwrap());
    for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
        .into_iter()
        .enumerate()
    {
        let result = mfd::export_with_profile(project, &design, profile);
        retain(path, &format!("refusal-{index}"), &result);
        assert!(
            matches!(result, Err(MfdError::Unsupported(_))),
            "{result:?}"
        );
        assert_eq!(
            [&design, &source_schema, &target_schema].map(|path| std::fs::read(path).unwrap()),
            before
        );
        let fresh = path.join(format!("fresh-{index}/design.mfd"));
        let result = mfd::export_with_profile(project, &fresh, profile);
        retain(path, &format!("fresh-refusal-{index}"), &result);
        assert!(
            matches!(result, Err(MfdError::Unsupported(_))),
            "{result:?}"
        );
        assert!(!fresh.parent().unwrap().exists());
    }
}

#[test]
fn helper_sharing_unowned_raise_and_boundary_owner_controls_refuse_atomically() {
    let dir = Directory::new("refusals");
    let baseline = project(Some(3), true);
    let mut cases = Vec::new();
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        20,
        Node::Call {
            function: "not".into(),
            args: vec![4],
        },
    );
    cases.push(("shared-raise", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        20,
        Node::Call {
            function: "not".into(),
            args: vec![5],
        },
    );
    cases.push(("shared-true", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        20,
        Node::Call {
            function: "not".into(),
            args: vec![6],
        },
    );
    cases.push(("shared-if", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].bindings[0].node = 4;
    cases.push(("binding-raise", candidate));
    let mut candidate = baseline.clone();
    candidate
        .graph
        .nodes
        .insert(20, Node::Raise { message: None });
    cases.push(("second-raise", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].filter = None;
    cases.push(("unowned-raise", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    cases.push(("false-opposite", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].iteration_output = IterationOutput::First;
    cases.push(("first", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        20,
        Node::Const {
            value: Value::Int(1),
        },
    );
    candidate.root.children[0]
        .windows
        .push(SequenceWindow::First { count: 20 });
    cases.push(("window", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].sort_by = Some(0);
    cases.push(("sort", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].group_by = Some(0);
    cases.push(("group", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].set_output_path(Some(3));
    cases.push(("dynamic-document", candidate));
    let mut candidate = baseline.clone();
    candidate.root.filter = Some(5);
    cases.push(("ancestor-filter", candidate));
    let mut candidate = baseline.clone();
    candidate.target_path = Some("output.csv".into());
    cases.push(("csv", candidate));
    let mut candidate = baseline.clone();
    candidate.source_path = Some("https://example.invalid/input.xml".into());
    cases.push(("remote", candidate));
    let mut candidate = baseline.clone();
    candidate.source_options.local_xml_file_set = true;
    cases.push(("file-set", candidate));
    let mut candidate = baseline.clone();
    candidate.extra_sources.push(NamedSource {
        name: "Other".into(),
        path: "other.xml".into(),
        schema: candidate.source.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    cases.push(("extra-source", candidate));
    let mut candidate = baseline.clone();
    candidate.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Item".into()],
        },
        selection: FailureSelection::WhenTrue { predicate: 2 },
        message: Some(3),
    });
    cases.push(("global-rule", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["Blocked".into()],
            frame: None,
        },
    );
    cases.push(("unframed-relative-predicate", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        3,
        Node::Position {
            collection: Vec::new(),
        },
    );
    cases.push(("unidentified-position", candidate));
    let mut candidate = baseline.clone();
    candidate.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "Item",
            vec![
                SchemaNode::scalar("Value", ScalarType::Int),
                SchemaNode::scalar("Blocked", ScalarType::Bool),
                SchemaNode::scalar("Message", ScalarType::String),
            ],
        )],
    );
    cases.push(("nonrepeating-item", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].iteration = ScopeIteration::Source(vec!["Missing".into()]);
    cases.push(("wrong-primary-path", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 0,
        item: 20,
    });
    candidate.graph.nodes.insert(
        20,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    cases.push(("generated-owner", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        6,
        Node::If {
            condition: 5,
            then: 4,
            else_: 5,
        },
    );
    cases.push(("predicate-is-owned-true", candidate));
    let mut candidate = baseline.clone();
    candidate.root.children.push(Scope {
        target_field: "Row".into(),
        ..Scope::default()
    });
    cases.push(("duplicate-target-path", candidate));
    let mut candidate = baseline.clone();
    candidate.root.bindings.push(Binding {
        target_field: "RootValue".into(),
        node: 0,
    });
    cases.push(("outside-owner-binding", candidate));
    let mut candidate = baseline.clone();
    let row = candidate.target.child("Row").unwrap().clone();
    let mut other_row = row.clone();
    other_row.name = "OtherRow".into();
    candidate.target = SchemaNode::group("Output", vec![row, other_row]);
    let mut other_owner = candidate.root.children[0].clone();
    other_owner.target_field = "OtherRow".into();
    candidate.root.children.push(other_owner);
    cases.push(("two-source-owners", candidate));
    let mut candidate = baseline.clone();
    candidate.graph.nodes.insert(
        20,
        Node::Aggregate {
            function: mapping::AggregateOp::Count,
            collection: vec!["Item".into()],
            value: Vec::new(),
            expression: None,
            arg: None,
        },
    );
    candidate.root.children[0].bindings[0].node = 20;
    cases.push(("aggregate-target-prepass", candidate));
    let mut candidate = baseline.clone();
    candidate.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Item",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Blocked", ScalarType::Bool),
                    optional(SchemaNode::scalar("Message", ScalarType::String)),
                    SchemaNode::group("Nested", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    candidate.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Nested".into(), "Value".into()],
            frame: Some(vec!["Item".into()]),
        },
    );
    cases.push(("nested-repeated-binding", candidate));
    for (label, candidate) in cases {
        refusal(&dir.child(label), &candidate);
    }
}

#[test]
fn ordinary_consumers_survive_and_no_raise_profiles_emit_equal_bytes() {
    let dir = Directory::new("ordinary");
    let mut candidate = project(Some(3), true);
    candidate.graph.nodes.insert(
        8,
        Node::Call {
            function: "not".into(),
            args: vec![2],
        },
    );
    candidate.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Result", ScalarType::Int),
                    SchemaNode::scalar("Allowed", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    candidate.root.children[0].bindings.push(Binding {
        target_field: "Allowed".into(),
        node: 8,
    });
    let design = export(&dir.child("with-not"), &candidate, ExportProfile::NativeMfd);
    let xml = std::fs::read_to_string(design).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let not = component(&doc, "logical-not");
    let filter = component(&doc, "filter");
    assert!(edge(
        &doc,
        &entry_pin(component(&doc, "Input"), "Blocked"),
        &pin(filter, "sources", 1)
    ));
    assert!(!edge(
        &doc,
        &pin(not, "targets", 0),
        &pin(filter, "sources", 1)
    ));
    assert!(edge(
        &doc,
        &pin(not, "targets", 0),
        &entry_pin(component(&doc, "Output"), "Allowed")
    ));
    let mut ordinary = project(Some(3), true);
    ordinary.graph.nodes.retain(|id, _| ![4, 5, 6].contains(id));
    ordinary.root.children[0].filter = Some(2);
    let path = dir.child("no-raise");
    let design = path.join("legacy.mfd");
    let default = mfd::export_with_profile(&ordinary, &design, ExportProfile::FerruleExtensions);
    retain(&path, "legacy-default", &default);
    assert!(default.unwrap().warnings.is_empty());
    let before = std::fs::read(&design).unwrap();
    let strict = mfd::export_with_profile(&ordinary, &design, ExportProfile::NativeMfd);
    retain(&path, "legacy-native", &strict);
    assert!(strict.unwrap().is_native_compatible());
    assert_eq!(std::fs::read(&design).unwrap(), before);
    assert!(ordinary.failure_rules.is_empty());
}

#[test]
fn connected_pipeline_raise_and_user_function_raise_refuse_before_publication() {
    let dir = Directory::new("pipeline");
    let mut first = project(None, true);
    first.graph.nodes.retain(|id, _| ![4, 5, 6].contains(id));
    first.target = first.source.clone();
    first.root.children[0].target_field = "Item".into();
    first.root.children[0].filter = None;
    first.root.children[0].bindings = vec![
        Binding {
            target_field: "Value".into(),
            node: 0,
        },
        Binding {
            target_field: "Blocked".into(),
            node: 2,
        },
        Binding {
            target_field: "Message".into(),
            node: 3,
        },
    ];
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![
            PipelineStage {
                id: "first".into(),
                mapping_path: None,
                project: first.clone(),
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "second".into(),
                mapping_path: None,
                project: project(Some(3), true),
                source: PipelineInput::StageTarget {
                    stage: "first".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    let validation = engine::validate_pipeline(&pipeline);
    retain(&dir.0, "pipeline-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    let mut function_pipeline = pipeline.clone();
    function_pipeline.stages[1].project = first;
    function_pipeline.stages[1].project.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "local".into(),
            name: "fail".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: BTreeMap::from([(20, Node::Raise { message: None })]),
            },
            output: 20,
        },
    );
    for (index, pipeline) in [pipeline, function_pipeline].into_iter().enumerate() {
        std::fs::write(
            dir.0.join(format!("pipeline-{index}.json")),
            serde_json::to_vec_pretty(&pipeline).unwrap(),
        )
        .unwrap();
        for (profile_index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
            .into_iter()
            .enumerate()
        {
            let parent = dir.0.join(format!("unpublished-{index}-{profile_index}"));
            let design = parent.join("design.mfd");
            let result = mfd::export_pipeline_with_profile(&pipeline, &design, profile);
            retain(
                &dir.0,
                &format!("pipeline-{index}-{profile_index}"),
                &result,
            );
            assert!(
                matches!(result, Err(MfdError::Unsupported(message)) if message.contains("contains Raise") && message.contains("native item ordering across stages"))
            );
            assert!(!parent.exists());
        }
    }
}

#[test]
fn oversized_deep_and_high_fanout_proofs_refuse_before_publication() {
    let dir = Directory::new("proof-budgets");
    let mut oversized = project(Some(3), true);
    for id in 10_000..14_097 {
        oversized.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::Bool(true),
            },
        );
    }
    let mut deep = project(Some(3), true);
    let mut previous = 2;
    // Ascending IDs warm the height memo before each next parent is visited.
    for id in 100..280 {
        deep.graph.nodes.insert(
            id,
            Node::Call {
                function: "not".into(),
                args: vec![previous],
            },
        );
        previous = id;
    }
    let mut fanout = project(Some(3), true);
    for id in 100..2100 {
        fanout.graph.nodes.insert(
            id,
            Node::Call {
                function: "concat".into(),
                args: vec![3; 64],
            },
        );
    }
    let mut schema_depth = project(Some(3), true);
    let mut nested = SchemaNode::scalar("Leaf", ScalarType::String);
    for _ in 0..130 {
        nested = SchemaNode::group("Layer", vec![nested]);
    }
    nested.name = "Deep".into();
    let ir::SchemaKind::Group { children, .. } = &mut schema_depth.source.kind else {
        panic!()
    };
    children.push(nested);
    for (label, candidate) in [
        ("graph-count", oversized),
        ("memoized-height", deep),
        ("dependency-work", fanout),
        ("schema-depth", schema_depth),
    ] {
        let path = dir.child(label);
        let result = mfd::preflight_export(&candidate, &path.join("preflight.mfd"));
        retain(&path, "bounded-preflight", &result);
        assert!(
            matches!(result, Err(MfdError::Unsupported(message)) if message.contains("bounded"))
        );
        assert!(!path.join("preflight.mfd").exists());
        refusal(&path, &candidate);
    }
}

#[test]
fn closed_required_metadata_capacity_and_restricted_names_refuse_before_validation() {
    let dir = Directory::new("required-metadata");
    let mut valid = project(Some(3), true);
    valid.source.property_count_range = Some(ir::PropertyCountRange::new(1, Some(1)).unwrap());
    let ir::SchemaKind::Group { required, .. } = &mut valid.source.kind else {
        panic!()
    };
    *required = vec!["Item".into()];
    let valid_path = dir.child("valid-closed-capacity");
    std::fs::write(
        valid_path.join("original-project.json"),
        serde_json::to_vec_pretty(&valid).unwrap(),
    )
    .unwrap();
    let validation = engine::validate(&valid);
    retain(&valid_path, "valid-metadata-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    let result = mfd::preflight_export(&valid, &valid_path.join("preflight.mfd"));
    retain(&valid_path, "valid-metadata-preflight", &result);
    let report = result.unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(!valid_path.join("preflight.mfd").exists());

    let mut duplicate_required = valid.clone();
    let ir::SchemaKind::Group { required, .. } = &mut duplicate_required.source.kind else {
        panic!()
    };
    // Two entries exceed the one-child closed capacity even though property
    // counting would first reduce their identical names to one required key.
    *required = vec!["Item".into(), "Item".into()];
    let mut restricted = valid.clone();
    let ir::SchemaKind::Group {
        xml_restricted_alternatives,
        ..
    } = &mut restricted.target.kind
    else {
        panic!()
    };
    *xml_restricted_alternatives = vec!["MissingType".into()];
    for (label, candidate) in [
        ("required-capacity", duplicate_required),
        ("restricted-without-alternatives", restricted),
    ] {
        let path = dir.child(label);
        std::fs::write(
            path.join("original-project.json"),
            serde_json::to_vec_pretty(&candidate).unwrap(),
        )
        .unwrap();
        let result = mfd::preflight_export(&candidate, &path.join("preflight.mfd"));
        retain(&path, "metadata-preflight", &result);
        assert!(
            matches!(result, Err(MfdError::Unsupported(message)) if message.contains("bounded closed scalar proof shape"))
        );
        assert!(!path.join("preflight.mfd").exists());
        refusal(&path, &candidate);
    }
}

fn set_predicate(project: &mut Project, predicate: u32) {
    let Some(Node::If { condition, .. }) = project.graph.nodes.get_mut(&6) else {
        panic!("guard must remain If");
    };
    *condition = predicate;
}

fn source_item_field_mut<'a>(project: &'a mut Project, name: &str) -> &'a mut SchemaNode {
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        panic!("source root group");
    };
    let item = children
        .iter_mut()
        .find(|node| node.name == "Item")
        .unwrap();
    let ir::SchemaKind::Group { children, .. } = &mut item.kind else {
        panic!("source item group");
    };
    children.iter_mut().find(|node| node.name == name).unwrap()
}

fn predicate_design(path: &Path, candidate: &Project, native_name: &str, field: Option<&str>) {
    predicate_design_with_target_route(path, candidate, native_name, field, false);
}

fn predicate_design_with_target_route(
    path: &Path,
    candidate: &Project,
    native_name: &str,
    field: Option<&str>,
    target_via_scope_variable: bool,
) {
    for (index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
        .into_iter()
        .enumerate()
    {
        let directory = path.join(index.to_string());
        std::fs::create_dir(&directory).unwrap();
        let design = export(&directory, candidate, profile);
        let xml = std::fs::read_to_string(design).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let predicate = component(&doc, native_name);
        let output = field.map_or_else(
            || pin(predicate, "targets", 0),
            |name| entry_pin(predicate, name),
        );
        let filter = component(&doc, "filter");
        let exception = component(&doc, "exception");
        assert!(
            edge(&doc, &output, &pin(filter, "sources", 1)),
            "original proven predicate must feed the native filter"
        );
        assert!(edge(
            &doc,
            &pin(filter, "targets", 0),
            &pin(exception, "sources", 0)
        ));
        let target = entry_pin(component(&doc, "Output"), "Row");
        if target_via_scope_variable {
            let variable = component(&doc, "scope-sequence");
            let item = variable
                .descendants()
                .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Item"))
                .unwrap();
            let input = item.attribute("inpkey").unwrap();
            let output = item.attribute("outkey").unwrap();
            assert!(edge(&doc, &pin(filter, "targets", 1), input));
            assert!(edge(&doc, output, &target));
            assert!(!edge(&doc, &pin(filter, "targets", 1), &target));
            let position = component(&doc, "position");
            assert!(edge(
                &doc,
                &entry_pin(component(&doc, "Input"), "Item"),
                &pin(position, "sources", 0)
            ));
            assert!(edge(
                &doc,
                &pin(position, "targets", 0),
                &pin(predicate, "sources", 0)
            ));
            assert!(!edge(&doc, output, &pin(position, "sources", 0)));
        } else {
            assert!(edge(&doc, &pin(filter, "targets", 1), &target));
        }
    }
}

#[test]
fn required_boolean_constants_operators_and_lazy_boolean_if_are_admitted() {
    let dir = Directory::new("predicate-bools");
    let mut cases = Vec::new();
    cases.push((
        "required-field",
        project(Some(3), true),
        "Input",
        Some("Blocked"),
        false,
        true,
    ));
    let mut constant = project(Some(3), true);
    constant.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    let safe = input(vec![item(10, Value::Bool(true), text("ignored"))]);
    assert_eq!(
        capture(&dir.0, "constant-false", &constant, &safe).unwrap(),
        rows(&[10])
    );
    predicate_design(
        &dir.child("constant-false-design"),
        &constant,
        "constant",
        None,
    );
    constant.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    assert!(
        matches!(capture(&dir.0, "constant-true", &constant, &safe), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "ignored")
    );
    predicate_design(
        &dir.child("constant-true-design"),
        &constant,
        "constant",
        None,
    );
    for (label, function, native_name, constant, safe, selected) in [
        ("not", "not", "logical-not", None, true, false),
        ("and", "and", "logical-and", Some(true), false, true),
        ("or", "or", "logical-or", Some(false), false, true),
    ] {
        let mut candidate = project(Some(3), true);
        let mut args = vec![2];
        if let Some(value) = constant {
            candidate.graph.nodes.insert(
                21,
                Node::Const {
                    value: Value::Bool(value),
                },
            );
            args.push(21);
        }
        candidate.graph.nodes.insert(
            20,
            Node::Call {
                function: function.into(),
                args,
            },
        );
        set_predicate(&mut candidate, 20);
        cases.push((label, candidate, native_name, None, safe, selected));
    }
    let mut conditional = project(Some(3), true);
    conditional.graph.nodes.insert(
        20,
        Node::If {
            condition: 2,
            then: 21,
            else_: 22,
        },
    );
    conditional.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    conditional.graph.nodes.insert(
        22,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    set_predicate(&mut conditional, 20);
    cases.push(("boolean-if", conditional, "if-else", None, false, true));
    for (label, candidate, native_name, field, safe, selected) in cases {
        let source = input(vec![
            item(10, Value::Bool(safe), text("unread")),
            item(20, Value::Bool(safe), text("also-unread")),
        ]);
        assert_eq!(
            capture(&dir.0, &format!("{label}-safe"), &candidate, &source).unwrap(),
            rows(&[10, 20])
        );
        let source = input(vec![
            item(10, Value::Bool(safe), text("unread")),
            item(20, Value::Bool(selected), text("selected")),
        ]);
        assert!(
            matches!(capture(&dir.0, &format!("{label}-late"), &candidate, &source), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "selected")
        );
        predicate_design(&dir.child(label), &candidate, native_name, field);
    }
    // Shared logical operands stay one proven DAG, rather than being expanded.
    let mut shared = project(Some(3), true);
    shared.graph.nodes.insert(
        20,
        Node::Call {
            function: "and".into(),
            args: vec![2, 2],
        },
    );
    shared.graph.nodes.insert(
        21,
        Node::Call {
            function: "or".into(),
            args: vec![20, 20],
        },
    );
    set_predicate(&mut shared, 21);
    let safe = input(vec![item(1, Value::Bool(false), text("ignored"))]);
    assert_eq!(
        capture(&dir.0, "shared-dag-safe", &shared, &safe).unwrap(),
        rows(&[1])
    );
    predicate_design(&dir.child("shared-dag"), &shared, "logical-or", None);
}

#[test]
fn direct_same_type_integer_string_boolean_and_owner_position_comparisons_are_admitted() {
    let dir = Directory::new("predicate-comparisons");
    for (function, native_name, safe, selected) in [
        ("equal", "equal", 9, 10),
        ("not_equal", "not-equal", 10, 9),
        ("less_than", "less", 10, 9),
        ("greater_than", "greater", 10, 11),
        ("less_or_equal", "less-equal", 11, 10),
        ("greater_or_equal", "greater-equal", 9, 10),
    ] {
        let mut candidate = project(Some(3), true);
        candidate.graph.nodes.insert(
            20,
            Node::Call {
                function: function.into(),
                args: vec![0, 21],
            },
        );
        candidate.graph.nodes.insert(
            21,
            Node::Const {
                value: Value::Int(10),
            },
        );
        set_predicate(&mut candidate, 20);
        let source = input(vec![item(safe, Value::Bool(false), text("unread"))]);
        assert_eq!(
            capture(&dir.0, &format!("{function}-safe"), &candidate, &source).unwrap(),
            rows(&[safe])
        );
        let source = input(vec![item(selected, Value::Bool(false), text("selected"))]);
        assert!(
            matches!(capture(&dir.0, &format!("{function}-selected"), &candidate, &source), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "selected")
        );
        predicate_design(&dir.child(function), &candidate, native_name, None);
    }
    let mut string = project(Some(3), true);
    assert!(source_item_field_mut(&mut string, "Message").set_xml_optional(false));
    string.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![3, 21],
        },
    );
    string
        .graph
        .nodes
        .insert(21, Node::Const { value: text("b") });
    set_predicate(&mut string, 20);
    let source = input(vec![item(1, Value::Bool(false), text("a"))]);
    assert_eq!(
        capture(&dir.0, "string-safe", &string, &source).unwrap(),
        rows(&[1])
    );
    let source = input(vec![item(2, Value::Bool(false), text("b"))]);
    assert!(
        matches!(capture(&dir.0, "string-selected", &string, &source), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "b")
    );
    predicate_design(&dir.child("string"), &string, "equal", None);
    let mut boolean = project(Some(3), true);
    boolean.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![2, 21],
        },
    );
    boolean.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    set_predicate(&mut boolean, 20);
    let source = input(vec![
        item(1, Value::Bool(false), text("unread")),
        item(2, Value::Bool(true), text("boolean-selected")),
    ]);
    assert!(
        matches!(capture(&dir.0, "boolean-selected", &boolean, &source), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "boolean-selected")
    );
    predicate_design(&dir.child("boolean"), &boolean, "equal", None);
    let mut position = project(Some(3), true);
    position.graph.nodes.insert(
        10,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    position.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![10, 21],
        },
    );
    position.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Int(2),
        },
    );
    set_predicate(&mut position, 20);
    let source = input(vec![
        item(1, Value::Bool(false), text("unread")),
        item(2, Value::Bool(false), text("position-two")),
    ]);
    assert!(
        matches!(capture(&dir.0, "position-selected", &position, &source), Err(EngineError::MappingException { node: 4, message: Some(message) }) if message == "position-two")
    );
    predicate_design_with_target_route(&dir.child("position"), &position, "equal", None, true);
}

fn predicate_refusal(path: &Path, candidate: &Project) {
    let validation = engine::validate(candidate);
    retain(path, "predicate-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    let result = mfd::preflight_export(candidate, &path.join("preflight.mfd"));
    retain(path, "predicate-preflight", &result);
    assert!(
        matches!(result, Err(MfdError::Unsupported(message)) if message.contains("predicate is not proven"))
    );
    assert!(!path.join("preflight.mfd").exists());
    refusal(path, candidate);
}

#[test]
fn nonboolean_computed_nullable_and_optional_predicates_refuse_before_publication() {
    let dir = Directory::new("predicate-refusals");
    let source = input(vec![item(1, Value::Bool(false), text("unread"))]);
    let mut cases = Vec::new();
    for (label, value, found) in [
        ("constant-int", Value::Int(1), "int"),
        ("constant-null", Value::Null, "null"),
        ("constant-float", Value::Float(1.0), "float"),
    ] {
        let mut candidate = project(Some(3), true);
        candidate.graph.nodes.insert(2, Node::Const { value });
        assert!(
            matches!(capture(&dir.0, label, &candidate, &source), Err(EngineError::NotABool { node: 2, found: actual }) if actual == found)
        );
        cases.push((label, candidate));
    }
    let mut integer = project(Some(3), true);
    *source_item_field_mut(&mut integer, "Blocked") =
        SchemaNode::scalar("Blocked", ScalarType::Int);
    let integer_input = input(vec![item(1, Value::Int(1), text("unread"))]);
    let input_xml = format_xml::to_string(&integer.source, &integer_input);
    retain(&dir.0, "declared-int-input-xml", &input_xml);
    assert!(input_xml.is_ok());
    assert!(matches!(
        capture(&dir.0, "declared-int", &integer, &integer_input),
        Err(EngineError::NotABool {
            node: 2,
            found: "int"
        })
    ));
    cases.push(("declared-int", integer));
    let mut wrong_call = project(Some(3), true);
    wrong_call.graph.nodes.insert(
        20,
        Node::Call {
            function: "not".into(),
            args: vec![0],
        },
    );
    set_predicate(&mut wrong_call, 20);
    let error = capture(&dir.0, "wrong-not-domain", &wrong_call, &source).unwrap_err();
    assert!(matches!(error, EngineError::Function(_)));
    assert_eq!(error.to_string(), "`not` cannot accept a int argument");
    cases.push(("wrong-not-domain", wrong_call));
    let mut string_call = project(Some(3), true);
    string_call.graph.nodes.insert(
        20,
        Node::Call {
            function: "string".into(),
            args: vec![0],
        },
    );
    set_predicate(&mut string_call, 20);
    assert!(matches!(
        capture(&dir.0, "string-call", &string_call, &source),
        Err(EngineError::NotABool {
            node: 20,
            found: "string"
        })
    ));
    cases.push(("string-call", string_call));
    let mut conditional = project(Some(3), true);
    conditional.graph.nodes.insert(
        20,
        Node::If {
            condition: 2,
            then: 21,
            else_: 22,
        },
    );
    conditional.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    conditional.graph.nodes.insert(
        22,
        Node::Const {
            value: Value::Int(1),
        },
    );
    set_predicate(&mut conditional, 20);
    assert!(matches!(
        capture(&dir.0, "mixed-if", &conditional, &source),
        Err(EngineError::NotABool {
            node: 20,
            found: "int"
        })
    ));
    cases.push(("mixed-if", conditional));
    let mut mixed = project(Some(3), true);
    mixed.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![0, 21],
        },
    );
    mixed
        .graph
        .nodes
        .insert(21, Node::Const { value: text("1") });
    set_predicate(&mut mixed, 20);
    cases.push(("mixed-comparison", mixed));
    let mut computed = project(Some(3), true);
    computed.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![22, 21],
        },
    );
    computed.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Int(1),
        },
    );
    computed.graph.nodes.insert(
        22,
        Node::Call {
            function: "add".into(),
            args: vec![0, 21],
        },
    );
    set_predicate(&mut computed, 20);
    cases.push(("computed-comparison", computed));
    let mut floating = project(Some(3), true);
    floating.graph.nodes.insert(
        20,
        Node::Call {
            function: "equal".into(),
            args: vec![21, 22],
        },
    );
    floating.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Float(1.0),
        },
    );
    floating.graph.nodes.insert(
        22,
        Node::Const {
            value: Value::Float(1.0),
        },
    );
    set_predicate(&mut floating, 20);
    cases.push(("float-comparison", floating));
    let mut optional_field = project(Some(3), true);
    assert!(source_item_field_mut(&mut optional_field, "Blocked").set_xml_optional(true));
    let absent = input(vec![item(1, Value::Null, text("unread"))]);
    assert!(matches!(
        capture(&dir.0, "optional-absent", &optional_field, &absent),
        Err(EngineError::NotABool {
            node: 2,
            found: "null"
        })
    ));
    cases.push(("optional-absent", optional_field));
    let mut nil = project(Some(3), true);
    source_item_field_mut(&mut nil, "Blocked").nillable = true;
    let source_nil = input(vec![item(1, Value::xml_nil(), text("unread"))]);
    assert!(matches!(
        capture(&dir.0, "nillable", &nil, &source_nil),
        Err(EngineError::NotABool {
            node: 2,
            found: "xml nil"
        })
    ));
    cases.push(("nillable", nil));
    let mut attribute = project(Some(3), true);
    source_item_field_mut(&mut attribute, "Blocked").attribute = true;
    assert!(matches!(
        capture(&dir.0, "optional-attribute", &attribute, &absent),
        Err(EngineError::NotABool {
            node: 2,
            found: "null"
        })
    ));
    cases.push(("optional-attribute", attribute));
    let mut ancestor = project(Some(3), true);
    let ir::SchemaKind::Group { children, .. } = &mut ancestor.source.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children.push(optional(SchemaNode::group(
        "Details",
        vec![SchemaNode::scalar("Flag", ScalarType::Bool)],
    )));
    ancestor.graph.nodes.insert(
        2,
        Node::SourceField {
            path: vec!["Details".into(), "Flag".into()],
            frame: Some(vec!["Item".into()]),
        },
    );
    assert!(matches!(
        capture(&dir.0, "optional-ancestor", &ancestor, &source),
        Err(EngineError::NotABool {
            node: 2,
            found: "null"
        })
    ));
    cases.push(("optional-ancestor", ancestor));
    for (label, candidate) in cases {
        predicate_refusal(&dir.child(label), &candidate);
    }
}
