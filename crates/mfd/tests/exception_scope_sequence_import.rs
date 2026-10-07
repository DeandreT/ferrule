use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use engine::EngineError;
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};
use mfd::{ExportProfile, ImportIssueKind, ImportOptions, ImportProfile, MfdError};

struct Directory(PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_scope_sequence_{label}_{}_{}",
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
            eprintln!(
                "retained scope-sequence import originals: {}",
                self.0.display()
            );
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn retain(path: &Path, label: &str, value: &impl Debug) {
    std::fs::write(
        path.join(format!("{label}.debug.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}
fn imported_original(imported: &mfd::Imported) -> String {
    format!(
        "mapping_path: {:#?}\nwarnings: {:#?}\nproject: {:#?}\n",
        imported.mapping_path, imported.warnings, imported.project
    )
}
fn import_error_original(error: &MfdError) -> String {
    let mut original = format!("Err: {error:#?}\nDisplay: {error}\n");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        original.push_str(&format!("Caused by: {cause:#?}\nDisplay: {cause}\n"));
        source = cause.source();
    }
    original
}
fn retain_imported(path: &Path, label: &str, result: &Result<mfd::Imported, MfdError>) {
    let original = match result {
        Ok(imported) => format!("Ok Imported\n{}", imported_original(imported)),
        Err(error) => import_error_original(error),
    };
    std::fs::write(path.join(format!("{label}.debug.txt")), original).unwrap();
}
fn retain_outcome(path: &Path, label: &str, result: &Result<mfd::ImportOutcome, MfdError>) {
    let original = match result {
        Ok(outcome) => format!(
            "Ok ImportOutcome\nreport: {:#?}\n{}",
            outcome.report,
            imported_original(&outcome.imported)
        ),
        Err(error) => import_error_original(error),
    };
    std::fs::write(path.join(format!("{label}.debug.txt")), original).unwrap();
}
fn optional(mut node: SchemaNode) -> SchemaNode {
    assert!(node.set_xml_optional(true));
    node
}
fn field(name: &str) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame: Some(vec!["Item".into()]),
    }
}
fn project() -> Project {
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
                (4, Node::Raise { message: Some(10) }),
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
                        then: 4,
                        else_: 5,
                    },
                ),
                (
                    10,
                    Node::Position {
                        collection: vec!["Item".into()],
                    },
                ),
                (
                    11,
                    Node::Position {
                        collection: vec!["Item".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                filter: Some(6),
                bindings: vec![
                    Binding {
                        target_field: "Result".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Rank".into(),
                        node: 11,
                    },
                ],
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
fn input(values: &[(i64, Value)]) -> Instance {
    group(vec![(
        "Item",
        Instance::Repeated(
            values
                .iter()
                .map(|(value, blocked)| {
                    group(vec![
                        ("Value", Instance::Scalar(Value::Int(*value))),
                        ("Blocked", Instance::Scalar(blocked.clone())),
                        ("Message", Instance::Scalar(Value::String("unused".into()))),
                    ])
                })
                .collect(),
        ),
    )])
}
fn rows(values: &[(i64, i64)]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            values
                .iter()
                .map(|(value, rank)| {
                    group(vec![
                        ("Result", Instance::Scalar(Value::Int(*value))),
                        ("Rank", Instance::Scalar(Value::Int(*rank))),
                    ])
                })
                .collect(),
        ),
    )])
}
fn evaluate(
    path: &Path,
    label: &str,
    project: &Project,
    source: &Instance,
) -> Result<Instance, EngineError> {
    retain(path, &format!("{label}-project"), project);
    retain(path, &format!("{label}-input"), source);
    let result = engine::run(project, source);
    retain(path, label, &result);
    result
}
fn exported(path: &Path, project: &Project, profile: ExportProfile) -> PathBuf {
    retain(path, "authored-project", project);
    let validation = engine::validate(project);
    retain(path, "authored-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    let mapping = path.join("design.mfd");
    let result = mfd::export_with_profile(project, &mapping, profile);
    retain(path, "export", &result);
    let report = result.unwrap();
    assert!(
        report.is_native_compatible() && report.warnings.is_empty(),
        "{report:?}"
    );
    std::fs::write(path.join("input.xml"), "<Input><Item><Value>10</Value><Blocked>false</Blocked><Message>unused</Message></Item></Input>").unwrap();
    mapping
}
fn imported(path: &Path, mapping: &Path, item_order: bool) -> Project {
    let options = ImportOptions::default().with_package_root(path);
    let options = if item_order {
        options.with_item_ordered_exceptions()
    } else {
        options
    };
    let result = mfd::import_with_profile(mapping, &options, ImportProfile::Executable);
    retain_outcome(
        path,
        if item_order {
            "item-import"
        } else {
            "legacy-import"
        },
        &result,
    );
    let outcome = result.unwrap();
    assert!(
        outcome.report.executable
            && outcome.report.issues.is_empty()
            && outcome.imported.warnings.is_empty(),
        "report: {:?}; warnings: {:?}",
        outcome.report,
        outcome.imported.warnings
    );
    let validation = engine::validate(&outcome.imported.project);
    retain(path, "imported-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    outcome.imported.project
}
fn guard(project: &Project) -> (u32, u32, Option<u32>) {
    let filter = project.root.children[0].filter.unwrap();
    let Some(Node::If {
        condition,
        then,
        else_,
    }) = project.graph.nodes.get(&filter)
    else {
        panic!("exact lazy owner guard required")
    };
    let (raise, yes) = if matches!(project.graph.nodes.get(then), Some(Node::Raise { .. })) {
        (*then, *else_)
    } else {
        (*else_, *then)
    };
    assert!(matches!(
        project.graph.nodes.get(&yes),
        Some(Node::Const {
            value: Value::Bool(true)
        })
    ));
    let Some(Node::Raise { message }) = project.graph.nodes.get(&raise) else {
        panic!("selected branch must own Raise")
    };
    assert!(project.failure_rules.is_empty());
    (*condition, raise, *message)
}
fn component<'a, 'input>(
    doc: &'a roxmltree::Document<'input>,
    name: &str,
) -> roxmltree::Node<'a, 'input> {
    doc.descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
        .unwrap()
}
fn pin(node: roxmltree::Node<'_, '_>, side: &str, pos: u32) -> u32 {
    node.children()
        .find(|node| node.has_tag_name(side))
        .unwrap()
        .children()
        .find(|node| {
            node.has_tag_name("datapoint")
                && node
                    .attribute("pos")
                    .and_then(|value| value.parse::<u32>().ok())
                    == Some(pos)
        })
        .unwrap()
        .attribute("key")
        .unwrap()
        .parse()
        .unwrap()
}
fn entry(node: roxmltree::Node<'_, '_>, name: &str, attribute: &str) -> u32 {
    node.descendants()
        .find(|node| {
            node.has_tag_name("entry")
                && node.attribute("name") == Some(name)
                && node.attribute(attribute).is_some()
        })
        .unwrap()
        .attribute(attribute)
        .unwrap()
        .parse()
        .unwrap()
}
fn edges(doc: &roxmltree::Document<'_>) -> Vec<(u32, u32)> {
    doc.descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .flat_map(|vertex| {
            let from = vertex.attribute("vertexkey").unwrap().parse().unwrap();
            vertex
                .descendants()
                .filter(|node| node.has_tag_name("edge"))
                .map(move |edge| (from, edge.attribute("vertexkey").unwrap().parse().unwrap()))
        })
        .collect()
}
fn rewrite_edges(xml: &str, change: impl FnOnce(&mut Vec<(u32, u32)>)) -> String {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let graph = doc
        .descendants()
        .find(|node| node.has_tag_name("graph"))
        .unwrap();
    let mut connections = edges(&doc);
    change(&mut connections);
    let replacement = format!(
        "<connections>{}</connections>",
        connections
            .into_iter()
            .map(|(from, to)| format!("<edge from=\"{from}\" to=\"{to}\"/>"))
            .collect::<String>()
    );
    let mut result = xml.to_owned();
    result.replace_range(graph.range(), &replacement);
    result
}
fn replace_component(xml: &str, name: &str, change: impl FnOnce(&str) -> String) -> String {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let node = component(&doc, name);
    let range = node.range();
    let replacement = change(&xml[range.clone()]);
    let mut result = xml.to_owned();
    result.replace_range(range, &replacement);
    result
}
fn refusal(path: &Path, mapping: &Path, expected: &str) {
    let before = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .collect::<Vec<_>>();
    let options = ImportOptions::default()
        .with_package_root(path)
        .with_item_ordered_exceptions();
    let partial = mfd::import_with_options(mapping, &options);
    retain_imported(path, "partial-original", &partial);
    let partial = partial.unwrap();
    assert!(
        partial
            .warnings
            .iter()
            .any(|warning| warning.contains(expected)),
        "{:?}",
        partial.warnings
    );
    assert!(partial.project.failure_rules.is_empty());
    assert!(
        !partial
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Raise { .. }))
    );
    let strict = mfd::import_with_profile(mapping, &options, ImportProfile::Executable);
    retain_outcome(path, "strict-original", &strict);
    let Err(MfdError::IncompatibleImport(report)) = strict else {
        panic!("typed strict refusal required")
    };
    assert!(!report.executable);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::ImportWarning
                && issue.message.contains(expected)),
        "{report:?}"
    );
    for (name, bytes) in before {
        assert_eq!(std::fs::read(path.join(name)).unwrap(), bytes);
    }
    assert!(!path.join("accepted.xml").exists());
    assert!(!path.join("project.json").exists());
}

