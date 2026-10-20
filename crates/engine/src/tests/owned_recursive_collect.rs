//! #463 recursive collection walks from one explicitly owned root collection.
use ir::{Instance, ScalarType, SchemaNode, Value, XmlNil};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph,
    NamedSource, Node, OwnedRecursiveCollect, OwnedSourceField, OwnedSourceRef, Project, Scope,
    ScopeConstruction, ScopeIteration, SequenceExpr, SourceOwner, SourceScopeLocation,
    SourceScopeStep, SourceScopeTarget, UserFunction,
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
fn walk(name: &str, parts: &[&str], prefix: u32, separator: u32, item: u32) -> SequenceExpr {
    SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
        children: path(&["Dir"]),
        descent_value: path(&["Note"]),
        values: path(&["file"]),
        value: path(&["Note"]),
        prefix,
        separator,
        item,
    })
}
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
fn dir(note: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    group(vec![
        ("Note", text(note)),
        (
            "file",
            Instance::Repeated(
                files
                    .iter()
                    .map(|file| group(vec![("Note", text(file))]))
                    .collect(),
            ),
        ),
        ("Dir", Instance::Repeated(children)),
    ])
}
fn endpoint(note: &str, dirs: Vec<Instance>) -> Instance {
    group(vec![
        ("Note", text(note)),
        ("Dir", Instance::Repeated(dirs)),
    ])
}
fn dir_schema() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            SchemaNode::group("file", vec![SchemaNode::scalar("Note", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}
fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Note", ScalarType::String),
            dir_schema().repeating(),
        ],
    )
}
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}

/// Primary and Lookup share every path. Node 10 is bound to `V1`; 1 and 2 are
/// the empty prefix and `/` separator; 3 is the index 1.
fn project(v1: Node, nodes: Vec<(u32, Node)>) -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("/".into()),
        },
    );
    graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Int(1),
        },
    );
    graph.nodes.insert(10, v1);
    graph.nodes.insert(11, item());
    graph.nodes.extend(nodes);
    Project {
        source: endpoint_schema("Input"),
        target: SchemaNode::group("Out", vec![SchemaNode::scalar("V1", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: endpoint_schema("Lookup"),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "V1".into(),
                node: 10,
            }],
            ..Scope::default()
        },
    }
}
fn item_at(name: &str, parts: &[&str], index: u32) -> Node {
    Node::SequenceItemAt {
        sequence: walk(name, parts, 1, 2, 11),
        index,
    }
}
fn primary() -> Instance {
    endpoint(
        "P.note",
        vec![dir("p", &["a"], vec![dir("sub", &["b"], vec![])])],
    )
}
fn lookup() -> Instance {
    endpoint("N.note", vec![dir("n", &["m"], vec![])])
}
fn run(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
) -> Result<Instance, EngineError> {
    let execution = ExecutionContext::new(std::path::Path::new("owned463.mapping"));
    crate::run_outputs_with_sources_and_context(project, source, extras, &execution)
        .map(|outputs| outputs.primary)
}
fn v1(value: Instance) -> Instance {
    group(vec![("V1", value)])
}
fn with_lookup(value: Instance) -> Vec<(String, Instance)> {
    vec![("Lookup".into(), value)]
}

#[test]
fn owned_roots_select_only_the_declared_endpoint() {
    let primary_walk = project(item_at("Primary", &["Dir"], 3), vec![]);
    assert!(crate::validate(&primary_walk).is_empty());
    assert_eq!(
        run(&primary_walk, &primary(), with_lookup(lookup())),
        Ok(v1(text("/p/a")))
    );
    let named_walk = project(item_at("Lookup", &["Dir"], 3), vec![]);
    assert_eq!(
        run(&named_walk, &primary(), with_lookup(lookup())),
        Ok(v1(text("/n/m")))
    );
    // A missing selected collection never borrows the colliding Primary tree.
    assert_eq!(
        run(
            &named_walk,
            &primary(),
            with_lookup(group(vec![("Note", text("N.note"))]))
        ),
        Ok(v1(Instance::Scalar(Value::Null)))
    );
    assert_eq!(
        run(&named_walk, &primary(), Vec::new()),
        Err(EngineError::OwnedSourceAccess {
            node: Some(10),
            scope: None,
            source: OwnedSourceError::SourceUnavailable {
                owner: owner("Lookup"),
            },
        })
    );
}

