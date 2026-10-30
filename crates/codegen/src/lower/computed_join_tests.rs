//! Generated Rust and C# share this lowering, which refuses computed join-key
//! sources until generated joins execute them. The interpreter admits them,
//! so the lowering's own diagnostics keep every generation route closed.

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

/// Engine validation admits computed keys, so each route refuses them with
/// its own unsupported-scope or unsupported-node diagnostic.
#[test]
fn every_generation_route_refuses_computed_join_sources() {
    for key_first in [true, false] {
        for (project, expected) in [
            (
                scope_project(key_first),
                Diagnostic::UnsupportedScope {
                    target_path: vec!["Hit".into()],
                    feature: ScopeFeature::ComputedJoinSource,
                },
            ),
            (
                aggregate_project(key_first),
                Diagnostic::UnsupportedNode {
                    node: 3,
                    kind: UnsupportedNodeKind::ComputedJoinAggregate,
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
}

#[test]
fn computed_join_scopes_lower_to_an_unsupported_scope() {
    for key_first in [true, false] {
        let project = scope_project(key_first);
        let error = lower_iteration(&project.root.children[0], &["Hit".into()])
            .expect_err("computed join scopes do not lower");
        assert_eq!(
            error,
            Diagnostic::UnsupportedScope {
                target_path: vec!["Hit".into()],
                feature: ScopeFeature::ComputedJoinSource,
            }
        );
        assert_eq!(
            error.to_string(),
            "target scope `Hit`: code generation does not support an inner join with a computed key source"
        );
        assert!(crate::JoinPlan::from_mapping(&plan(key_first)).is_none());
    }
}

#[test]
fn computed_join_aggregates_lower_to_an_unsupported_node() {
    for key_first in [true, false] {
        let project = aggregate_project(key_first);
        assert_eq!(
            lower_expression(3, &project.graph.nodes[&3], &project.graph)
                .expect_err("computed join aggregates do not lower"),
            Diagnostic::UnsupportedNode {
                node: 3,
                kind: UnsupportedNodeKind::ComputedJoinAggregate,
            }
        );
    }
}
