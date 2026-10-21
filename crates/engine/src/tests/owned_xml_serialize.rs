//! Owned XML serialization: the structured element belongs to one exact
//! endpoint and frame, while the schema and writer keep the legacy behavior.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph, NamedSource, Node,
    OwnedSourceRef, OwnedXmlSerialize, Project, Scope, ScopeIteration, SequenceExpr, SourceOwner,
    UserFunction,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError, run_with_sources, validate};

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
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
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn header_schema() -> SchemaNode {
    SchemaNode::group("Header", vec![string("id").attribute(), string("Note")])
}
fn detail_schema() -> SchemaNode {
    SchemaNode::group("Detail", vec![string("code").attribute()])
}
fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            header_schema(),
            SchemaNode::group("Rows", vec![string("Label"), detail_schema()]).repeating(),
        ],
    )
}
fn header(id: &str) -> Instance {
    group(vec![("Header", group(vec![("id", text(id))]))])
}
fn rows(details: &[Option<&str>]) -> Instance {
    Instance::Repeated(
        details
            .iter()
            .map(|code| match code {
                Some(code) => group(vec![("Detail", group(vec![("code", text(code))]))]),
                None => group(vec![("Label", text("no detail"))]),
            })
            .collect(),
    )
}
fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}
fn serializer(
    owner_name: Option<&str>,
    path: &[&str],
    frame: Option<&[&str]>,
    schema: SchemaNode,
) -> Node {
    Node::OwnedXmlSerialize(OwnedXmlSerialize {
        source: OwnedSourceRef {
            owner: owner(owner_name),
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: frame.map(|frame| frame.iter().map(|part| (*part).into()).collect()),
        schema: Box::new(schema),
        declaration: false,
        indent: false,
        namespace: None,
    })
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
/// `Header` is a named endpoint spelled like the primary root field.
fn project(target: SchemaNode, root: Scope, nodes: Vec<(u32, Node)>) -> Project {
    Project {
        source: source_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Header".into(),
            path: "header.json".into(),
            schema: source_schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root,
    }
}
fn root(pairs: &[(&str, u32)]) -> Scope {
    Scope {
        bindings: bindings(pairs),
        ..Scope::default()
    }
}
fn missing(node: u32, owner: SourceOwner, path: &[&str]) -> EngineError {
    EngineError::OwnedSourceAccess {
        node: Some(node),
        scope: None,
        source: OwnedSourceError::MissingOwnedElement {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
    }
}
fn output(fields: Vec<(&str, &str)>) -> Instance {
    group(
        fields
            .into_iter()
            .map(|(name, xml)| (name, text(xml)))
            .collect(),
    )
}

#[test]
fn colliding_endpoints_serialize_only_their_exact_owner() {
    let project = project(
        strings("Output", &["Primary", "Alias"]),
        root(&[("Primary", 1), ("Alias", 2)]),
        vec![
            (1, serializer(None, &["Header"], None, header_schema())),
            (
                2,
                serializer(Some("Header"), &["Header"], None, header_schema()),
            ),
        ],
    );
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    assert_eq!(
        run_with_sources(&project, &header("p"), vec![("Header".into(), header("h"))]),
        Ok(output(vec![
            ("Primary", r#"<Header id="p"></Header>"#),
            ("Alias", r#"<Header id="h"></Header>"#),
        ]))
    );
    // A missing primary element is never repaired by the named endpoint.
    assert_eq!(
        run_with_sources(
            &project,
            &group(vec![]),
            vec![("Header".into(), header("h"))]
        ),
        Err(missing(1, SourceOwner::Primary, &["Header"]))
    );
    assert_eq!(
        run_with_sources(&project, &header("p"), vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: OwnedSourceError::SourceUnavailable {
                owner: owner(Some("Header")),
            },
        })
    );
}

#[test]
fn active_frames_select_the_current_item_and_explicit_root_pins() {
    let project = project(
        SchemaNode::group(
            "Output",
            vec![strings("Row", &["Pinned", "Unpinned", "Root"]).repeating()],
        ),
        Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: vec!["Rows".into()],
                    },
                    output_path: None,
                },
                bindings: bindings(&[("Pinned", 1), ("Unpinned", 2), ("Root", 3)]),
                ..Scope::default()
            }],
            ..Scope::default()
        },
        vec![
            (
                1,
                serializer(None, &["Rows", "Detail"], Some(&["Rows"]), detail_schema()),
            ),
            (
                2,
                serializer(None, &["Rows", "Detail"], None, detail_schema()),
            ),
            (3, serializer(None, &["Header"], Some(&[]), header_schema())),
        ],
    );
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let source = |details: &[Option<&str>]| match header("p") {
        Instance::Group(mut fields) => {
            fields.push(("Rows".into(), rows(details)));
            Instance::Group(fields)
        }
        _ => unreachable!("authored group"),
    };
    // An attribute-only element keeps the writer's self-closing form.
    let row = |code: &str| {
        output(vec![
            ("Pinned", &format!(r#"<Detail code="{code}"/>"#)),
            ("Unpinned", &format!(r#"<Detail code="{code}"/>"#)),
            ("Root", r#"<Header id="p"></Header>"#),
        ])
    };
    assert_eq!(
        run_with_sources(&project, &source(&[Some("a"), Some("b")]), vec![]),
        Ok(group(vec![(
            "Row",
            Instance::Repeated(vec![row("a"), row("b")])
        )]))
    );
    // The second row's absent element never borrows the first row's.
    assert_eq!(
        run_with_sources(&project, &source(&[Some("a"), None]), vec![]),
        Err(missing(1, SourceOwner::Primary, &["Rows", "Detail"]))
    );
}

#[test]
fn inactive_frames_and_uniterated_repetition_are_typed_failures() {
    let input = group(vec![("Rows", rows(&[Some("a")]))]);
    let run = |node: Node| {
        let project = project(
            strings("Output", &["Xml"]),
            root(&[("Xml", 1)]),
            vec![(1, node)],
        );
        assert!(validate(&project).is_empty(), "{:?}", validate(&project));
        run_with_sources(&project, &input, vec![])
    };
    assert_eq!(
        run(serializer(
            None,
            &["Rows", "Detail"],
            Some(&["Rows"]),
            detail_schema()
        )),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(vec!["Rows".into()]),
            },
        })
    );
    // Unlike the legacy reader, the first repetition is never chosen implicitly.
    assert_eq!(
        run(serializer(None, &["Rows", "Detail"], None, detail_schema())),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: OwnedSourceError::UniteratedOwnedRepetition {
                path: vec!["Rows".into(), "Detail".into()],
            },
        })
    );
}

