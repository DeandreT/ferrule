//! Owned XML mixed-content atomization: the group belongs to one exact
//! endpoint and frame, and occurrence replacements evaluate in owned
//! occurrences of that same endpoint.
use ir::{Instance, ScalarType, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph, NamedSource, Node,
    OwnedPosition, OwnedSourceField, OwnedSourceRef, OwnedXmlMixedContent,
    OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement, Project, Scope, ScopeIteration,
    SequenceExpr, SourceOwner, UserFunction, XmlMixedContentReplacement,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError, run_with_sources, validate};

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
            string("u"),
        ],
    )
}

/// Every endpoint shares this shape, so only the owner distinguishes values.
fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Title"),
            SchemaNode::group("Doc", vec![para_schema()]),
            SchemaNode::group(
                "Docs",
                vec![
                    SchemaNode::group("Doc", vec![string("id").attribute(), para_schema()])
                        .repeating(),
                ],
            ),
            SchemaNode::group("Plain", vec![string("Code")]),
        ],
    )
}

fn xml(text: &str) -> Instance {
    format_xml::from_str(text, &source_schema()).expect("self-authored XML fixture parses")
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
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

fn position(owner_name: Option<&str>, collection: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(owner_name),
        collection: Some(path(collection)),
    })
}

fn replacement(
    element: &str,
    context: OwnedXmlMixedContentContext,
    expression: u32,
) -> OwnedXmlMixedContentReplacement {
    OwnedXmlMixedContentReplacement {
        element: element.into(),
        context,
        expression,
    }
}

const OCCURRENCE: OwnedXmlMixedContentContext = OwnedXmlMixedContentContext::Occurrence;
const PARENT: OwnedXmlMixedContentContext = OwnedXmlMixedContentContext::Parent;

fn mixed(
    owner_name: Option<&str>,
    segments: &[&str],
    frame: Option<&[&str]>,
    replacements: Vec<OwnedXmlMixedContentReplacement>,
) -> Node {
    Node::OwnedXmlMixedContent(OwnedXmlMixedContent {
        source: owned_ref(owner_name, segments),
        frame: frame.map(path),
        replacements,
    })
}

fn concat(args: Vec<u32>) -> Node {
    Node::Call {
        function: "concat".into(),
        args,
    }
}

fn constant(value: Value) -> Node {
    Node::Const { value }
}

fn named(name: &str) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.xml"),
        schema: source_schema(),
        options: Default::default(),
        dynamic_path: None,
    }
}

