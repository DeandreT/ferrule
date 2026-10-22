//! #467 runtime-named property reads from one explicitly owned open object.
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph, NamedSource, Node,
    OwnedDynamicSourceField, OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeIteration,
    SequenceExpr, SourceOwner, UserFunction,
};

use crate::{EngineError, ExecutionContext, OwnedCapabilityContext, OwnedSourceError};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owner(name: &str) -> SourceOwner {
    match name {
        "Primary" => SourceOwner::Primary,
        name => SourceOwner::Named { name: name.into() },
    }
}
fn dynamic(name: &str, object: &[&str], frame: Option<&[&str]>, key: u32) -> Node {
    Node::OwnedDynamicSourceField(OwnedDynamicSourceField {
        object: OwnedSourceRef {
            owner: owner(name),
            path: path(object),
        },
        frame: frame.map(path),
        key,
    })
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn null() -> Instance {
    Instance::Scalar(Value::Null)
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
fn open(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Id", ScalarType::String)])
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .expect("open object schema")
}
/// Primary and Lookup share every object and property name.
fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            open("Props"),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Id", ScalarType::String), open("Props")],
            )
            .repeating(),
        ],
    )
}
fn endpoint(props: Vec<(&str, Instance)>, rows: Vec<Instance>) -> Instance {
    group(vec![
        ("Key", text("color")),
        ("Props", group(props)),
        ("Rows", Instance::Repeated(rows)),
    ])
}
fn row(id: &str, props: Vec<(&str, Instance)>) -> Instance {
    group(vec![("Id", text(id)), ("Props", group(props))])
}
fn primary() -> Instance {
    endpoint(
        vec![
            ("Id", text("P.id")),
            ("color", text("P.red")),
            ("blank", Instance::Scalar(Value::json_null())),
            ("nested", group(vec![("x", text("structural"))])),
        ],
        vec![
            row("P1", vec![("color", text("P.row1"))]),
            row("P2", vec![("size", text("P.row2"))]),
        ],
    )
}
fn lookup() -> Instance {
    endpoint(
        vec![("size", text("N.small"))],
        vec![row("N1", vec![("color", text("N.row1"))])],
    )
}

const KEY_COLOR: u32 = 1;
const KEY_SIZE: u32 = 2;
const KEY_INT: u32 = 3;
const KEY_BLANK: u32 = 4;
const KEY_NESTED: u32 = 5;
const KEY_ID: u32 = 6;

