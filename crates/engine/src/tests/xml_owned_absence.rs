//! Structured XML lookups owned by an active repeated item treat that item's
//! absent optional group as authoritative. Only lookups without a matching
//! active owner fall back to the root context.

use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, ScopeIteration, XmlMixedContentReplacement};

const XML: NodeId = 10;

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn extra_schema() -> SchemaNode {
    SchemaNode::group(
        "Extra",
        vec![SchemaNode::scalar("Flag", ScalarType::String)],
    )
}

fn details_schema() -> SchemaNode {
    SchemaNode::group(
        "Details",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            extra_schema(),
        ],
    )
}

fn header_schema() -> SchemaNode {
    SchemaNode::group(
        "Header",
        vec![SchemaNode::scalar("Code", ScalarType::String)],
    )
}

fn source_schema() -> SchemaNode {
    let note = SchemaNode::group(
        "Note",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Em", ScalarType::String).repeating(),
        ],
    );
    SchemaNode::group(
        "Source",
        vec![
            header_schema(),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    details_schema(),
                    note,
                ],
            )
            .repeating(),
        ],
    )
}

fn source(xml: &str) -> Instance {
    format_xml::from_str(xml, &source_schema()).expect("self-authored XML fixture parses")
}

/// Binds `Name` and node `XML` for every `Rows` item. `guarded` evaluates the
/// XML node only for the row named `first`, so absent rows take the lazy
/// constant branch.
fn project(xml: Node, guarded: bool) -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Rows", "Name"]),
            frame: None,
        },
    );
    graph.nodes.insert(
        2,
        Node::Const {
            value: string("first"),
        },
    );
    graph.nodes.insert(
        3,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 2],
        },
    );
    graph.nodes.insert(
        4,
        Node::Const {
            value: string("skipped"),
        },
    );
    graph.nodes.insert(
        5,
        Node::If {
            condition: 3,
            then: XML,
            else_: 4,
        },
    );
    graph.nodes.insert(XML, xml);
    Project {
        source: source_schema(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Out",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Xml", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Out".into(),
                iteration: ScopeIteration::Source(path(&["Rows"])),
                bindings: vec![
                    Binding {
                        target_field: "Name".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "Xml".into(),
                        node: if guarded { 5 } else { XML },
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn serialize(frame: Option<&[&str]>, segments: &[&str], schema: SchemaNode) -> Node {
    Node::XmlSerialize {
        path: path(segments),
        frame: frame.map(path),
        schema: Box::new(schema),
        declaration: false,
        indent: false,
        namespace: None,
    }
}

fn mixed(frame: Option<&[&str]>, segments: &[&str]) -> Node {
    Node::XmlMixedContent {
        path: path(segments),
        frame: frame.map(path),
        replacements: vec![XmlMixedContentReplacement {
            element: "Em".into(),
            collection: Vec::new(),
            expression: 4,
        }],
    }
}

fn rows(values: &[(&str, Option<&str>)]) -> Instance {
    Instance::Group(
        vec![(
            "Out".into(),
            Instance::Repeated(
                values
                    .iter()
                    .map(|(name, xml)| {
                        Instance::Group(
                            vec![
                                ("Name".into(), Instance::Scalar(string(name))),
                                (
                                    "Xml".into(),
                                    Instance::Scalar(xml.map_or(Value::Null, string)),
                                ),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn run_ok(project: &Project, source: &Instance) -> Instance {
    assert!(validate(project).is_empty(), "{:?}", validate(project));
    run(project, source).expect("owned XML fixture runs")
}

fn assert_missing(project: &Project, source: &Instance, display: &str) {
    assert!(validate(project).is_empty(), "{:?}", validate(project));
    assert!(
        matches!(
            run(project, source),
            Err(EngineError::MissingSourceField(ref missing)) if missing == display
        ),
        "{:?}",
        run(project, source)
    );
}

const DETAILS_PRESENT_THEN_ABSENT: &str = "<Source><Header><Code>H</Code></Header>\
<Rows><Name>first</Name><Details><Code>A</Code></Details></Rows>\
<Rows><Name>second</Name></Rows></Source>";

#[test]
fn xml_serialize_does_not_borrow_the_first_rows_details() {
    let source = source(DETAILS_PRESENT_THEN_ABSENT);
    let node = || serialize(None, &["Rows", "Details"], details_schema());

    assert_eq!(
        run_ok(&project(node(), true), &source),
        rows(&[
            ("first", Some("<Details><Code>A</Code></Details>")),
            ("second", Some("skipped")),
        ])
    );
    assert_missing(&project(node(), false), &source, "Rows/Details");
}

#[test]
fn explicit_frame_xml_serialize_keeps_owned_absence() {
    let source = source(DETAILS_PRESENT_THEN_ABSENT);
    let node = || serialize(Some(&["Rows"]), &["Details"], details_schema());

    assert_eq!(
        run_ok(&project(node(), true), &source),
        rows(&[
            ("first", Some("<Details><Code>A</Code></Details>")),
            ("second", Some("skipped")),
        ])
    );
    assert_missing(&project(node(), false), &source, "Rows/Details");
}

#[test]
fn nested_optional_group_absence_is_owned_by_the_active_row() {
    let source = source(
        "<Source><Header><Code>H</Code></Header>\
<Rows><Name>first</Name><Details><Code>A</Code><Extra><Flag>yes</Flag></Extra></Details></Rows>\
<Rows><Name>second</Name><Details><Code>B</Code></Details></Rows></Source>",
    );
    for (frame, segments, display) in [
        (
            None,
            &["Rows", "Details", "Extra"][..],
            "Rows/Details/Extra",
        ),
        (
            Some(&["Rows"][..]),
            &["Details", "Extra"][..],
            "Rows/Details/Extra",
        ),
    ] {
        let node = || serialize(frame, segments, extra_schema());
        assert_eq!(
            run_ok(&project(node(), true), &source),
            rows(&[
                ("first", Some("<Extra><Flag>yes</Flag></Extra>")),
                ("second", Some("skipped")),
            ])
        );
        assert_missing(&project(node(), false), &source, display);
    }
}

#[test]
fn unowned_root_lookup_still_resolves_for_every_row() {
    let source = source(DETAILS_PRESENT_THEN_ABSENT);
    let header = "<Header><Code>H</Code></Header>";

    assert_eq!(
        run_ok(
            &project(serialize(None, &["Header"], header_schema()), false),
            &source
        ),
        rows(&[("first", Some(header)), ("second", Some(header))])
    );
}

#[test]
fn xml_mixed_content_does_not_borrow_the_first_rows_note() {
    let source = source(
        "<Source><Header><Code>H</Code></Header>\
<Rows><Name>first</Name><Note>one <Em>x</Em> end</Note></Rows>\
<Rows><Name>second</Name></Rows></Source>",
    );
    for (frame, segments, display) in [
        (None, &["Rows", "Note"][..], "Rows/Note"),
        // Framed mixed-content failures keep their existing frame-relative display.
        (Some(&["Rows"][..]), &["Note"][..], "Note"),
    ] {
        assert_eq!(
            run_ok(&project(mixed(frame, segments), true), &source),
            rows(&[
                ("first", Some("one skipped end")),
                ("second", Some("skipped"))
            ])
        );
        assert_missing(&project(mixed(frame, segments), false), &source, display);
    }
}

#[test]
fn absent_first_row_still_reports_before_a_present_second_row() {
    let source = source(
        "<Source><Header><Code>H</Code></Header>\
<Rows><Name>second</Name></Rows>\
<Rows><Name>first</Name><Details><Code>A</Code></Details></Rows></Source>",
    );
    let node = || serialize(None, &["Rows", "Details"], details_schema());

    assert_eq!(
        run_ok(&project(node(), true), &source),
        rows(&[
            ("second", Some("skipped")),
            ("first", Some("<Details><Code>A</Code></Details>")),
        ])
    );
    assert_missing(&project(node(), false), &source, "Rows/Details");
}
