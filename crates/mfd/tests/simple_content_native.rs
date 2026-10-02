use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration, SequenceExpr,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_simple_content_native_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(simple: bool) -> Project {
    let value = if simple {
        SchemaNode::group(
            "Value",
            vec![SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text()],
        )
    } else {
        SchemaNode::scalar("Value", ScalarType::String)
    };
    let mut value = value;
    value.xml_optional = true;
    let mut result = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text()],
    );
    result.xml_optional = true;
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![SchemaNode::scalar("Key", ScalarType::String), value],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![SchemaNode::scalar("Key", ScalarType::String), result],
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
                (
                    0,
                    Node::SourceField {
                        path: if simple {
                            vec!["Value".into(), XML_TEXT_FIELD.into()]
                        } else {
                            vec!["Value".into()]
                        },
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Key".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![0],
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
                    5,
                    Node::If {
                        condition: 2,
                        then: 3,
                        else_: 4,
                    },
                ),
                (
                    6,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Item".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                bindings: vec![Binding {
                    target_field: "Key".into(),
                    node: 1,
                }],
                children: vec![Scope {
                    target_field: "Result".into(),
                    iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                        from: None,
                        to: 5,
                        item: 6,
                    }),
                    iteration_output: IterationOutput::MappedSequence,
                    bindings: vec![Binding {
                        target_field: XML_TEXT_FIELD.into(),
                        node: 0,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn schema_child_mut<'a>(node: &'a mut SchemaNode, path: &[&str]) -> &'a mut SchemaNode {
    if path.is_empty() {
        return node;
    }
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        panic!("group")
    };
    schema_child_mut(
        children
            .iter_mut()
            .find(|node| node.name == path[0])
            .unwrap(),
        &path[1..],
    )
}

fn run_roundtrip(project: &Project, fixture: &Fixture) -> String {
    let input = "<Input><Item><Key>A</Key><Value>alpha &amp; beta</Value></Item><Item><Key>B</Key><Value/></Item><Item><Key>C</Key></Item></Input>";
    let source = format_xml::from_str(input, &project.source).unwrap();
    let expected = engine::run(project, &source).unwrap();
    let path = fixture.0.join("mapping.mfd");
    let report = mfd::export_with_profile(project, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let xml = std::fs::read_to_string(&path).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert!(
        !document
            .descendants()
            .any(|node| node.has_tag_name("component")
                && node.attribute("name") == Some("generate-sequence"))
    );
    let target = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("uid") == Some("3"))
        .unwrap();
    assert!(
        !target.descendants().any(
            |node| node.has_tag_name("entry") && node.attribute("name") == Some(XML_TEXT_FIELD)
        )
    );
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let imported_source = format_xml::from_str(input, &imported.project.source).unwrap();
    assert_eq!(
        engine::run(&imported.project, &imported_source).unwrap(),
        expected
    );
    let items = expected
        .field("Item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert!(items[0].field("Result").is_some());
    assert_eq!(
        items[1]
            .field("Result")
            .and_then(Instance::as_mapped_sequence)
            .and_then(|items| items.first())
            .and_then(|node| node.field(XML_TEXT_FIELD))
            .and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
    assert_eq!(
        items[2]
            .field("Result")
            .and_then(Instance::as_mapped_sequence),
        Some(&[][..])
    );
    let second = fixture.0.join("second.mfd");
    let warnings = mfd::export(&imported.project, &second).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let imported_twice = mfd::import(&second).unwrap();
    assert!(
        imported_twice.warnings.is_empty(),
        "{:?}",
        imported_twice.warnings
    );
    let source_twice = format_xml::from_str(input, &imported_twice.project.source).unwrap();
    assert_eq!(
        engine::run(&imported_twice.project, &source_twice).unwrap(),
        expected
    );
    xml
}

#[test]
fn scalar_and_simple_content_keep_empty_and_absent_distinct() {
    for simple in [false, true] {
        let fixture = Fixture::new();
        let xml = run_roundtrip(&project(simple), &fixture);
        assert!(!xml.contains("name=\"exists\""));
        assert!(!xml.contains("name=\"if-else\""));
    }
}

#[test]
fn shared_count_and_predicate_consumers_are_preserved() {
    let mut project = project(false);
    let SchemaKind::Group { children, .. } =
        &mut schema_child_mut(&mut project.target, &["Item"]).kind
    else {
        unreachable!()
    };
    children.extend([
        SchemaNode::scalar("Exists", ScalarType::Bool),
        SchemaNode::scalar("Count", ScalarType::Int),
    ]);
    project.root.children[0].bindings.extend([
        Binding {
            target_field: "Exists".into(),
            node: 2,
        },
        Binding {
            target_field: "Count".into(),
            node: 5,
        },
    ]);
    let xml = run_roundtrip(&project, &Fixture::new());
    assert!(xml.contains("name=\"exists\""));
    assert!(xml.contains("name=\"if-else\""));
}

fn assert_rejected(project: &Project) {
    let fixture = Fixture::new();
    let native = fixture.0.join("native.mfd");
    let rejected = mfd::export_with_profile(project, &native, mfd::ExportProfile::NativeMfd);
    assert!(
        matches!(rejected, Err(mfd::MfdError::IncompatibleExport(_))),
        "{rejected:?}"
    );
    assert!(!native.exists());
    assert!(std::fs::read_dir(&fixture.0).unwrap().next().is_none());
    let extension = fixture.0.join("extension.mfd");
    let report = mfd::preflight_export(project, &extension).unwrap();
    assert!(report.warnings.is_empty(), "{report:?}");
    assert!(
        report.issues.iter().any(|issue| {
            issue.feature == mfd::ExportCompatibilityFeature::XmlTextOccurrence
                && issue.component_uid.is_some()
                && issue.message.starts_with("XML simple-content scope")
        }),
        "{report:?}"
    );
    let warnings = mfd::export(project, &extension).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        std::fs::read_to_string(extension)
            .unwrap()
            .contains("name=\"#text\"")
    );
}

#[test]
fn nil_numeric_attribute_and_extra_payload_profiles_reject_atomically() {
    let mut p = project(false);
    schema_child_mut(&mut p.source, &["Item", "Value"]).nillable = true;
    assert_rejected(&p);
    let mut p = project(false);
    schema_child_mut(&mut p.source, &["Item", "Value"]).kind = SchemaKind::Scalar {
        ty: ScalarType::Int,
    };
    assert_rejected(&p);
    let mut p = project(true);
    let SchemaKind::Group { children, .. } =
        &mut schema_child_mut(&mut p.source, &["Item", "Value"]).kind
    else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("lang", ScalarType::String).attribute());
    assert_rejected(&p);
    let mut p = project(false);
    let SchemaKind::Group { children, .. } =
        &mut schema_child_mut(&mut p.target, &["Item", "Result"]).kind
    else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("lang", ScalarType::String).attribute());
    assert_rejected(&p);
}