/// Bindings `V1..` read the given nodes in order.
fn project(values: Vec<Node>, extra: Vec<(u32, Node)>) -> Project {
    let mut graph = Graph::default();
    for (id, key) in [
        (KEY_COLOR, Value::String("color".into())),
        (KEY_SIZE, Value::String("size".into())),
        (KEY_INT, Value::Int(7)),
        (KEY_BLANK, Value::String("blank".into())),
        (KEY_NESTED, Value::String("nested".into())),
        (KEY_ID, Value::String("Id".into())),
    ] {
        graph.nodes.insert(id, Node::Const { value: key });
    }
    let mut bindings = Vec::new();
    let mut fields = Vec::new();
    for (index, node) in values.into_iter().enumerate() {
        let id = 10 + index as u32;
        graph.nodes.insert(id, node);
        let name = format!("V{}", index + 1);
        fields.push(SchemaNode::scalar(&name, ScalarType::String));
        bindings.push(Binding {
            target_field: name,
            node: id,
        });
    }
    graph.nodes.extend(extra);
    Project {
        source: endpoint_schema("Input"),
        target: SchemaNode::group("Out", fields),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Lookup".into(),
                path: "lookup.json".into(),
                schema: endpoint_schema("Lookup"),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Settings".into(),
                path: "settings.json".into(),
                schema: open("Settings"),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings,
            ..Scope::default()
        },
    }
}
fn run(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
) -> Result<Instance, EngineError> {
    let execution = ExecutionContext::new(std::path::Path::new("owned467.mapping"));
    crate::run_outputs_with_sources_and_context(project, source, extras, &execution)
        .map(|outputs| outputs.primary)
}
fn inputs(lookup: Option<Instance>) -> Vec<(String, Instance)> {
    let mut inputs = vec![("Settings".to_owned(), group(vec![("mode", text("S.fast"))]))];
    inputs.extend(lookup.map(|lookup| ("Lookup".to_owned(), lookup)));
    inputs
}
fn out(values: Vec<Instance>) -> Instance {
    Instance::Group(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (format!("V{}", index + 1), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn access(node: u32, source: OwnedSourceError) -> EngineError {
    EngineError::OwnedSourceAccess {
        node: Some(node),
        scope: None,
        source,
    }
}

#[test]
fn colliding_owners_select_only_their_own_object_and_property() {
    let project = project(
        vec![
            dynamic("Primary", &["Props"], None, KEY_COLOR),
            dynamic("Lookup", &["Props"], None, KEY_COLOR),
            dynamic("Lookup", &["Props"], None, KEY_SIZE),
            dynamic("Primary", &["Props"], None, KEY_SIZE),
            dynamic("Primary", &["Props"], None, KEY_BLANK),
            dynamic("Primary", &["Props"], None, KEY_NESTED),
            dynamic("Primary", &["Props"], None, KEY_INT),
            dynamic("Primary", &["Props"], None, KEY_ID),
        ],
        vec![],
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    assert_eq!(
        run(&project, &primary(), inputs(Some(lookup()))),
        Ok(out(vec![
            text("P.red"),
            // A property missing from Lookup never borrows Primary's.
            null(),
            text("N.small"),
            null(),
            // A present JSON null stays distinct; a structural value is absent.
            Instance::Scalar(Value::json_null()),
            null(),
            // A non-string key reads nothing.
            null(),
            // A declared child of the open object is read like a property.
            text("P.id"),
        ]))
    );
    // A missing selected object never borrows the colliding Primary object.
    assert_eq!(
        run(
            &project,
            &primary(),
            inputs(Some(group(vec![("Key", text("k"))])))
        ),
        Ok(out(vec![
            text("P.red"),
            null(),
            null(),
            null(),
            Instance::Scalar(Value::json_null()),
            null(),
            null(),
            text("P.id"),
        ]))
    );
    // The first read of an unsupplied owner fails at its node.
    assert_eq!(
        run(&project, &primary(), inputs(None)),
        Err(access(
            11,
            OwnedSourceError::SourceUnavailable {
                owner: owner("Lookup")
            }
        ))
    );
}

#[test]
fn open_roots_root_pins_and_source_keys() {
    let key_field = |name: &str| {
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owner(name),
                path: path(&["Key"]),
            },
            frame: None,
        })
    };
    let project = project(
        vec![
            dynamic("Settings", &[], None, 20),
            dynamic("Settings", &[], Some(&[]), 20),
            dynamic("Primary", &["Props"], Some(&[]), 21),
            dynamic("Lookup", &["Props"], Some(&[]), 22),
        ],
        vec![
            (
                20,
                Node::Const {
                    value: Value::String("mode".into()),
                },
            ),
            (21, key_field("Primary")),
            (22, key_field("Lookup")),
        ],
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    let mut named = lookup();
    if let Instance::Group(fields) = &mut named {
        fields[0].1 = text("size");
    }
    assert_eq!(
        run(&project, &primary(), inputs(Some(named))),
        Ok(out(vec![
            text("S.fast"),
            text("S.fast"),
            text("P.red"),
            text("N.small"),
        ]))
    );
}

fn rows_project(name: &str, values: Vec<Node>) -> Project {
    let mut project = project(values, vec![]);
    let fields = match &project.target.kind {
        ir::SchemaKind::Group { children, .. } => children.clone(),
        _ => unreachable!("authored target"),
    };
    project.target = SchemaNode::group("Out", vec![SchemaNode::group("Items", fields).repeating()]);
    project.root = Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owner(name),
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: std::mem::take(&mut project.root.bindings),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}
fn items(rows: Vec<Vec<Instance>>) -> Instance {
    group(vec![(
        "Items",
        Instance::Repeated(rows.into_iter().map(out).collect()),
    )])
}

#[test]
fn row_frames_belong_to_their_owner_only() {
    let project = rows_project(
        "Primary",
        vec![
            dynamic("Primary", &["Rows", "Props"], None, KEY_COLOR),
            dynamic("Primary", &["Rows", "Props"], Some(&["Rows"]), KEY_SIZE),
            dynamic("Primary", &["Props"], None, KEY_COLOR),
            dynamic("Lookup", &["Props"], None, KEY_SIZE),
        ],
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    assert_eq!(
        run(&project, &primary(), inputs(Some(lookup()))),
        Ok(items(vec![
            vec![text("P.row1"), null(), text("P.red"), text("N.small")],
            vec![null(), text("P.row2"), text("P.red"), text("N.small")],
        ]))
    );

    // Lookup has no active row: its repetition is never repaired by the
    // Primary row or by Lookup's first row.
    let uniterated = rows_project(
        "Primary",
        vec![dynamic("Lookup", &["Rows", "Props"], None, KEY_COLOR)],
    );
    assert_eq!(
        run(&uniterated, &primary(), inputs(Some(lookup()))),
        Err(access(
            10,
            OwnedSourceError::UniteratedOwnedRepetition {
                path: path(&["Rows", "Props"])
            }
        ))
    );
    let inactive = rows_project(
        "Primary",
        vec![dynamic(
            "Lookup",
            &["Rows", "Props"],
            Some(&["Rows"]),
            KEY_COLOR,
        )],
    );
    assert_eq!(
        run(&inactive, &primary(), inputs(Some(lookup()))),
        Err(access(
            10,
            OwnedSourceError::InactiveSourceFrame {
                owner: owner("Lookup"),
                collection: Some(path(&["Rows"])),
            }
        ))
    );
    let named = rows_project(
        "Lookup",
        vec![
            dynamic("Lookup", &["Rows", "Props"], Some(&["Rows"]), KEY_COLOR),
            dynamic("Primary", &["Props"], None, KEY_COLOR),
        ],
    );
    assert_eq!(
        run(&named, &primary(), inputs(Some(lookup()))),
        Ok(items(vec![vec![text("N.row1"), text("P.red")]]))
    );
}

#[test]
fn keys_evaluate_first_and_untaken_branches_read_nothing() {
    let conditional = |condition: u32| Node::If {
        condition,
        then: 30,
        else_: 31,
    };
    let project = project(
        vec![conditional(40), conditional(41)],
        vec![
            (30, dynamic("Lookup", &["Props"], None, 32)),
            (
                31,
                Node::Const {
                    value: Value::String("skip".into()),
                },
            ),
            (32, Node::Raise { message: None }),
            (
                40,
                Node::Const {
                    value: Value::Bool(false),
                },
            ),
            (
                41,
                Node::Const {
                    value: Value::Bool(false),
                },
            ),
        ],
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    // Neither the raising key nor the unsupplied owner is reached.
    assert_eq!(
        run(&project, &primary(), inputs(None)),
        Ok(out(vec![text("skip"), text("skip")]))
    );
    let mut taken = project.clone();
    taken.graph.nodes.insert(
        41,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    // The key fails before the unsupplied owner is inspected.
    assert_eq!(
        run(&taken, &primary(), inputs(None)),
        Err(EngineError::MappingException {
            node: 32,
            message: None
        })
    );
    // A non-string key reads nothing, even from an unsupplied owner.
    taken.graph.nodes.insert(
        32,
        Node::Const {
            value: Value::Int(1),
        },
    );
    assert_eq!(
        run(&taken, &primary(), inputs(None)),
        Ok(out(vec![text("skip"), null()]))
    );
    taken.graph.nodes.insert(
        32,
        Node::Const {
            value: Value::String("size".into()),
        },
    );
    assert_eq!(
        run(&taken, &primary(), inputs(None)),
        Err(access(
            30,
            OwnedSourceError::SourceUnavailable {
                owner: owner("Lookup")
            }
        ))
    );
}

#[test]
fn malformed_ownership_and_closed_contexts_are_refused() {
    for (node, expected) in [
        (
            dynamic("Missing", &["Props"], None, KEY_COLOR),
            "unknown source owner `Missing`",
        ),
        (
            dynamic("", &["Props"], None, KEY_COLOR),
            "named source owner cannot be empty",
        ),
        (
            dynamic("Primary", &["Key"], None, KEY_COLOR),
            "owned dynamic object `Key` matches no open scalar object in its owner",
        ),
        (
            dynamic("Primary", &["Rows"], None, KEY_COLOR),
            "owned dynamic object `Rows` matches no open scalar object in its owner",
        ),
        (
            dynamic("Primary", &["Props"], Some(&["Rows"]), KEY_COLOR),
            "owned frame `Rows` does not prefix dynamic object `Props`",
        ),
        (
            dynamic(
                "Primary",
                &["Rows", "Props"],
                Some(&["Rows", "Props"]),
                KEY_COLOR,
            ),
            "owned collection `Rows/Props` is not a repeating path in its owner",
        ),
    ] {
        let project = project(vec![node], vec![]);
        let issues = crate::validate(&project);
        assert!(
            issues
                .iter()
                .any(|issue| issue.to_string().contains(expected)),
            "{expected}: {issues:?}"
        );
    }

    // A dynamic named owner is a closed capability context.
    let mut dynamic_owner = project(vec![dynamic("Lookup", &["Props"], None, KEY_COLOR)], vec![]);
    dynamic_owner.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(
        KEY_COLOR,
        path(&["Rows"]),
    ));
    assert!(matches!(
        run(&dynamic_owner, &primary(), inputs(Some(lookup()))),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::DynamicOwner,
            ..
        })
    ));

    // Failure-rule expressions and isolated functions keep refusing the read.
    let mut failure = project(
        vec![],
        vec![(50, dynamic("Primary", &["Props"], None, KEY_COLOR))],
    );
    failure.graph.nodes.insert(
        51,
        Node::Call {
            function: "exists".into(),
            args: vec![50],
        },
    );
    failure.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: Vec::new(),
        },
        selection: FailureSelection::WhenTrue { predicate: 51 },
        message: None,
    });
    assert!(matches!(
        run(&failure, &primary(), inputs(Some(lookup()))),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::FailureRule,
            ..
        })
    ));
    let mut function = project(
        vec![Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: vec![],
        }],
        vec![],
    );
    let mut body = Graph::default();
    body.nodes.insert(
        1,
        Node::Const {
            value: Value::String("color".into()),
        },
    );
    body.nodes
        .insert(2, dynamic("Primary", &["Props"], None, 1));
    function.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "local".into(),
            name: "read".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 2,
        },
    );
    assert_eq!(
        run(&function, &primary(), inputs(Some(lookup()))),
        Err(EngineError::UnsupportedSourceOwnership {
            node: 2,
            function: Some(FunctionId::new(1)),
            kind: "owned_dynamic_source_field",
        })
    );
}

