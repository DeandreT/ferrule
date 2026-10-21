//! Models for the owned XML serialization corpus (#469). Each model is a
//! complete Project; `cases.json` refers to these keys and node ids.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, NamedSource, NamedTarget,
    Node, NodeId, OwnedSourceField, OwnedSourceRef, OwnedXmlSerialize, Project, Scope,
    ScopeIteration, SourceOwner,
};

pub const MODELS: &[&str] = &[
    "X01-endpoints",
    "X02-nested",
    "X03-frames",
    "X04-grouped",
    "X05-failure-walk",
];

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

pub fn header_schema() -> SchemaNode {
    SchemaNode::group(
        "Header",
        vec![
            string("id").attribute(),
            string("Note"),
            string("Tag").repeating(),
        ],
    )
}

pub fn line_schema() -> SchemaNode {
    SchemaNode::group(
        "Line",
        vec![
            string("n").attribute(),
            SchemaNode::scalar("Qty", ScalarType::Int),
        ],
    )
}

pub fn detail_schema() -> SchemaNode {
    SchemaNode::group(
        "Detail",
        vec![
            string("code").attribute(),
            string("Text"),
            line_schema().repeating(),
        ],
    )
}

/// The primary and every named endpoint share this declared shape, so only
/// the owner distinguishes their values.
pub fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            header_schema(),
            SchemaNode::group(
                "Orders",
                vec![
                    SchemaNode::group(
                        "Order",
                        vec![string("num").attribute(), string("Label"), detail_schema()],
                    )
                    .repeating(),
                ],
            ),
            string("Single"),
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

/// Authored document options: `(declaration, indent, namespace)`.
type Options<'a> = (bool, bool, Option<&'a str>);

const COMPACT: Options<'static> = (false, false, None);

pub fn serializer(
    owner_name: Option<&str>,
    segments: &[&str],
    frame: Option<&[&str]>,
    schema: SchemaNode,
    (declaration, indent, namespace): Options<'_>,
) -> Node {
    Node::OwnedXmlSerialize(OwnedXmlSerialize {
        source: OwnedSourceRef {
            owner: owner(owner_name),
            path: path(segments),
        },
        frame: frame.map(path),
        schema: Box::new(schema),
        declaration,
        indent,
        namespace: namespace.map(Into::into),
    })
}

fn field(owner_name: Option<&str>, segments: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner(owner_name),
            path: path(segments),
        },
        frame: None,
    })
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

fn owned_iteration(owner_name: Option<&str>, segments: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: owner(owner_name),
            path: path(segments),
        },
        output_path: None,
    }
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

fn target(name: &str, schema: SchemaNode, root: Scope) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: Some(format!("{name}.json")),
        schema,
        options: Default::default(),
        root,
    }
}

