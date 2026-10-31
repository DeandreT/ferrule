//! Generated Rust and C# share this lowering. A computed join-key source
//! lowers wherever a collection join does, carrying its key node, and every
//! route that refuses legacy joins refuses it with the same diagnostic.

use ir::{ScalarType, Value};
use mapping::{
    AggregateOp, Binding as MappingBinding, ComputedJoinSourceId, JoinConditions, JoinId, JoinKey,
    JoinPlan, JoinSource,
};

use super::*;

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

const JOIN: JoinId = JoinId::new(7);
const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);

fn plan(key_first: bool) -> JoinPlan {
    let customers = JoinSource::new(vec!["Customer".into()]);
    if key_first {
        JoinPlan::new(
            JoinSource::computed(KEY, 1),
            customers,
            JoinConditions::new(JoinKey::computed(KEY, vec!["Number".into()])),
        )
    } else {
        JoinPlan::new(
            customers,
            JoinSource::computed(KEY, 1),
            JoinConditions::new(JoinKey::new(
                vec!["Customer".into()],
                vec!["Number".into()],
                Vec::new(),
            )),
        )
    }
    .unwrap()
}

fn project(root: Scope, aggregate: Option<JoinPlan>) -> Project {
    let mut nodes = BTreeMap::from([
        (
            1,
            Node::Const {
                value: Value::String("C2".into()),
            },
        ),
        (
            2,
            Node::JoinField {
                join: JOIN,
                collection: vec!["Customer".into()],
                path: vec!["Name".into()],
            },
        ),
    ]);
    if let Some(plan) = aggregate {
        nodes.insert(
            3,
            Node::JoinAggregate {
                function: AggregateOp::Count,
                join: JOIN,
                plan,
                expression: None,
                arg: None,
            },
        );
    }
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Customer", vec![scalar("Number"), scalar("Name")]).repeating()],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Hit", vec![scalar("Name")]).repeating(),
                SchemaNode::scalar("Count", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph { nodes },
        root,
    }
}

