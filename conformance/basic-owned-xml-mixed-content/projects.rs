//! Models for the owned XML mixed-content corpus (#468). Each model is a
//! complete Project; `cases.json` refers to these keys and node ids.
use ir::{ScalarType, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{
    Binding, Graph, NamedSource, NamedTarget, Node, NodeId, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, OwnedXmlMixedContent, OwnedXmlMixedContentContext,
    OwnedXmlMixedContentReplacement, Project, Scope, ScopeIteration, SourceOwner,
    XmlMixedContentReplacement,
};

pub const MODELS: &[&str] = &["M01-endpoints", "M02-nested", "M03-frames", "M04-lazy"];

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

/// A mixed paragraph: text, repeated `b` scalars, repeated `i` groups with an
/// attribute and text, and one optional `u`.
pub fn para_schema() -> SchemaNode {
    SchemaNode::group(
        "Para",
        vec![
            string(XML_TEXT_FIELD).text(),
            string("b").repeating(),
            SchemaNode::group(
                "i",
                vec![string("n").attribute(), string(XML_TEXT_FIELD).text()],
            )
            .repeating(),
            string("u"),
        ],
    )
}

/// The primary and every named endpoint share this declared shape, so only
/// the owner distinguishes their values.
pub fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Title"),
            para_schema(),
            SchemaNode::group(
                "Docs",
                vec![SchemaNode::group("Doc", vec![string("id").attribute(), para_schema()])
                    .repeating()],
            ),
        ],
    )
}

pub fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn owned_ref(owner_name: Option<&str>, segments: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(owner_name),
        path: path(segments),
    }
}

/// A replacement: `(element, occurrence context, expression)`.
type Rule<'a> = (&'a str, bool, NodeId);

pub fn mixed(
    owner_name: Option<&str>,
    segments: &[&str],
    frame: Option<&[&str]>,
    rules: &[Rule<'_>],
) -> Node {
    Node::OwnedXmlMixedContent(OwnedXmlMixedContent {
        source: owned_ref(owner_name, segments),
        frame: frame.map(path),
        replacements: rules
            .iter()
            .map(|(element, occurrence, expression)| OwnedXmlMixedContentReplacement {
                element: (*element).into(),
                context: if *occurrence {
                    OwnedXmlMixedContentContext::Occurrence
                } else {
                    OwnedXmlMixedContentContext::Parent
                },
                expression: *expression,
            })
            .collect(),
    })
}

fn field(owner_name: Option<&str>, segments: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: owned_ref(owner_name, segments),
        frame: None,
    })
}

fn position(owner_name: Option<&str>, collection: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(owner_name),
        collection: Some(path(collection)),
    })
}

fn text(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}

fn concat(args: &[NodeId]) -> Node {
    Node::Call {
        function: "concat".into(),
        args: args.to_vec(),
    }
}

fn bindings(pairs: &[(&str, NodeId)]) -> Vec<Binding> {
    pairs
        .iter()
        .map(|(target, node)| Binding {
            target_field: (*target).into(),
            node: *node,
        })
        .collect()
}

fn strings(name: &str, fields: &[&str]) -> SchemaNode {
    SchemaNode::group(name, fields.iter().map(|field| string(field)).collect())
}

fn named(name: &str) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema: input_schema(),
        options: Default::default(),
        dynamic_path: None,
    }
}

fn target(name: &str, schema: SchemaNode, bound: &[(&str, NodeId)]) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: Some(format!("{name}.json")),
        schema,
        options: Default::default(),
        root: Scope {
            bindings: bindings(bound),
            ..Scope::default()
        },
    }
}