#[test]
fn typed_scope_sequence_reimports_two_cycles_with_exact_owner_raw_message_and_compact_target_positions()
 {
    let dir = Directory::new("cycles");
    for polarity in [true, false] {
        let safe = input(&[(10, Value::Bool(!polarity)), (20, Value::Bool(!polarity))]);
        let late = input(&[(10, Value::Bool(!polarity)), (20, Value::Bool(polarity))]);
        for (profile_index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
            .into_iter()
            .enumerate()
        {
            let mut candidate = project();
            if !polarity {
                candidate.graph.nodes.insert(
                    6,
                    Node::If {
                        condition: 2,
                        then: 5,
                        else_: 4,
                    },
                );
            }
            for cycle in 0..2 {
                let path = dir.child(&format!("{polarity}-{profile_index}-{cycle}"));
                let design = exported(&path, &candidate, profile);
                let xml = std::fs::read_to_string(&design).unwrap();
                let doc = roxmltree::Document::parse(&xml).unwrap();
                let variable = component(&doc, "scope-sequence");
                let native_edges = edges(&doc);
                assert!(native_edges.contains(&(
                    entry(component(&doc, "Input"), "Input", "outkey"),
                    entry(variable, "compute-when", "inpkey")
                )));
                assert!(native_edges.contains(&(
                    pin(
                        component(&doc, "filter"),
                        "targets",
                        if polarity { 1 } else { 0 }
                    ),
                    entry(variable, "Item", "inpkey")
                )));
                assert!(native_edges.contains(&(
                    entry(variable, "Item", "outkey"),
                    entry(component(&doc, "Output"), "Row", "inpkey")
                )));
                candidate = imported(&path, &design, true);
                let (predicate, raise, message) = guard(&candidate);
                assert!(
                    matches!(&candidate.graph.nodes[&predicate], Node::SourceField { path, frame } if path.as_slice() == ["Blocked"] && frame.as_deref() == Some(&["Item".to_owned()][..]))
                );
                let message = message.unwrap();
                let rank = candidate.root.children[0]
                    .bindings
                    .iter()
                    .find(|binding| binding.target_field == "Rank")
                    .unwrap()
                    .node;
                assert_ne!(message, rank);
                for id in [message, rank] {
                    assert!(
                        matches!(&candidate.graph.nodes[&id], Node::Position { collection } if collection.as_slice() == ["Item"])
                    );
                }
                assert_eq!(
                    candidate.root.children[0].source(),
                    Some(&["Item".to_owned()][..])
                );
                assert_eq!(
                    evaluate(&path, "safe", &candidate, &safe).unwrap(),
                    rows(&[(10, 1), (20, 2)])
                );
                assert_eq!(
                    evaluate(&path, "empty", &candidate, &input(&[])).unwrap(),
                    rows(&[])
                );
                assert!(
                    matches!(evaluate(&path, "late", &candidate, &late), Err(EngineError::MappingException { node, message: Some(message) }) if node == raise && message == "2")
                );
                let output = evaluate(&path, "writer-safe", &candidate, &safe).unwrap();
                let actual = format_xml::to_string(&candidate.target, &output);
                retain(&path, "writer-full-original", &actual);
                let literal = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Result>10</Result>\n    <Rank>1</Rank>\n  </Row>\n  <Row>\n    <Result>20</Result>\n    <Rank>2</Rank>\n  </Row>\n</Output>";
                std::fs::write(path.join("hand-output.xml"), literal).unwrap();
                assert_eq!(actual.unwrap(), literal);
            }
        }
    }
}

#[test]
fn scope_sequence_import_keeps_lazy_message_original_predicate_errors_and_first_selected_position()
{
    let dir = Directory::new("lazy");
    let path = dir.child("divide-message");
    let mut candidate = project();
    candidate.graph.nodes.insert(
        12,
        Node::Const {
            value: Value::Int(0),
        },
    );
    candidate.graph.nodes.insert(
        13,
        Node::Call {
            function: "divide".into(),
            args: vec![0, 12],
        },
    );
    candidate
        .graph
        .nodes
        .insert(4, Node::Raise { message: Some(13) });
    candidate.graph.nodes.remove(&10);
    let design = exported(&path, &candidate, ExportProfile::NativeMfd);
    let candidate = imported(&path, &design, true);
    assert_eq!(
        evaluate(
            &path,
            "unselected",
            &candidate,
            &input(&[(10, Value::Bool(false)), (20, Value::Bool(false))])
        )
        .unwrap(),
        rows(&[(10, 1), (20, 2)])
    );
    assert!(
        matches!(evaluate(&path, "selected-message-divide", &candidate, &input(&[(10, Value::Bool(false)), (20, Value::Bool(true))])), Err(EngineError::Function(source)) if matches!(source, functions::FunctionError::DivideByZero))
    );
    let (predicate, _, _) = guard(&candidate);
    // Typed host controls intentionally exceed XML Boolean lexical inputs.
    for (label, value, found) in [("null", Value::Null, "null"), ("int", Value::Int(1), "int")] {
        assert!(
            matches!(evaluate(&path, label, &candidate, &input(&[(10, value)])), Err(EngineError::NotABool { node, found: actual }) if node == predicate && actual == found)
        );
    }
    let path = dir.child("position-first");
    let candidate = project();
    let design = exported(&path, &candidate, ExportProfile::NativeMfd);
    let candidate = imported(&path, &design, true);
    let (_, raise, _) = guard(&candidate);
    assert!(
        matches!(evaluate(&path, "first-selected", &candidate, &input(&[(10, Value::Bool(true)), (20, Value::Bool(true))])), Err(EngineError::MappingException { node, message: Some(message) }) if node == raise && message == "1")
    );
}

#[test]
fn scope_sequence_item_import_preserves_earlier_target_error_and_legacy_global_scan_default() {
    let dir = Directory::new("ordering");
    let mut candidate = project();
    candidate.graph.nodes.insert(
        12,
        Node::Const {
            value: Value::Int(0),
        },
    );
    candidate.graph.nodes.insert(
        13,
        Node::Call {
            function: "divide".into(),
            args: vec![0, 12],
        },
    );
    candidate.root.children[0].bindings[0].node = 13;
    let design = exported(&dir.0, &candidate, ExportProfile::NativeMfd);
    let item = imported(&dir.0, &design, true);
    let legacy = imported(&dir.0, &design, false);
    assert_eq!(legacy.failure_rules.len(), 1);
    assert!(
        !legacy
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Raise { .. }))
    );
    let late = input(&[(10, Value::Bool(false)), (20, Value::Bool(true))]);
    assert!(
        matches!(evaluate(&dir.0, "item-earlier-target", &item, &late), Err(EngineError::Function(source)) if matches!(source, functions::FunctionError::DivideByZero))
    );
    assert!(
        matches!(evaluate(&dir.0, "legacy-global-late", &legacy, &late), Err(EngineError::MappingFailure { rule: 1, message: Some(message) }) if message == "2")
    );
    let (_, raise, _) = guard(&item);
    assert!(
        matches!(evaluate(&dir.0, "selected-before-target", &item, &input(&[(10, Value::Bool(true))])), Err(EngineError::MappingException { node, message: Some(message) }) if node == raise && message == "1")
    );
    assert!(!dir.0.join("output.xml").exists());
}