fn scope_project(key_first: bool) -> Project {
    project(
        Scope {
            children: vec![Scope {
                target_field: "Hit".into(),
                iteration: ScopeIteration::InnerJoin {
                    id: JOIN,
                    plan: plan(key_first),
                },
                bindings: vec![MappingBinding {
                    target_field: "Name".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
        None,
    )
}

fn aggregate_project(key_first: bool) -> Project {
    project(
        Scope {
            bindings: vec![MappingBinding {
                target_field: "Count".into(),
                node: 3,
            }],
            ..Scope::default()
        },
        Some(plan(key_first)),
    )
}

/// The project's diagnostics from every generation route: plain lowering,
/// each owned preset, and the primary-document routes (direct, XML and
/// JSON).
fn route_diagnostics(project: &Project) -> Vec<(String, Vec<Diagnostic>)> {
    let refused = |route: String, result: Result<Program, LowerError>| {
        let diagnostics = result
            .map(|_| Vec::new())
            .unwrap_or_else(LowerError::into_diagnostics);
        (route, diagnostics)
    };
    let mut routes = vec![refused("lower".into(), lower(project))];
    routes.extend(crate::OwnedPreset::ALL.into_iter().map(|preset| {
        refused(
            format!("{preset:?}"),
            crate::lower_with_owned_capabilities(project, preset.capabilities()),
        )
    }));
    for (format, xml, json) in [
        ("direct", false, false),
        ("xml", true, false),
        ("json", false, true),
    ] {
        let mut project = project.clone();
        for options in [&mut project.source_options, &mut project.target_options] {
            options.xml_document = xml;
            options.json_document = json;
        }
        let filename = 99;
        project.graph.nodes.insert(
            filename,
            Node::Const {
                value: Value::String("input".into()),
            },
        );
        let document = mapping::DocumentProject::new(
            project.clone(),
            mapping::PrimarySourcePlan::new(filename),
        );
        routes.push(refused(
            format!("document {format}"),
            crate::lower_document(&document)
                .map(|document| document.program().clone())
                .map_err(|error| match error {
                    crate::DocumentLowerError::Mapping(error) => error,
                    crate::DocumentLowerError::Filename(error) => panic!("{format}: {error}"),
                }),
        ));
        match format {
            "xml" => routes.push(refused(
                "primary xml documents".into(),
                lower_primary_xml_documents(&project),
            )),
            "json" => routes.push(refused(
                "primary json documents".into(),
                lower_primary_json_documents(&document),
            )),
            _ => {}
        }
    }
    routes
}

/// The same project keyed by the existing singleton form over a root
/// `Code` field instead of the computed constant.
fn singleton_equivalent(project: &Project) -> Project {
    let mut project = project.clone();
    project.source = SchemaNode::group(
        "Source",
        vec![
            scalar("Code"),
            SchemaNode::group("Customer", vec![scalar("Number"), scalar("Name")]).repeating(),
        ],
    );
    let singleton = JoinPlan::new(
        JoinSource::singleton(vec!["Code".into()]),
        JoinSource::new(vec!["Customer".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Code".into()],
            Vec::new(),
            vec!["Number".into()],
        )),
    )
    .unwrap();
    if let Some(Node::JoinAggregate { plan, .. }) = project.graph.nodes.get_mut(&3) {
        *plan = singleton;
    } else if let ScopeIteration::InnerJoin { plan, .. } = &mut project.root.children[0].iteration {
        *plan = singleton;
    }
    project
}

fn expected_plan(key_first: bool) -> crate::JoinPlan {
    let customers = crate::JoinSource::new(vec!["Customer".into()]);
    if key_first {
        crate::JoinPlan::new(
            crate::JoinSource::computed(KEY, 1),
            customers,
            crate::JoinConditions::new(crate::JoinKey::computed(KEY, vec!["Number".into()])),
        )
    } else {
        crate::JoinPlan::new(
            customers,
            crate::JoinSource::computed(KEY, 1),
            crate::JoinConditions::new(crate::JoinKey::new(
                vec!["Customer".into()],
                vec!["Number".into()],
                Vec::new(),
            )),
        )
    }
    .unwrap()
}

#[test]
fn computed_join_scopes_and_aggregates_lower_with_their_key() {
    for key_first in [true, false] {
        let program = lower(&scope_project(key_first)).expect("computed join scopes lower");
        let iteration = program.root.children[0]
            .iteration
            .as_ref()
            .expect("join iteration");
        assert_eq!(
            iteration.inner_join().map(InnerJoin::plan),
            Some(&expected_plan(key_first))
        );
        assert!(iteration.roots().any(|root| root == 1));
        assert!(program.expressions.iter().any(|node| node.id == 1));

        let program = lower(&aggregate_project(key_first)).expect("computed aggregates lower");
        assert!(matches!(
            program.expressions.iter().find(|node| node.id == 3),
            Some(ExpressionNode {
                expression: Expression::JoinAggregate { join, .. },
                ..
            }) if join.plan() == &expected_plan(key_first)
        ));
        assert!(program.expressions.iter().any(|node| node.id == 1));
    }
}

/// Generated joins admit computed keys exactly where they admit legacy
/// joins: plain and document lowering lower both, and each owned preset
/// refuses both with its own typed capability diagnostic.
#[test]
fn every_generation_route_admits_computed_keys_like_singleton_keys() {
    for key_first in [true, false] {
        for project in [scope_project(key_first), aggregate_project(key_first)] {
            assert_eq!(engine::validate(&project), []);
            let singleton = singleton_equivalent(&project);
            assert_eq!(engine::validate(&singleton), []);
            let routes = route_diagnostics(&project);
            assert_eq!(routes, route_diagnostics(&singleton));
            for (route, diagnostics) in &routes {
                let owned = crate::OwnedPreset::ALL
                    .into_iter()
                    .any(|preset| format!("{preset:?}") == *route);
                assert_eq!(diagnostics.is_empty(), !owned, "{route}: {diagnostics:?}");
            }
        }
    }
}

/// `project` keyed by `tokenize("C2", " ")` instead of the constant `C2`.
fn with_sequence_key(mut project: Project) -> Project {
    project.graph.nodes.extend([
        (
            4,
            Node::Const {
                value: Value::String(" ".into()),
            },
        ),
        (
            5,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    let tokens = mapping::ComputedJoinSource::sequence(
        KEY,
        mapping::SequenceExpr::Tokenize {
            input: 1,
            delimiter: 4,
            item: 5,
        },
    );
    let keyed = |plan: &JoinPlan| {
        let mut sources = plan
            .sources()
            .map(|source| match source {
                JoinSource::Computed(_) => JoinSource::Computed(tokens.clone()),
                JoinSource::Collection(_) => source.clone(),
            })
            .collect::<Vec<_>>();
        let second = sources.remove(1);
        let first = sources.remove(0);
        let (_, conditions) = plan.stages().next().expect("one stage");
        JoinPlan::new(first, second, conditions.clone()).unwrap()
    };
    if let Some(Node::JoinAggregate { plan, .. }) = project.graph.nodes.get_mut(&3) {
        *plan = keyed(plan);
    } else if let ScopeIteration::InnerJoin { plan, .. } = &mut project.root.children[0].iteration {
        *plan = keyed(plan);
    }
    project
}

/// The interpreter runs sequence-valued keys, but generated code does not:
/// every route refuses them with its own scope or node diagnostic.
#[test]
fn every_generation_route_refuses_sequence_valued_keys() {
    for key_first in [true, false] {
        for (project, expected) in [
            (
                with_sequence_key(scope_project(key_first)),
                Diagnostic::UnsupportedScope {
                    target_path: vec!["Hit".into()],
                    feature: ScopeFeature::SequenceComputedJoinKey,
                },
            ),
            (
                with_sequence_key(aggregate_project(key_first)),
                Diagnostic::UnsupportedNode {
                    node: 3,
                    kind: UnsupportedNodeKind::SequenceComputedJoinAggregate,
                },
            ),
        ] {
            assert_eq!(engine::validate(&project), []);
            for (route, diagnostics) in route_diagnostics(&project) {
                assert!(
                    diagnostics.contains(&expected),
                    "{route}: expected {expected:?} in {diagnostics:?}"
                );
            }
        }
    }
    assert_eq!(
        Diagnostic::UnsupportedScope {
            target_path: vec!["Hit".into()],
            feature: ScopeFeature::SequenceComputedJoinKey,
        }
        .to_string(),
        "target scope `Hit`: code generation does not support an inner join with a sequence-valued computed key"
    );
    assert_eq!(
        Diagnostic::UnsupportedNode {
            node: 3,
            kind: UnsupportedNodeKind::SequenceComputedJoinAggregate,
        }
        .to_string(),
        "graph node 3: code generation does not support a join aggregate with a sequence-valued computed key"
    );
}