#[test]
fn relative_paths_and_an_active_frame_keep_their_roles() {
    // An active Primary item pins the root collection to that item's subtree.
    let mut nested = project(item_at("Primary", &["Dir"], 3), vec![]);
    nested.target = SchemaNode::group(
        "Out",
        vec![
            SchemaNode::group("Items", vec![SchemaNode::scalar("V1", ScalarType::String)])
                .repeating(),
        ],
    );
    nested.root = Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Dir"]),
                },
                output_path: None,
            },
            bindings: vec![Binding {
                target_field: "V1".into(),
                node: 10,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    assert!(crate::validate(&nested).is_empty());
    let source = endpoint(
        "P.note",
        vec![dir("p", &["a"], vec![]), dir("q", &["c"], vec![])],
    );
    assert_eq!(
        run(&nested, &source, with_lookup(lookup())),
        Ok(group(vec![(
            "Items",
            Instance::Repeated(vec![v1(text("/p/a")), v1(text("/q/c"))]),
        )]))
    );
}

#[test]
fn parent_arguments_precede_the_walk_and_the_index() {
    let owned_prefix = |id: u32, name: &str| {
        (
            id,
            Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: owner(name),
                    path: path(&["Note"]),
                },
                frame: None,
            }),
        )
    };
    let mut prefixed = project(
        Node::SequenceItemAt {
            sequence: walk("Primary", &["Dir"], 20, 2, 11),
            index: 3,
        },
        vec![owned_prefix(20, "Lookup")],
    );
    assert_eq!(
        run(&prefixed, &primary(), with_lookup(lookup())),
        Ok(v1(text("N.note/p/a")))
    );
    // A prefix error precedes the unsupplied owner and the never-evaluated index.
    prefixed.graph.nodes.insert(
        20,
        Node::Const {
            value: Value::XmlNil(XmlNil),
        },
    );
    prefixed
        .graph
        .nodes
        .insert(30, Node::Raise { message: None });
    if let Some(Node::SequenceItemAt { index, sequence }) = prefixed.graph.nodes.get_mut(&10) {
        *index = 30;
        *sequence = walk("Lookup", &["Dir"], 20, 2, 11);
    }
    assert_eq!(
        run(&prefixed, &primary(), Vec::new()),
        Err(EngineError::Function(
            functions::FunctionError::TypeMismatch {
                function: "recursive-collect",
                got: "xml nil",
            }
        ))
    );
    // A walk error precedes the index too.
    prefixed
        .graph
        .nodes
        .insert(20, owned_prefix(20, "Primary").1);
    assert_eq!(
        run(&prefixed, &primary(), Vec::new()),
        Err(EngineError::OwnedSourceAccess {
            node: Some(10),
            scope: None,
            source: OwnedSourceError::SourceUnavailable {
                owner: owner("Lookup"),
            },
        })
    );
}

#[test]
fn owned_walks_keep_legacy_limits_and_typed_leaf_errors() {
    let walk = project(item_at("Primary", &["Dir"], 3), vec![]);
    let mut deepest = dir("leaf", &[], vec![]);
    for depth in 0..crate::sequence::MAX_RECURSIVE_SEQUENCE_DEPTH {
        deepest = dir(&format!("level-{depth}"), &[], vec![deepest]);
    }
    assert_eq!(
        run(&walk, &endpoint("P", vec![deepest]), Vec::new()),
        Err(EngineError::RecursiveSequenceDepth {
            limit: crate::sequence::MAX_RECURSIVE_SEQUENCE_DEPTH,
        })
    );
    let null_leaf = group(vec![
        ("Note", text("d")),
        (
            "file",
            Instance::Repeated(vec![group(vec![("Note", Instance::Scalar(Value::Null))])]),
        ),
    ]);
    assert_eq!(
        run(&walk, &endpoint("P", vec![null_leaf]), Vec::new()),
        Err(EngineError::Function(
            functions::FunctionError::TypeMismatch {
                function: "recursive-collect",
                got: "null",
            }
        ))
    );
}

