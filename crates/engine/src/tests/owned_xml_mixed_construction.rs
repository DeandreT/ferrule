//! Owned XML mixed-content construction: a scope's own owned selector supplies
//! the retained text/element order and mapped direct children, never another
//! endpoint or an enclosing frame.
use ir::{Instance, ScalarType, SchemaNode, XML_TEXT_FIELD};
use mapping::{
    Binding, Graph, NamedSource, Node, OwnedSourceField, OwnedSourceRef, Project, Scope,
    ScopeConstruction, ScopeIteration, SourceOwner, XmlMixedContentElement,
};

use crate::{EngineError, OwnedCapabilityContext, run_with_sources, validate};

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn para_schema() -> SchemaNode {
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

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Title"),
            SchemaNode::group("Doc", vec![para_schema().repeating()]),
        ],
    )
}

fn target_schema() -> SchemaNode {
    let p = |name: &str| {
        SchemaNode::group(
            name,
            vec![
                string(XML_TEXT_FIELD).text(),
                string("Strong").repeating(),
                string("Em").repeating(),
            ],
        )
        .repeating()
    };
    SchemaNode::group("Out", vec![p("P"), p("Q")])
}

fn xml(text: &str) -> Instance {
    format_xml::from_str(text, &source_schema()).expect("self-authored XML fixture parses")
}

fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}

fn owned_ref(owner_name: Option<&str>, path: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(owner_name),
        path: path.iter().map(|part| (*part).into()).collect(),
    }
}

fn field(owner_name: Option<&str>, path: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: owned_ref(owner_name, path),
        frame: None,
    })
}

fn owned_iteration(owner_name: Option<&str>, path: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: owned_ref(owner_name, path),
        output_path: None,
    }
}

/// One mapped paragraph: `b` becomes upper-cased `Strong`, `i` becomes `Em`
/// holding its `n` attribute; node ids start at `base`.
fn paragraph(
    target: &str,
    owner_name: Option<&str>,
    para: &[&str],
    base: u32,
    nodes: &mut Vec<(u32, Node)>,
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
        (
            base + 1,
            Node::Call {
                function: "upper".into(),
                args: vec![base],
            },
        ),
        (base + 2, field(owner_name, &n)),
        (base + 3, field(owner_name, &text)),
    ]);
    let child = |name: &str, path: &[&str], value| Scope {
        target_field: name.into(),
        iteration: owned_iteration(owner_name, path),
        construction: ScopeConstruction::Scalar { value },
        ..Scope::default()
    };
    Scope {
        target_field: target.into(),
        iteration: owned_iteration(owner_name, para),
        construction: ScopeConstruction::XmlMixedContent {
            elements: vec![
                XmlMixedContentElement {
                    source: "b".into(),
                    target: "Strong".into(),
                },
                XmlMixedContentElement {
                    source: "i".into(),
                    target: "Em".into(),
                },
            ],
        },
        bindings: vec![Binding {
            target_field: XML_TEXT_FIELD.into(),
            node: base + 3,
        }],
        children: vec![child("Strong", &b, base + 1), child("Em", &i, base + 2)],
        ..Scope::default()
    }
}

fn named(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.xml"),
        schema,
        options: Default::default(),
        dynamic_path: None,
    }
}

/// `P` maps the primary paragraphs and `Q` the `notes` paragraphs of the same
/// declared shape.
fn colliding() -> Project {
    let mut nodes = Vec::new();
    let p = paragraph("P", None, &["Doc", "Para"], 10, &mut nodes);
    let q = paragraph("Q", Some("notes"), &["Doc", "Para"], 20, &mut nodes);
    Project {
        source: source_schema(),
        target: target_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![named("notes", source_schema())],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope {
            children: vec![p, q],
            ..Scope::default()
        },
    }
}

fn write(project: &Project, output: &Instance) -> String {
    let options = format_xml::XmlWriteOptions {
        scalar_output: None,
        declaration: false,
        indent: false,
        default_namespace: None,
        schema_hints: None,
    };
    format_xml::to_string_with_options(&project.target, output, &options)
        .expect("constructed XML writes")
}