fn bindings(pairs: &[(&str, u32)]) -> Vec<Binding> {
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

/// `notes` and `Doc` are named endpoints; `Doc` is also spelled like the
/// primary root field holding the mixed group.
fn project(target: SchemaNode, root: Scope, nodes: Vec<(u32, Node)>) -> Project {
    Project {
        source: source_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![named("notes"), named("Doc")],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn output(fields: &[(&str, &str)]) -> Instance {
    Instance::Group(
        fields
            .iter()
            .map(|(name, value)| ((*name).into(), text(value)))
            .collect::<Vec<_>>()
            .into(),
    )
}

/// The primary and `notes` paragraphs, each with its own replacements: `b`
/// occurrences number and read themselves, `i` reads its owner's title in
/// the parent context.
fn endpoints() -> Project {
    let para = ["Doc", "Para"];
    let b = ["Doc", "Para", "b"];
    project(
        strings("Output", &["Primary", "Notes"]),
        Scope {
            bindings: bindings(&[("Primary", 1), ("Notes", 2)]),
            ..Scope::default()
        },
        vec![
            (
                1,
                mixed(
                    None,
                    &para,
                    None,
                    vec![
                        replacement("b", OCCURRENCE, 10),
                        replacement("i", PARENT, 15),
                    ],
                ),
            ),
            (
                2,
                mixed(
                    Some("notes"),
                    &para,
                    None,
                    vec![
                        replacement("b", OCCURRENCE, 20),
                        replacement("i", PARENT, 25),
                    ],
                ),
            ),
            (10, concat(vec![11, 12, 13, 14, 16])),
            (11, constant(Value::String("[".into()))),
            (12, position(None, &b)),
            (13, constant(Value::String(":".into()))),
            (14, field(None, &b)),
            (16, constant(Value::String("]".into()))),
            (15, field(None, &["Title"])),
            (20, concat(vec![11, 21, 13, 22, 16])),
            (21, position(Some("notes"), &b)),
            (22, field(Some("notes"), &b)),
            (25, field(Some("notes"), &["Title"])),
        ],
    )
}

const PRIMARY: &str = "<Input><Title>P</Title><Doc><Para>One <b>x</b> two <i n=\"1\">it</i> <b>y</b>.</Para></Doc></Input>";
const NOTES: &str =
    "<Input><Title>N</Title><Doc><Para>Note <i n=\"2\">ni</i><b>z</b>  end</Para></Doc></Input>";

#[test]
fn colliding_endpoints_atomize_only_their_own_group_and_occurrences() {
    let project = endpoints();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let outcome = run_with_sources(
        &project,
        &xml(PRIMARY),
        vec![("notes".into(), xml(NOTES)), ("Doc".into(), xml(NOTES))],
    )
    .unwrap();
    assert_eq!(
        outcome,
        output(&[
            ("Primary", "One [1:x] two P [2:y]."),
            ("Notes", "Note N[1:z]  end"),
        ])
    );
}

#[test]
fn a_missing_selected_group_never_borrows_another_endpoint() {
    let project = endpoints();
    let primary = xml("<Input><Title>P</Title><Plain><Code>c</Code></Plain></Input>");
    // `Doc` is a named endpoint spelled like the absent primary field.
    let error = run_with_sources(
        &project,
        &primary,
        vec![("notes".into(), xml(NOTES)), ("Doc".into(), xml(NOTES))],
    )
    .unwrap_err();
    assert_eq!(
        error,
        EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::MissingOwnedElement {
                owner: SourceOwner::Primary,
                path: path(&["Doc", "Para"]),
            },
        }
    );
    // An unsupplied owner is a typed failure of the reading node.
    let error = run_with_sources(&project, &xml(PRIMARY), vec![]).unwrap_err();
    assert_eq!(
        error,
        EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: OwnedSourceError::SourceUnavailable {
                owner: owner(Some("notes")),
            },
        }
    );
}

#[test]
fn occurrence_reads_of_another_owner_cross_its_uniterated_repetition() {
    let mut project = endpoints();
    // The primary occurrence frame never serves a `notes` read.
    project
        .graph
        .nodes
        .insert(14, field(Some("notes"), &["Doc", "Para", "b"]));
    let error =
        run_with_sources(&project, &xml(PRIMARY), vec![("notes".into(), xml(NOTES))]).unwrap_err();
    assert_eq!(
        error,
        EngineError::OwnedSourceAccess {
            node: Some(14),
            scope: None,
            source: OwnedSourceError::UniteratedOwnedRepetition {
                path: path(&["Doc", "Para", "b"]),
            },
        }
    );
    // A parent replacement has no occurrence frame of its own owner either.
    let mut project = endpoints();
    project
        .graph
        .nodes
        .insert(15, field(None, &["Doc", "Para", "b"]));
    let error =
        run_with_sources(&project, &xml(PRIMARY), vec![("notes".into(), xml(NOTES))]).unwrap_err();
    assert!(matches!(
        error,
        EngineError::OwnedSourceAccess {
            node: Some(15),
            source: OwnedSourceError::UniteratedOwnedRepetition { .. },
            ..
        }
    ));
}

#[test]
fn replacement_values_are_lexical_null_is_omitted_and_unmatched_rules_stay_lazy() {
    let para = ["Doc", "Para"];
    let project = project(
        strings("Output", &["Primary"]),
        Scope {
            bindings: bindings(&[("Primary", 1)]),
            ..Scope::default()
        },
        vec![
            (
                1,
                mixed(
                    None,
                    &para,
                    None,
                    vec![
                        replacement("b", PARENT, 2),
                        replacement("i", OCCURRENCE, 3),
                        // `u` never occurs, so its unsupplied owner is never read.
                        replacement("u", PARENT, 4),
                    ],
                ),
            ),
            (2, constant(Value::Null)),
            (3, field(None, &["Doc", "Para", "i", "n"])),
            (4, field(Some("notes"), &["Title"])),
        ],
    );
    assert!(validate(&project).is_empty());
    let outcome = run_with_sources(&project, &xml(PRIMARY), vec![]).unwrap();
    assert_eq!(outcome, output(&[("Primary", "One  two 1 .")]));
    let mut ints = project.clone();
    ints.graph.nodes.insert(2, constant(Value::Int(7)));
    ints.graph.nodes.insert(3, constant(Value::Bool(true)));
    assert_eq!(
        run_with_sources(&ints, &xml(PRIMARY), vec![]).unwrap(),
        output(&[("Primary", "One 7 two true 7.")])
    );
    // Text-only content has no retained stream and returns the text value.
    let plain = xml("<Input><Doc><Para>  only text  </Para></Doc></Input>");
    assert_eq!(
        run_with_sources(&project, &plain, vec![]).unwrap(),
        output(&[("Primary", "  only text  ")])
    );
}

#[test]
fn nested_rows_select_their_own_group_and_keep_outer_frames() {
    let row = ["Docs", "Doc"];
    let para = ["Docs", "Doc", "Para"];
    let b = ["Docs", "Doc", "Para", "b"];
    let project = project(
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
                bindings: bindings(&[("Pinned", 1), ("Free", 2)]),
                ..Scope::default()
            }],
            ..Scope::default()
        },
        vec![
            (
                1,
                mixed(
                    None,
                    &para,
                    Some(&row),
                    vec![replacement("b", OCCURRENCE, 10)],
                ),
            ),
            (
                2,
                mixed(None, &para, None, vec![replacement("b", PARENT, 15)]),
            ),
            // Row position, occurrence position and value, row id.
            (10, concat(vec![11, 12, 13, 14])),
            (11, position(None, &row)),
            (12, position(None, &b)),
            (13, field(None, &b)),
            (14, field(None, &["Docs", "Doc", "id"])),
            (15, field(None, &["Docs", "Doc", "id"])),
        ],
    );
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let source = xml(
        "<Input><Docs><Doc id=\"a\"><Para>A<b>p</b>-<b>q</b></Para></Doc><Doc id=\"b\"><Para><b>r</b>B</Para></Doc></Docs></Input>",
    );
    let rows = |pairs: &[(&str, &str)]| {
        Instance::Group(
            vec![(
                "Row".to_string(),
                Instance::Repeated(
                    pairs
                        .iter()
                        .map(|(pinned, free)| output(&[("Pinned", pinned), ("Free", free)]))
                        .collect(),
                ),
            )]
            .into(),
        )
    };
    assert_eq!(
        run_with_sources(&project, &source, vec![]).unwrap(),
        rows(&[("A11pa-12qa", "Aa-a"), ("21rbB", "bB")])
    );
    // The second row's absent group never borrows the first row's.
    let partial = xml(
        "<Input><Docs><Doc id=\"a\"><Para>A<b>p</b></Para></Doc><Doc id=\"b\"/></Docs></Input>",
    );
    assert_eq!(
        run_with_sources(&project, &partial, vec![]).unwrap_err(),
        EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::MissingOwnedElement {
                owner: SourceOwner::Primary,
                path: path(&para),
            },
        }
    );
    // Outside the row, an unpinned read crosses the uniterated repetition and
    // a pinned read has no active frame.
    let mut outside = project.clone();
    outside.root.bindings = outside.root.children[0].bindings.clone();
    outside.root.children.clear();
    outside.target = strings("Output", &["Pinned", "Free"]);
    assert!(matches!(
        run_with_sources(&outside, &source, vec![]).unwrap_err(),
        EngineError::OwnedSourceAccess {
            node: Some(1),
            source: OwnedSourceError::InactiveSourceFrame { .. },
            ..
        }
    ));
    outside.root.bindings.remove(0);
    assert!(matches!(
        run_with_sources(&outside, &source, vec![]).unwrap_err(),
        EngineError::OwnedSourceAccess {
            node: Some(2),
            source: OwnedSourceError::UniteratedOwnedRepetition { .. },
            ..
        }
    ));
}