#[test]
fn wrong_predicate_count_and_frame_reject_atomically() {
    let mut p = project(false);
    p.graph.nodes.insert(
        2,
        Node::Call {
            function: "exists".into(),
            args: vec![1],
        },
    );
    assert_rejected(&p);
    let mut p = project(false);
    p.graph.nodes.insert(
        5,
        Node::If {
            condition: 2,
            then: 4,
            else_: 3,
        },
    );
    assert_rejected(&p);
    let mut p = project(false);
    p.root.children[0].children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 3,
        item: 6,
    });
    assert_rejected(&p);
    let mut p = project(false);
    p.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Item".into(), "Value".into()],
            frame: None,
        },
    );
    assert_rejected(&p);
}

#[test]
fn filtered_parent_position_keeps_lowered_text_in_its_selected_item() {
    let mut p = project(false);
    let SchemaKind::Group { children, .. } = &mut schema_child_mut(&mut p.target, &["Item"]).kind
    else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("Position", ScalarType::Int));
    p.graph.nodes.insert(
        7,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    p.graph.nodes.insert(
        8,
        Node::Call {
            function: "exists".into(),
            args: vec![0],
        },
    );
    p.root.children[0].filter = Some(8);
    p.root.children[0].bindings.push(Binding {
        target_field: "Position".into(),
        node: 7,
    });
    let fixture = Fixture::new();
    let input = "<Input><Item><Key>A</Key></Item><Item><Key>B</Key><Value/></Item><Item><Key>C</Key><Value>selected</Value></Item></Input>";
    let source = format_xml::from_str(input, &p.source).unwrap();
    let expected = engine::run(&p, &source).unwrap();
    let path = fixture.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&p, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let input = format_xml::from_str(input, &imported.project.source).unwrap();
    assert_eq!(engine::run(&imported.project, &input).unwrap(), expected);
}

#[test]
fn required_target_keeps_graph_fidelity_for_empty_and_absent_values() {
    let mut p = project(false);
    schema_child_mut(&mut p.target, &["Item", "Result"]).xml_optional = false;
    run_roundtrip(&p, &Fixture::new());
}