#[test]
fn writer_failures_and_laziness_keep_the_legacy_serializer_behavior() {
    let project = |condition: bool| {
        project(
            strings("Output", &["Xml"]),
            root(&[("Xml", 4)]),
            vec![
                (1, serializer(None, &["Header"], None, header_schema())),
                (
                    2,
                    Node::Const {
                        value: Value::Bool(condition),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("skipped".into()),
                    },
                ),
                (
                    4,
                    Node::If {
                        condition: 2,
                        then: 1,
                        else_: 3,
                    },
                ),
            ],
        )
    };
    // An unselected branch never reads or writes the element.
    assert_eq!(
        run_with_sources(&project(false), &group(vec![]), vec![]),
        Ok(output(vec![("Xml", "skipped")]))
    );
    let invalid = group(vec![(
        "Header",
        group(vec![("id", text("p")), ("Note", text("bell\u{1}"))]),
    )]);
    assert!(matches!(
        run_with_sources(&project(true), &invalid, vec![]),
        Err(EngineError::XmlSerialization { node: 1, message })
            if message.contains("forbidden XML 1.0 character U+0001")
    ));
    let scalar_element = group(vec![("Header", text("flat"))]);
    assert!(matches!(
        run_with_sources(&project(true), &scalar_element, vec![]),
        Err(EngineError::XmlSerialization { node: 1, .. })
    ));
}