fn issues(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

#[test]
fn validation_checks_the_exact_owner_schema_and_replacement_rules() {
    let check = |node: Node, expected: &str| {
        let mut project = endpoints();
        project.graph.nodes.insert(1, node);
        let messages = issues(&project);
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{messages:?} lacks {expected}"
        );
    };
    let para = ["Doc", "Para"];
    check(
        mixed(None, &["Plain"], None, vec![]),
        "owned XML mixed-content source `Plain` matches no mixed group in its owner",
    );
    check(
        mixed(Some("missing"), &para, None, vec![]),
        "unknown source owner `missing`",
    );
    check(
        mixed(None, &para, Some(&["Docs", "Doc"]), vec![]),
        "owned frame `Docs/Doc` does not prefix mixed group `Doc/Para`",
    );
    check(
        mixed(None, &para, None, vec![replacement("u", OCCURRENCE, 15)]),
        "owned XML mixed-content occurrence `Doc/Para/u` is not a repeating child element in its owner",
    );
    check(
        mixed(
            None,
            &para,
            None,
            vec![
                replacement("b", PARENT, 15),
                replacement("b", OCCURRENCE, 15),
            ],
        ),
        "XML mixed-content element `b` has more than one replacement",
    );
    check(
        mixed(None, &para, None, vec![replacement("", PARENT, 15)]),
        "XML mixed-content replacement element cannot be empty",
    );
    check(
        mixed(None, &para, None, vec![replacement("b", PARENT, 99)]),
        "replacement for `b` references missing node 99",
    );
    // A parent-context rule may name an undeclared element, like the legacy node.
    let mut undeclared = endpoints();
    undeclared.graph.nodes.insert(
        1,
        mixed(None, &para, None, vec![replacement("em", PARENT, 15)]),
    );
    assert!(issues(&undeclared).is_empty());
}