/// `notes` and `Para` are named endpoints; `Para` is also spelled like the
/// primary root field holding the mixed paragraph.
pub fn base(target: SchemaNode, root: Scope, nodes: Vec<(NodeId, Node)>) -> Project {
    Project {
        source: input_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![named("notes"), named("Para")],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}

fn endpoints() -> Project {
    let para = ["Para"];
    let b = ["Para", "b"];
    let mut project = base(
        strings("Output", &["Primary", "Alias"]),
        Scope {
            bindings: bindings(&[("Primary", 1), ("Alias", 3)]),
            ..Scope::default()
        },
        vec![
            (1, mixed(None, &para, None, &[("b", true, 10), ("i", false, 15)])),
            (
                2,
                mixed(Some("notes"), &para, None, &[("b", true, 20), ("i", true, 24)]),
            ),
            (3, mixed(Some("Para"), &para, None, &[])),
            (10, concat(&[11, 12, 13, 14, 16])),
            (11, text("[")),
            (12, position(None, &b)),
            (13, text(":")),
            (14, field(None, &b)),
            (16, text("]")),
            (15, field(None, &["Title"])),
            (20, concat(&[11, 21, 13, 22, 16])),
            (21, position(Some("notes"), &b)),
            (22, field(Some("notes"), &b)),
            (24, field(Some("notes"), &["Para", "i", "n"])),
        ],
    );
    project.extra_targets = vec![target("noted", strings("Noted", &["Notes"]), &[("Notes", 2)])];
    project
}

fn nested() -> Project {
    let row = ["Docs", "Doc"];
    let para = ["Docs", "Doc", "Para"];
    let b = ["Docs", "Doc", "Para", "b"];
    base(
        SchemaNode::group(
            "Output",
            vec![strings("Row", &["Pinned", "Free"]).repeating()],
        ),
        Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: owned_ref(None, &row),
                    output_path: None,
                },
                bindings: bindings(&[("Pinned", 30), ("Free", 40)]),
                ..Scope::default()
            }],
            ..Scope::default()
        },
        vec![
            (30, mixed(None, &para, Some(&row), &[("b", true, 31)])),
            (31, concat(&[32, 33, 34, 35, 36, 37, 38])),
            (32, position(None, &row)),
            (33, text(".")),
            (34, position(None, &b)),
            (35, text("=")),
            (36, field(None, &b)),
            (37, text("@")),
            (38, field(None, &["Docs", "Doc", "id"])),
            (40, mixed(None, &para, None, &[("i", false, 38), ("b", true, 41)])),
            (41, concat(&[42, 34])),
            (42, field(Some("notes"), &["Title"])),
        ],
    )
}

fn frames() -> Project {
    let row = ["Docs", "Doc"];
    let rows = ["Docs", "Doc", "Para"];
    let mut project = base(
        SchemaNode::group("Output", vec![strings("Row", &["Root"]).repeating()]),
        Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: owned_ref(None, &row),
                    output_path: None,
                },
                bindings: bindings(&[("Root", 50)]),
                ..Scope::default()
            }],
            ..Scope::default()
        },
        vec![
            (50, mixed(None, &["Para"], Some(&[]), &[("b", true, 51)])),
            (51, field(None, &["Para", "b"])),
            (52, mixed(None, &rows, Some(&row), &[])),
            (53, mixed(None, &rows, None, &[])),
            (54, mixed(None, &["Para"], None, &[("b", true, 55)])),
            (55, field(Some("notes"), &["Para", "b"])),
        ],
    );
    let one = |name: &str, node| target(name, strings("Probe", &["Text"]), &[("Text", node)]);
    project.extra_targets = vec![
        one("inactive", 52),
        one("uniterated", 53),
        one("foreign", 54),
    ];
    project
}

fn lazy() -> Project {
    let b = ["Para", "b"];
    base(
        strings("Output", &["Text"]),
        Scope {
            bindings: bindings(&[("Text", 60)]),
            ..Scope::default()
        },
        vec![
            (60, mixed(None, &["Para"], None, &[("b", true, 61), ("u", false, 66)])),
            (
                61,
                Node::If {
                    condition: 62,
                    then: 65,
                    else_: 64,
                },
            ),
            (
                62,
                Node::Call {
                    function: "equal".into(),
                    args: vec![63, 67],
                },
            ),
            (63, position(None, &b)),
            (67, Node::Const { value: Value::Int(2) }),
            (64, field(None, &b)),
            (65, Node::Raise { message: Some(68) }),
            (68, text("second b")),
            (66, field(Some("notes"), &["Title"])),
        ],
    )
}

pub fn project(model: &str) -> Project {
    match model {
        "M01-endpoints" => endpoints(),
        "M02-nested" => nested(),
        "M03-frames" => frames(),
        "M04-lazy" => lazy(),
        _ => unreachable!("authored model"),
    }
}

/// The legacy control: the unchanged legacy node over the same schema.
pub fn legacy() -> Project {
    base(
        strings("Output", &["Primary"]),
        Scope {
            bindings: bindings(&[("Primary", 1)]),
            ..Scope::default()
        },
        vec![
            (
                1,
                Node::XmlMixedContent {
                    path: path(&["Para"]),
                    frame: None,
                    replacements: vec![XmlMixedContentReplacement {
                        element: "b".into(),
                        collection: path(&["Para", "b"]),
                        expression: 2,
                    }],
                },
            ),
            (
                2,
                Node::SourceField {
                    path: vec![],
                    frame: Some(path(&["Para", "b"])),
                },
            ),
        ],
    )
}