#[test]
fn validation_checks_the_exact_owner_schema_and_document_options() {
    let issues = |node: Node| {
        let project = project(
            strings("Output", &["Xml"]),
            root(&[("Xml", 1)]),
            vec![(1, node)],
        );
        validate(&project)
            .into_iter()
            .map(|issue| issue.message)
            .collect::<Vec<_>>()
    };
    let with = |change: &dyn Fn(&mut OwnedXmlSerialize)| {
        let Node::OwnedXmlSerialize(mut serializer) =
            serializer(None, &["Header"], None, header_schema())
        else {
            unreachable!("authored owned serializer");
        };
        change(&mut serializer);
        Node::OwnedXmlSerialize(serializer)
    };
    assert!(issues(with(&|_| {})).is_empty());
    for (change, expected) in [
        (
            with(&|serializer| *serializer.schema = detail_schema()),
            "owned XML serializer source `Header` does not match its `Detail` schema in its owner",
        ),
        (
            with(&|serializer| *serializer.schema = header_schema().repeating()),
            "XML serializer schema must describe one document element",
        ),
        (
            with(&|serializer| serializer.namespace = Some(String::new())),
            "XML serializer namespace cannot be empty",
        ),
        (
            with(&|serializer| serializer.source.owner = owner(Some("missing"))),
            "unknown source owner `missing`",
        ),
        (
            with(&|serializer| serializer.frame = Some(vec!["Header".into()])),
            "owned collection `Header` is not a repeating path in its owner",
        ),
    ] {
        let found = issues(change);
        assert!(
            found.iter().any(|message| message == expected),
            "{expected}: {found:?}"
        );
    }
}

#[test]
fn functions_failure_rows_and_generated_items_refuse_the_owned_serializer() {
    let input = group(vec![
        ("Header", group(vec![])),
        ("Rows", rows(&[Some("a")])),
    ]);
    // An isolated function body never reads an endpoint.
    let mut function = project(strings("Output", &["Xml"]), root(&[("Xml", 1)]), vec![]);
    let id = FunctionId::new(1);
    function.user_functions.insert(
        id,
        UserFunction {
            library: "user".into(),
            name: "serialize".into(),
            description: None,
            parameters: vec![],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(5, serializer(None, &["Header"], None, header_schema()))].into(),
            },
            output: 5,
        },
    );
    function.graph.nodes.insert(
        1,
        Node::UserFunctionCall {
            function: id,
            args: vec![],
        },
    );
    assert_eq!(
        run_with_sources(&function, &input, vec![]),
        Err(EngineError::UnsupportedSourceOwnership {
            node: 5,
            function: Some(id),
            kind: "owned_xml_serialize",
        })
    );

    // An owned failure walk's message stays within its scalar subset.
    let mut failure = project(
        strings("Output", &[]),
        Scope::default(),
        vec![
            (1, serializer(None, &["Header"], None, header_schema())),
            (
                2,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
        ],
    );
    failure.failure_rules = vec![FailureRule {
        iteration: FailureIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Rows".into()],
            },
        },
        selection: FailureSelection::WhenTrue { predicate: 2 },
        message: Some(1),
    }];
    assert_eq!(
        run_with_sources(&failure, &input, vec![]),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(1),
            function: None,
            scope: None,
            context: OwnedCapabilityContext::FailureRule,
        })
    );

    // A generated item context closes the owned read inside it.
    let generated = project(
        SchemaNode::group("Output", vec![strings("Item", &["Xml"]).repeating()]),
        Scope {
            children: vec![Scope {
                target_field: "Item".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
                    input: 2,
                    delimiter: 3,
                    item: 4,
                }),
                bindings: bindings(&[("Xml", 1)]),
                ..Scope::default()
            }],
            ..Scope::default()
        },
        vec![
            (1, serializer(None, &["Header"], None, header_schema())),
            (
                2,
                Node::Const {
                    value: Value::String("a,b".into()),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String(",".into()),
                },
            ),
            (
                4,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            ),
        ],
    );
    assert!(matches!(
        run_with_sources(&generated, &input, vec![]),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::GeneratedItem,
            ..
        })
    ));
}