#[test]
fn closed_contexts_refuse_the_owned_atomizer() {
    let source = xml(PRIMARY);
    // A failure-rule predicate is not a target context.
    let mut failure = endpoints();
    failure.graph.nodes.insert(
        30,
        Node::Call {
            function: "exists".into(),
            args: vec![1],
        },
    );
    failure.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::WhenTrue { predicate: 30 },
        message: None,
    }];
    assert!(matches!(
        run_with_sources(&failure, &source, vec![]).unwrap_err(),
        EngineError::UnsupportedOwnedContext {
            node: Some(1),
            context: OwnedCapabilityContext::FailureRule,
            ..
        }
    ));
    // A generated item scope closes the owned read.
    let mut generated = endpoints();
    generated.target = SchemaNode::group("Output", vec![strings("Item", &["Primary"]).repeating()]);
    generated.root = Scope {
        children: vec![Scope {
            target_field: "Item".into(),
            iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                input: 31,
                delimiter: 32,
                item: 33,
            }),
            bindings: bindings(&[("Primary", 1)]),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    generated.graph.nodes.extend([
        (31, constant(Value::String("a,b".into()))),
        (32, constant(Value::String(",".into()))),
        (
            33,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        ),
    ]);
    assert!(matches!(
        run_with_sources(&generated, &source, vec![]).unwrap_err(),
        EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::GeneratedItem,
            ..
        }
    ));
    // An isolated function body never reads an endpoint.
    let mut function = endpoints();
    let id = FunctionId::new(1);
    function.user_functions.insert(
        id,
        UserFunction {
            library: "user".into(),
            name: "mixed".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(5, mixed(None, &["Doc", "Para"], None, vec![]))].into(),
            },
            output: 5,
        },
    );
    function.graph.nodes.insert(
        2,
        Node::UserFunctionCall {
            function: id,
            args: vec![],
        },
    );
    assert!(matches!(
        run_with_sources(&function, &source, vec![]).unwrap_err(),
        EngineError::UnsupportedSourceOwnership {
            node: 5,
            function: Some(_),
            kind: "owned_xml_mixed_content",
        }
    ));
    // Owned reads below a legacy mixed-content node remain an XML operator.
    let mut legacy = endpoints();
    legacy.graph.nodes.insert(
        1,
        Node::XmlMixedContent {
            path: path(&["Doc", "Para"]),
            frame: None,
            replacements: vec![XmlMixedContentReplacement {
                element: "i".into(),
                collection: vec![],
                expression: 15,
            }],
        },
    );
    assert!(matches!(
        run_with_sources(&legacy, &source, vec![("notes".into(), xml(NOTES))]).unwrap_err(),
        EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::XmlOperator,
            ..
        }
    ));
}

#[test]
fn legacy_mixed_content_is_unchanged() {
    let mut project = endpoints();
    project.graph.nodes.insert(
        1,
        Node::XmlMixedContent {
            path: path(&["Doc", "Para"]),
            frame: None,
            replacements: vec![XmlMixedContentReplacement {
                element: "b".into(),
                collection: path(&["Doc", "Para", "b"]),
                expression: 40,
            }],
        },
    );
    project.graph.nodes.insert(
        40,
        Node::SourceField {
            path: vec![],
            frame: Some(path(&["Doc", "Para", "b"])),
        },
    );
    project.graph.nodes.remove(&2);
    project.root.bindings.truncate(1);
    assert!(validate(&project).is_empty());
    // Unmatched elements keep their text and the legacy occurrence frame
    // still serves frame-pinned ordinary reads.
    let outcome = run_with_sources(&project, &xml(PRIMARY), vec![]);
    assert_eq!(outcome.unwrap(), output(&[("Primary", "One x two it y.")]));
    // A missing legacy group keeps its untyped legacy error.
    let error = run_with_sources(&project, &xml("<Input/>"), vec![]).unwrap_err();
    assert_eq!(error, EngineError::MissingSourceField("Doc/Para".into()));
}