#[test]
fn scope_sequence_opt_in_refuses_cross_context_positions_triggers_schema_changes_and_extra_inputs()
{
    let dir = Directory::new("refusals");
    for label in [
        "item-trigger",
        "raw-target-position",
        "filtered-message-position",
        "carrier-message",
        "extra-input",
        "position-extra-input",
        "facet-mismatch",
        "second-variable",
    ] {
        let path = dir.child(label);
        let candidate = project();
        let design = exported(&path, &candidate, ExportProfile::NativeMfd);
        let original = std::fs::read_to_string(&design).unwrap();
        let doc = roxmltree::Document::parse(&original).unwrap();
        let source = component(&doc, "Input");
        let variable = component(&doc, "scope-sequence");
        let exception = component(&doc, "exception");
        let raw = entry(source, "Item", "outkey");
        let root_output = entry(variable, "Item", "outkey");
        let scalar = entry(variable, "Value", "outkey");
        let compute = entry(variable, "compute-when", "inpkey");
        let message_input = pin(exception, "sources", 1);
        let positions = doc
            .descendants()
            .filter(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("position")
            })
            .collect::<Vec<_>>();
        let native_edges = edges(&doc);
        let message_position = positions
            .iter()
            .copied()
            .find(|node| native_edges.contains(&(pin(*node, "targets", 0), message_input)))
            .unwrap();
        let target_position = positions
            .iter()
            .copied()
            .find(|node| node.id() != message_position.id())
            .unwrap();
        let (changed, expected) = match label {
            "item-trigger" => (
                rewrite_edges(&original, |edges| {
                    edges.iter_mut().find(|(_, to)| *to == compute).unwrap().0 = raw;
                }),
                "compute trigger must exclusively read the primary parent root",
            ),
            "raw-target-position" => (
                rewrite_edges(&original, |edges| {
                    edges
                        .iter_mut()
                        .find(|(_, to)| *to == pin(target_position, "sources", 0))
                        .unwrap()
                        .0 = raw;
                }),
                "position reads the wrong raw or filtered sequence",
            ),
            "filtered-message-position" => (
                rewrite_edges(&original, |edges| {
                    edges
                        .iter_mut()
                        .find(|(_, to)| *to == pin(message_position, "sources", 0))
                        .unwrap()
                        .0 = root_output;
                }),
                "position reads the wrong raw or filtered sequence",
            ),
            "carrier-message" => (
                rewrite_edges(&original, |edges| {
                    edges
                        .iter_mut()
                        .find(|(_, to)| *to == message_input)
                        .unwrap()
                        .0 = scalar;
                }),
                "raw predicate/message and target positions must remain distinct",
            ),
            "extra-input" => {
                let changed = replace_component(&original, "scope-sequence", |body| {
                    let doc = roxmltree::Document::parse(body).unwrap();
                    let value = doc
                        .descendants()
                        .find(|node| {
                            node.has_tag_name("entry") && node.attribute("name") == Some("Value")
                        })
                        .unwrap();
                    let index = value.range().start + "<entry".len();
                    let mut result = body.to_owned();
                    result.insert_str(index, " inpkey=\"5000\"");
                    result
                });
                (
                    rewrite_edges(&changed, |edges| {
                        edges.push((entry(source, "Blocked", "outkey"), 5000))
                    }),
                    "keep branch must exclusively feed its unmodified root",
                )
            }
            "position-extra-input" => {
                let range = target_position.range();
                let body = &original[range.clone()];
                let changed_body = body.replace(
                    "</sources>",
                    "<datapoint pos=\"1\" key=\"5001\"/></sources>",
                );
                assert_ne!(changed_body, body);
                let mut changed = original.clone();
                changed.replace_range(range, &changed_body);
                (
                    rewrite_edges(&changed, |edges| {
                        edges.push((entry(source, "Blocked", "outkey"), 5001))
                    }),
                    "position must have one exact sequence input",
                )
            }
            "facet-mismatch" => {
                let mut source_schema = candidate.source.clone();
                let SchemaKind::Group { children, .. } = &mut source_schema.kind else {
                    unreachable!()
                };
                let SchemaKind::Group { children, .. } = &mut children[0].kind else {
                    unreachable!()
                };
                assert!(children[0].set_xml_optional(true));
                std::fs::write(
                    path.join("different-source.xsd"),
                    format_xml::xsd::export(&source_schema).unwrap(),
                )
                .unwrap();
                (
                    replace_component(&original, "scope-sequence", |body| {
                        body.replace(
                            "schema=\"design-source.xsd\"",
                            "schema=\"different-source.xsd\"",
                        )
                    }),
                    "variable must retain the exact local owner XML schema",
                )
            }
            "second-variable" => {
                let range = variable.range();
                let body = &original[range.clone()];
                // A second valid unconnected typed variable has no duplicate
                // ports and cannot be silently treated as our exclusive carrier.
                let duplicate = body.replace("scope-sequence", "another-sequence").replace(
                    &format!("uid=\"{}\"", variable.attribute("uid").unwrap()),
                    "uid=\"9000\"",
                );
                let duplicate_doc = roxmltree::Document::parse(&duplicate).unwrap();
                let mut duplicate = duplicate.clone();
                let mut attributes = duplicate_doc
                    .descendants()
                    .flat_map(|node| {
                        ["inpkey", "outkey"]
                            .into_iter()
                            .filter_map(move |name| node.attribute(name).map(|value| (name, value)))
                    })
                    .collect::<Vec<_>>();
                attributes.sort_unstable();
                attributes.dedup();
                for (name, value) in attributes {
                    duplicate = duplicate.replace(
                        &format!("{name}=\"{value}\""),
                        &format!("{name}=\"{}\"", value.parse::<u32>().unwrap() + 6000),
                    );
                }
                let mut changed = original.clone();
                changed.insert_str(range.end, &duplicate);
                (changed, "requires exactly one typed XML variable")
            }
            _ => unreachable!(),
        };
        std::fs::write(&design, changed).unwrap();
        refusal(&path, &design, expected);
    }
}