/// `(node name, text)` of every retained stream item of `group`.
fn stream(group: &Instance) -> Vec<(String, String)> {
    let text = |item: &Instance, name: &str| match item.field(name).and_then(Instance::as_scalar) {
        Some(ir::Value::String(value)) => value.clone(),
        other => format!("{other:?}"),
    };
    group
        .field(ir::XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
        .iter()
        .map(|item| {
            (
                text(item, ir::XML_NODE_NAME_FIELD),
                text(item, XML_TEXT_FIELD),
            )
        })
        .collect()
}

#[test]
fn colliding_endpoints_construct_only_their_own_text_order_and_children() {
    let project = colliding();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let primary = xml(
        "<Input><Doc><Para>One <b>x</b> two <i n=\"1\">it</i>.</Para><Para><b>y</b> end</Para></Doc></Input>",
    );
    let notes = xml("<Input><Doc><Para>Note <i n=\"7\">ni</i><b>z</b></Para></Doc></Input>");
    let output = run_with_sources(&project, &primary, vec![("notes".into(), notes)]).unwrap();
    assert_eq!(
        write(&project, &output),
        "<Out><P>One <Strong>X</Strong> two <Em>1</Em>.</P><P><Strong>Y</Strong> end</P><Q>Note <Em>7</Em><Strong>Z</Strong></Q></Out>"
    );
}

#[test]
fn an_endpoint_root_selection_never_attaches_the_enclosing_frame() {
    let mut project = colliding();
    // `note` is a mixed paragraph at its endpoint root, selected inside the
    // primary paragraph iteration whose item is the innermost legacy frame.
    project.extra_sources = vec![named("note", para_schema())];
    let mut nodes = Vec::new();
    let mut nested = paragraph("Q", Some("note"), &[], 20, &mut nodes);
    nested.target_field = "Note".into();
    project.graph.nodes.extend(nodes);
    project.root.children.truncate(1);
    project.root.children[0].children.push(nested);
    let note = |name: &str| {
        SchemaNode::group(
            name,
            vec![
                string(XML_TEXT_FIELD).text(),
                string("Strong").repeating(),
                string("Em").repeating(),
            ],
        )
        .repeating()
    };
    let mut p = note("P");
    if let ir::SchemaKind::Group { children, .. } = &mut p.kind {
        children.push(note("Note"));
    }
    project.target = SchemaNode::group("Out", vec![p]);
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let primary = xml("<Input><Doc><Para>p <b>x</b></Para></Doc></Input>");
    let note_input =
        format_xml::from_str("<Para>n <i n=\"5\">v</i></Para>", &para_schema()).unwrap();
    let output = run_with_sources(&project, &primary, vec![("note".into(), note_input)]).unwrap();
    let paragraph = &output.field("P").and_then(Instance::as_repeated).unwrap()[0];
    let pair = |name: &str, text: &str| (name.to_owned(), text.to_owned());
    assert_eq!(stream(paragraph), [pair("", "p "), pair("Strong", "X")]);
    let note = &paragraph
        .field("Note")
        .and_then(Instance::as_repeated)
        .unwrap()[0];
    assert_eq!(stream(note), [pair("", "n "), pair("Em", "5")]);
}

#[test]
fn grouped_and_ancestry_inherited_mixed_constructions_stay_closed() {
    let primary = xml("<Input><Doc><Para>a</Para></Doc></Input>");
    let notes = xml("<Input/>");
    // Grouping an owned selector does not qualify the construction.
    let mut grouped = colliding();
    grouped.graph.nodes.insert(
        90,
        Node::Const {
            value: ir::Value::String("k".into()),
        },
    );
    grouped.root.children[0].group_by = Some(90);
    assert!(matches!(
        run_with_sources(&grouped, &primary, vec![("notes".into(), notes.clone())]),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::XmlOperator,
            ..
        })
    ));
    // A construction without its own selector inside an owned iteration would
    // attach the enclosing item; it stays closed.
    let mut inherited = colliding();
    inherited.root.children.truncate(1);
    let paragraph = &mut inherited.root.children[0];
    paragraph.construction = ScopeConstruction::Constructed;
    paragraph.bindings.clear();
    paragraph.children = vec![Scope {
        target_field: "Inner".into(),
        construction: ScopeConstruction::XmlMixedContent {
            elements: vec![XmlMixedContentElement {
                source: "b".into(),
                target: "Strong".into(),
            }],
        },
        ..Scope::default()
    }];
    inherited.target = SchemaNode::group(
        "Out",
        vec![
            SchemaNode::group(
                "P",
                vec![SchemaNode::group(
                    "Inner",
                    vec![string(XML_TEXT_FIELD).text(), string("Strong").repeating()],
                )],
            )
            .repeating(),
        ],
    );
    assert!(
        validate(&inherited).is_empty(),
        "{:?}",
        validate(&inherited)
    );
    assert!(matches!(
        run_with_sources(&inherited, &primary, vec![("notes".into(), notes)]),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::XmlOperator,
            ..
        })
    ));
}
