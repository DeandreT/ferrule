//! Models for the owned XML mixed-content construction corpus (#470). Each
//! model is a complete Project; `cases.json` refers to these keys and node ids.
use ir::{ScalarType, SchemaKind, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{
    Binding, Graph, NamedSource, NamedTarget, Node, NodeId, OwnedSourceField, OwnedSourceRef,
    Project, Scope, ScopeConstruction, ScopeIteration, SourceOwner, XmlMixedContentElement,
};

pub const MODELS: &[&str] = &["C01-endpoints", "C02-nested", "C03-root-selection"];

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

/// A mixed paragraph: text, repeated `b` scalars and repeated `i` groups with
/// an attribute and text.
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
        ],
    )
}

/// The primary and the `notes` and `Para` endpoints share this shape.
pub fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Title"),
            SchemaNode::group("Doc", vec![para_schema().repeating()]),
            SchemaNode::group(
                "Docs",
                vec![
                    SchemaNode::group(
                        "Doc",
                        vec![string("id").attribute(), para_schema().repeating()],
                    )
                    .repeating(),
                ],
            ),
        ],
    )
}

/// A constructed paragraph: text, renamed `b` and `i` values, and `extra`
/// ordinary fields.
fn paragraph_target(name: &str, extra: &[&str]) -> SchemaNode {
    let mut fields = vec![
        string(XML_TEXT_FIELD).text(),
        string("Strong").repeating(),
        string("Em").repeating(),
    ];
    fields.extend(extra.iter().map(|field| string(field)));
    SchemaNode::group(name, fields).repeating()
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

fn field(owner_name: Option<&str>, segments: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: owned_ref(owner_name, segments),
        frame: None,
    })
}

fn call(function: &str, args: &[NodeId]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}

pub fn owned_iteration(owner_name: Option<&str>, segments: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: owned_ref(owner_name, segments),
        output_path: None,
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

pub fn elements() -> Vec<XmlMixedContentElement> {
    vec![
        XmlMixedContentElement {
            source: "b".into(),
            target: "Strong".into(),
        },
        XmlMixedContentElement {
            source: "i".into(),
            target: "Em".into(),
        },
    ]
}

/// Nodes `base..base+3` read one owner's paragraph: `b` (upper-cased into
/// `base+1`), the `i` attribute `n` and the text. `Strong` keeps `b` values
/// other than `skip` (node `90`); `Em` keeps every `n`.
fn paragraph(
    target: &str,
    owner_name: Option<&str>,
    para: &[&str],
    base: NodeId,
    extra: &[(&str, NodeId)],
    nodes: &mut Vec<(NodeId, Node)>,
) -> Scope {
    let mut b = para.to_vec();
    b.push("b");
    let mut i = para.to_vec();
    i.push("i");
    let mut n = i.clone();
    n.push("n");
    let mut text = para.to_vec();
    text.push(XML_TEXT_FIELD);
    nodes.extend([
        (base, field(owner_name, &b)),
        (base + 1, call("upper", &[base])),
        (base + 2, field(owner_name, &n)),
        (base + 3, field(owner_name, &text)),
        (base + 4, call("not_equal", &[base, 90])),
    ]);
    let mut pairs = vec![(XML_TEXT_FIELD, base + 3)];
    pairs.extend(extra.iter().copied());
    Scope {
        target_field: target.into(),
        iteration: owned_iteration(owner_name, para),
        construction: ScopeConstruction::XmlMixedContent {
            elements: elements(),
        },
        bindings: bindings(&pairs),
        children: vec![
            Scope {
                target_field: "Strong".into(),
                iteration: owned_iteration(owner_name, &b),
                filter: Some(base + 4),
                construction: ScopeConstruction::Scalar { value: base + 1 },
                ..Scope::default()
            },
            Scope {
                target_field: "Em".into(),
                iteration: owned_iteration(owner_name, &i),
                construction: ScopeConstruction::Scalar { value: base + 2 },
                ..Scope::default()
            },
        ],
        ..Scope::default()
    }
}

fn named(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema,
        options: Default::default(),
        dynamic_path: None,
    }
}

fn base(target: SchemaNode, children: Vec<Scope>, nodes: Vec<(NodeId, Node)>) -> Project {
    let mut nodes = nodes;
    nodes.push((
        90,
        Node::Const {
            value: Value::String("skip".into()),
        },
    ));
    Project {
        source: input_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named("notes", input_schema()),
            named("Para", input_schema()),
        ],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope {
            children,
            ..Scope::default()
        },
    }
}