/// `audit` and `Header` are named endpoints; `Header` is also spelled like the
/// primary root field the serializers read.
pub fn base(target: SchemaNode, root: Scope, nodes: Vec<(NodeId, Node)>) -> Project {
    Project {
        source: input_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![named("audit"), named("Header")],
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
    let mut project = base(
        strings("Output", &["Primary", "Alias"]),
        Scope {
            bindings: bindings(&[("Primary", 1), ("Alias", 3)]),
            ..Scope::default()
        },
        vec![
            (1, serializer(None, &["Header"], None, header_schema(), COMPACT)),
            (
                2,
                serializer(
                    Some("audit"),
                    &["Header"],
                    None,
                    header_schema(),
                    (true, true, Some("urn:audit")),
                ),
            ),
            (
                3,
                serializer(
                    Some("Header"),
                    &["Header"],
                    None,
                    header_schema(),
                    (false, true, None),
                ),
            ),
        ],
    );
    project.extra_targets = vec![target(
        "audit",
        strings("AuditOutput", &["Header"]),
        Scope {
            bindings: bindings(&[("Header", 2)]),
            ..Scope::default()
        },
    )];
    project
}

/// Owned order iteration with pinned and unpinned element reads, a foreign
/// endpoint root, and a nested line iteration reading its enclosing frame.
fn nested() -> Project {
    let lines = Scope {
        target_field: "Line".into(),
        iteration: owned_iteration(None, &["Orders", "Order", "Detail", "Line"]),
        bindings: bindings(&[("Xml", 13), ("Detail", 10)]),
        ..Scope::default()
    };
    let orders = Scope {
        target_field: "Order".into(),
        iteration: owned_iteration(None, &["Orders", "Order"]),
        bindings: bindings(&[("Detail", 10), ("Unpinned", 11), ("AuditHeader", 12)]),
        children: vec![lines],
        ..Scope::default()
    };
    let schema = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Order",
                vec![
                    string("Detail"),
                    string("Unpinned"),
                    string("AuditHeader"),
                    strings("Line", &["Xml", "Detail"]).repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let audit_orders = Scope {
        children: vec![Scope {
            target_field: "Order".into(),
            iteration: owned_iteration(Some("audit"), &["Orders", "Order"]),
            bindings: bindings(&[("Detail", 20), ("PrimaryHeader", 1)]),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let mut project = base(
        schema,
        Scope {
            children: vec![orders],
            ..Scope::default()
        },
        vec![
            (1, serializer(None, &["Header"], None, header_schema(), COMPACT)),
            (
                10,
                serializer(
                    None,
                    &["Orders", "Order", "Detail"],
                    Some(&["Orders", "Order"]),
                    detail_schema(),
                    COMPACT,
                ),
            ),
            (
                11,
                serializer(
                    None,
                    &["Orders", "Order", "Detail"],
                    None,
                    detail_schema(),
                    (false, true, Some("urn:detail")),
                ),
            ),
            (
                12,
                serializer(Some("audit"), &["Header"], None, header_schema(), COMPACT),
            ),
            (
                13,
                serializer(
                    None,
                    &["Orders", "Order", "Detail", "Line"],
                    None,
                    line_schema(),
                    COMPACT,
                ),
            ),
            (
                20,
                serializer(
                    Some("audit"),
                    &["Orders", "Order", "Detail"],
                    Some(&["Orders", "Order"]),
                    detail_schema(),
                    COMPACT,
                ),
            ),
        ],
    );
    project.extra_targets = vec![target(
        "audit",
        SchemaNode::group(
            "AuditOutput",
            vec![strings("Order", &["Detail", "PrimaryHeader"]).repeating()],
        ),
        audit_orders,
    )];
    project
}

/// Explicit root pins inside an active row, plus targets whose pinned or
/// unpinned reads have no exact active occurrence.
fn frames() -> Project {
    let orders = Scope {
        target_field: "Order".into(),
        iteration: owned_iteration(None, &["Orders", "Order"]),
        bindings: bindings(&[("RootHeader", 30), ("AuditHeader", 31)]),
        ..Scope::default()
    };
    let mut project = base(
        SchemaNode::group(
            "Output",
            vec![strings("Order", &["RootHeader", "AuditHeader"]).repeating()],
        ),
        Scope {
            children: vec![orders],
            ..Scope::default()
        },
        vec![
            (
                30,
                serializer(None, &["Header"], Some(&[]), header_schema(), COMPACT),
            ),
            (
                31,
                serializer(
                    Some("audit"),
                    &["Header"],
                    Some(&[]),
                    header_schema(),
                    (true, false, None),
                ),
            ),
            (
                32,
                serializer(
                    None,
                    &["Orders", "Order", "Detail"],
                    Some(&["Orders", "Order"]),
                    detail_schema(),
                    COMPACT,
                ),
            ),
            (
                33,
                serializer(
                    None,
                    &["Orders", "Order", "Detail"],
                    None,
                    detail_schema(),
                    COMPACT,
                ),
            ),
        ],
    );
    project.extra_targets = vec![
        target(
            "inactive",
            strings("Inactive", &["Detail"]),
            Scope {
                bindings: bindings(&[("Detail", 32)]),
                ..Scope::default()
            },
        ),
        target(
            "uniterated",
            strings("Uniterated", &["Detail"]),
            Scope {
                bindings: bindings(&[("Detail", 33)]),
                ..Scope::default()
            },
        ),
    ];
    project
}

/// Owned By groups: element reads use the first surviving member, while the
/// member iteration reads each member's own element.
fn grouped() -> Project {
    let members = Scope {
        target_field: "Member".into(),
        iteration: owned_iteration(None, &["Orders", "Order"]),
        bindings: bindings(&[("Detail", 41)]),
        ..Scope::default()
    };
    let groups = Scope {
        target_field: "Group".into(),
        iteration: owned_iteration(None, &["Orders", "Order"]),
        group_by: Some(40),
        bindings: bindings(&[("First", 41), ("Pinned", 42)]),
        children: vec![members],
        ..Scope::default()
    };
    base(
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Group",
                    vec![
                        string("First"),
                        string("Pinned"),
                        strings("Member", &["Detail"]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        Scope {
            children: vec![groups],
            ..Scope::default()
        },
        vec![
            (40, field(None, &["Orders", "Order", "Label"])),
            (
                41,
                serializer(
                    None,
                    &["Orders", "Order", "Detail"],
                    None,
                    detail_schema(),
                    COMPACT,
                ),
            ),
            (
                42,
                serializer(
                    None,
                    &["Orders", "Order", "Detail"],
                    Some(&["Orders", "Order"]),
                    detail_schema(),
                    COMPACT,
                ),
            ),
        ],
    )
}

/// An owned failure walk over `audit` orders runs before the primary element
/// is read; the walk never reads primary rows.
fn failure_walk() -> Project {
    let mut project = base(
        strings("Output", &["Primary"]),
        Scope {
            bindings: bindings(&[("Primary", 1)]),
            ..Scope::default()
        },
        vec![
            (1, serializer(None, &["Header"], None, header_schema(), COMPACT)),
            (50, field(Some("audit"), &["Orders", "Order", "Label"])),
            (
                51,
                Node::Const {
                    value: Value::String("stop".into()),
                },
            ),
            (
                52,
                Node::Call {
                    function: "equal".into(),
                    args: vec![50, 51],
                },
            ),
            (53, field(Some("audit"), &["Orders", "Order", "num"])),
        ],
    );
    project.failure_rules = vec![FailureRule {
        iteration: FailureIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: owner(Some("audit")),
                path: path(&["Orders", "Order"]),
            },
        },
        selection: FailureSelection::WhenTrue { predicate: 52 },
        message: Some(53),
    }];
    project
}

pub fn project(model: &str) -> Project {
    match model {
        "X01-endpoints" => endpoints(),
        "X02-nested" => nested(),
        "X03-frames" => frames(),
        "X04-grouped" => grouped(),
        "X05-failure-walk" => failure_walk(),
        _ => panic!("unknown owned XML serialization model {model}"),
    }
}

/// The ordinary saved control keeps the legacy serializer and its outward
/// fallback: a missing primary element is repaired from the root of the named
/// endpoint spelled like the path.
pub fn legacy() -> Project {
    base(
        strings("Output", &["Primary"]),
        Scope {
            bindings: bindings(&[("Primary", 1)]),
            ..Scope::default()
        },
        vec![(
            1,
            Node::XmlSerialize {
                path: path(&["Header"]),
                frame: None,
                schema: Box::new(header_schema()),
                declaration: false,
                indent: true,
                namespace: None,
            },
        )],
    )
}