#[test]
fn legacy_dynamic_field_keeps_outward_fallback() {
    let legacy = Node::DynamicSourceField {
        object: path(&["Props"]),
        frame: None,
        key: KEY_COLOR,
    };
    let mut legacy_project = rows_project("Primary", vec![legacy.clone()]);
    legacy_project.root.children[0].iteration = ScopeIteration::Source(path(&["Rows"]));
    assert!(
        crate::validate(&legacy_project).is_empty(),
        "{:?}",
        crate::validate(&legacy_project)
    );
    // The second row has no object, so the legacy read falls back to the root
    // object; the owned form never does.
    let source = endpoint(
        vec![("color", text("P.red"))],
        vec![
            row("P1", vec![("color", text("P.row1"))]),
            group(vec![("Id", text("P2"))]),
        ],
    );
    assert_eq!(
        run(&legacy_project, &source, inputs(Some(lookup()))),
        Ok(items(vec![vec![text("P.row1")], vec![text("P.red")]]))
    );
    let owned = rows_project(
        "Primary",
        vec![dynamic("Primary", &["Rows", "Props"], None, KEY_COLOR)],
    );
    assert_eq!(
        run(&owned, &source, inputs(Some(lookup()))),
        Ok(items(vec![vec![text("P.row1")], vec![null()]]))
    );
    // A legacy read stays closed inside an owned scope.
    let mixed = rows_project("Primary", vec![legacy]);
    assert!(matches!(
        run(&mixed, &source, inputs(Some(lookup()))),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::DynamicTarget,
            ..
        })
    ));
}