#[test]
fn unqualified_owned_producer_contexts_refuse_before_evaluation() {
    let closed = |node: Option<u32>, scope: Option<SourceScopeLocation>, context| {
        EngineError::UnsupportedOwnedContext {
            node,
            function: None,
            scope,
            context,
        }
    };
    // Private-item reducers stay closed.
    let exists = project(
        Node::SequenceExists {
            sequence: walk("Lookup", &["Dir"], 1, 2, 11),
            predicate: 11,
        },
        vec![],
    );
    assert_eq!(
        run(&exists, &primary(), with_lookup(lookup())),
        Err(closed(
            Some(10),
            None,
            OwnedCapabilityContext::GeneratedItem
        ))
    );
    let aggregate = project(
        Node::SequenceAggregate {
            function: AggregateOp::Count,
            sequence: walk("Lookup", &["Dir"], 1, 2, 11),
            predicate: None,
            expression: None,
            arg: None,
        },
        vec![],
    );
    assert_eq!(
        run(&aggregate, &primary(), with_lookup(lookup())),
        Err(closed(
            Some(10),
            None,
            OwnedCapabilityContext::GeneratedItem
        ))
    );
    // A generated item never becomes an endpoint-bearing scope frame.
    let mut scope = project(Node::Const { value: Value::Null }, vec![]);
    scope.target = SchemaNode::group(
        "Out",
        vec![SchemaNode::scalar("Files", ScalarType::String).repeating()],
    );
    scope.root = Scope {
        children: vec![Scope {
            target_field: "Files".into(),
            iteration: ScopeIteration::Sequence(walk("Lookup", &["Dir"], 1, 2, 11)),
            construction: ScopeConstruction::Scalar { value: 11 },
            ..Scope::default()
        }],
        ..Scope::default()
    };
    assert!(crate::validate(&scope).is_empty());
    assert_eq!(
        run(&scope, &primary(), with_lookup(lookup())),
        Err(closed(
            None,
            Some(
                SourceScopeLocation::root(SourceScopeTarget::Primary)
                    .descendant(SourceScopeStep::Child(0))
            ),
            OwnedCapabilityContext::GeneratedItem,
        ))
    );
    let mut failure = project(Node::Const { value: Value::Null }, vec![]);
    failure.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: walk("Lookup", &["Dir"], 1, 2, 11),
        },
        selection: FailureSelection::All,
        message: None,
    }];
    assert_eq!(
        run(&failure, &primary(), with_lookup(lookup())),
        Err(closed(None, None, OwnedCapabilityContext::FailureRule))
    );
    // A closed parent context refuses an otherwise qualified ItemAt.
    let lookup_count = project(
        Node::Aggregate {
            function: AggregateOp::Count,
            collection: path(&["Dir"]),
            value: vec![],
            expression: Some(20),
            arg: None,
        },
        vec![(20, item_at("Lookup", &["Dir"], 3))],
    );
    assert_eq!(
        run(&lookup_count, &primary(), with_lookup(lookup())),
        Err(closed(
            Some(20),
            None,
            OwnedCapabilityContext::AggregateLookup
        ))
    );
}

#[test]
fn function_bodies_and_dynamic_owners_refuse_owned_producers() {
    let function = FunctionId::new(7);
    let mut body = Graph::default();
    for (id, node) in [
        (
            1,
            Node::Const {
                value: Value::String(String::new()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("/".into()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (10, item_at("Primary", &["Dir"], 3)),
        (11, item()),
    ] {
        body.nodes.insert(id, node);
    }
    let mut call = project(
        Node::UserFunctionCall {
            function,
            args: vec![],
        },
        vec![],
    );
    call.user_functions.insert(
        function,
        UserFunction {
            library: "user".into(),
            name: "walk".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 10,
        },
    );
    assert_eq!(
        run(&call, &primary(), with_lookup(lookup())),
        Err(EngineError::UnsupportedSourceOwnership {
            node: 10,
            function: Some(function),
            kind: "owned_recursive_collect",
        })
    );
    let mut dynamic = project(item_at("Lookup", &["Dir"], 3), vec![]);
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 1,
        iteration: path(&["Dir"]),
    });
    assert_eq!(
        run(&dynamic, &primary(), Vec::new()),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(10),
            function: None,
            scope: None,
            context: OwnedCapabilityContext::DynamicOwner,
        })
    );
}