/// `P`, `Q` and the named target's `R` construct the primary, `notes` and
/// `Para` paragraphs of the same declared shape.
fn endpoints() -> Project {
    let para = ["Doc", "Para"];
    let mut nodes = Vec::new();
    let p = paragraph("P", None, &para, 10, &[], &mut nodes);
    let q = paragraph("Q", Some("notes"), &para, 20, &[], &mut nodes);
    let r = paragraph("R", Some("Para"), &para, 30, &[], &mut nodes);
    let mut project = base(
        SchemaNode::group(
            "Out",
            vec![paragraph_target("P", &[]), paragraph_target("Q", &[])],
        ),
        vec![p, q],
        nodes,
    );
    project.extra_targets = vec![NamedTarget {
        name: "aliased".into(),
        path: Some("aliased.json".into()),
        schema: SchemaNode::group("Aliased", vec![paragraph_target("R", &[])]),
        options: Default::default(),
        root: Scope {
            children: vec![r],
            ..Scope::default()
        },
    }];
    project
}

/// Rows of the primary `Docs/Doc`; each row constructs its own paragraphs and
/// a paragraph reads its row's `id` through the enclosing frame.
fn nested() -> Project {
    let mut nodes = vec![(40, field(None, &["Docs", "Doc", "id"]))];
    let p = paragraph(
        "P",
        None,
        &["Docs", "Doc", "Para"],
        50,
        &[("Doc", 40)],
        &mut nodes,
    );
    base(
        SchemaNode::group(
            "Out",
            vec![
                SchemaNode::group("Row", vec![string("Id"), paragraph_target("P", &["Doc"])])
                    .repeating(),
            ],
        ),
        vec![Scope {
            target_field: "Row".into(),
            iteration: owned_iteration(None, &["Docs", "Doc"]),
            bindings: bindings(&[("Id", 40)]),
            children: vec![p],
            ..Scope::default()
        }],
        nodes,
    )
}

/// `note` is a mixed paragraph at its endpoint root, selected inside each
/// primary paragraph, whose item is the innermost legacy frame there.
/// Its `Em` values read the `notes` title, demanded only when an `i` occurs.
fn root_selection() -> Project {
    let mut nodes = Vec::new();
    let mut outer = paragraph("P", None, &["Doc", "Para"], 60, &[], &mut nodes);
    let mut inner = paragraph("Note", Some("note"), &[], 70, &[], &mut nodes);
    nodes.push((79, field(Some("notes"), &["Title"])));
    if let ScopeConstruction::Scalar { value } = &mut inner.children[1].construction {
        *value = 79;
    }
    outer.children.push(inner);
    let mut p = paragraph_target("P", &[]);
    if let SchemaKind::Group { children, .. } = &mut p.kind {
        children.push(paragraph_target("Note", &[]));
    }
    let mut project = base(SchemaNode::group("Out", vec![p]), vec![outer], nodes);
    project.extra_sources.push(named("note", para_schema()));
    project
}

pub fn project(model: &str) -> Project {
    match model {
        "C01-endpoints" => endpoints(),
        "C02-nested" => nested(),
        "C03-root-selection" => root_selection(),
        _ => unreachable!("authored model"),
    }
}

/// The legacy control: the unchanged legacy-selected construction.
pub fn legacy() -> Project {
    let mut project = endpoints();
    project.extra_targets.clear();
    project.root.children.truncate(1);
    let paragraph = &mut project.root.children[0];
    paragraph.iteration = ScopeIteration::Source(path(&["Doc", "Para"]));
    paragraph.bindings.clear();
    paragraph.children = vec![Scope {
        target_field: "Strong".into(),
        iteration: ScopeIteration::Source(path(&["Doc", "Para", "b"])),
        construction: ScopeConstruction::Scalar { value: 91 },
        ..Scope::default()
    }];
    project.graph = Graph {
        nodes: [(
            91,
            Node::SourceField {
                path: vec![],
                frame: Some(path(&["Doc", "Para", "b"])),
            },
        )]
        .into(),
    };
    project.target = SchemaNode::group("Out", vec![paragraph_target("P", &[])]);
    project
}