#[test]
fn exists_predicates_keep_the_receiving_owner_frames() {
    // A pure-producer Exists predicate adds only its private item; the owned
    // read still selects Lookup's object, never Primary's.
    let project = project(
        vec![Node::SequenceExists {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 60,
                item: 61,
            },
            predicate: 62,
        }],
        vec![
            (
                60,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                61,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            ),
            (
                62,
                Node::Call {
                    function: "equal".into(),
                    args: vec![63, 64],
                },
            ),
            (63, dynamic("Lookup", &["Props"], None, KEY_SIZE)),
            (
                64,
                Node::Const {
                    value: Value::String("N.small".into()),
                },
            ),
        ],
    );
    assert!(
        crate::validate(&project).is_empty(),
        "{:?}",
        crate::validate(&project)
    );
    assert_eq!(
        run(&project, &primary(), inputs(Some(lookup()))),
        Ok(out(vec![Instance::Scalar(Value::Bool(true))]))
    );
    let mut primary_size = project.clone();
    primary_size
        .graph
        .nodes
        .insert(63, dynamic("Primary", &["Props"], None, KEY_SIZE));
    assert_eq!(
        run(&primary_size, &primary(), inputs(Some(lookup()))),
        Ok(out(vec![Instance::Scalar(Value::Bool(false))]))
    );
}
