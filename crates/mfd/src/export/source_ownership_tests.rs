use std::collections::BTreeMap;
use std::path::Path;

use ir::{ScalarType, SchemaNode};
use mapping::{FunctionId, Graph, Node, OwnedPosition, Project, Scope, SourceOwner, UserFunction};

use super::{ExportProfile, export, export_with_profile, preflight_export};
use crate::MfdError;

fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        user_functions: BTreeMap::new(),
        failure_rules: vec![],
        graph: Graph {
            nodes: BTreeMap::from([(
                9,
                Node::OwnedPosition(OwnedPosition {
                    owner: SourceOwner::Primary,
                    collection: None,
                }),
            )]),
        },
        root: Scope::default(),
    }
}
fn check(error: MfdError, function: Option<FunctionId>) {
    assert!(
        matches!(error, MfdError::UnsupportedSourceOwnership { node: 9, function: found, kind: "owned_position" } if found == function)
    );
}

#[test]
fn every_export_profile_refuses_dead_owned_nodes_before_writing_anything() {
    let project = project();
    // A nonexistent parent discriminates an early typed refusal from artifact I/O.
    let path = Path::new("nonexistent-owned-export-parent/output.mfd");
    check(preflight_export(&project, path).unwrap_err(), None);
    check(export(&project, path).unwrap_err(), None);
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        check(
            export_with_profile(&project, path, profile).unwrap_err(),
            None,
        );
    }
    assert!(!path.exists());
}

#[test]
fn unused_function_bodies_and_special_early_routes_are_not_silently_omitted() {
    let mut project = project();
    let id = FunctionId::new(7);
    let body = std::mem::take(&mut project.graph);
    project.user_functions.insert(
        id,
        UserFunction {
            library: "tests".into(),
            name: "unused".into(),
            description: None,
            parameters: vec![],
            output_name: "value".into(),
            output_type: ScalarType::Int,
            body,
            output: 9,
        },
    );
    check(
        super::prepare_export(&project, Path::new("output.mfd"))
            .err()
            .unwrap(),
        Some(id),
    );
    // The ownership gate precedes the qualified root-view and branch planners.
    project.source_options.xml_root_view_read_policy = true;
    check(
        super::prepare_export(&project, Path::new("output.mfd"))
            .err()
            .unwrap(),
        Some(id),
    );
}

#[test]
fn each_serial_stage_refuses_before_pipeline_publication_in_every_profile() {
    use mapping::{Binding, Pipeline, PipelineInput, PipelineStage};
    for owned_stage in [0, 1] {
        let mut ordinary = project();
        ordinary.graph.nodes.clear();
        ordinary.source = SchemaNode::group(
            "Document",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        ordinary.target = ordinary.source.clone();
        ordinary.source_path = Some("input.xml".into());
        ordinary.target_path = Some("output.xml".into());
        ordinary.graph.nodes.insert(
            1,
            Node::Const {
                value: ir::Value::String("value".into()),
            },
        );
        ordinary.root.bindings.push(Binding {
            target_field: "Value".into(),
            node: 1,
        });
        let mut pipeline = Pipeline {
            main_mapping_path: None,
            stages: vec![
                PipelineStage {
                    id: "first".into(),
                    mapping_path: None,
                    project: ordinary.clone(),
                    source: PipelineInput::Host {
                        name: "input".into(),
                    },
                    extra_sources: vec![],
                },
                PipelineStage {
                    id: "second".into(),
                    mapping_path: None,
                    project: ordinary,
                    source: PipelineInput::StageTarget {
                        stage: "first".into(),
                        target: None,
                    },
                    extra_sources: vec![],
                },
            ],
        };
        pipeline.stages[owned_stage].project.graph.nodes.insert(
            9,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(vec![]),
            }),
        );
        assert!(engine::validate_pipeline(&pipeline).is_empty());
        let path = Path::new("nonexistent-owned-export-parent/pipeline.mfd");
        check(
            super::preflight_pipeline_export(&pipeline, path).unwrap_err(),
            None,
        );
        for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
            check(
                super::export_pipeline_with_profile(&pipeline, path, profile).unwrap_err(),
                None,
            );
        }
        assert!(!path.exists());
    }
}

#[test]
fn owned_recursive_walks_refuse_before_writing_in_every_placement() {
    let walk = || {
        mapping::SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
            collection: mapping::OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Dir".into()],
            },
            children: vec!["Dir".into()],
            descent_value: vec!["Note".into()],
            values: vec!["file".into()],
            value: vec!["Note".into()],
            prefix: 1,
            separator: 1,
            item: 2,
        })
    };
    let path = Path::new("nonexistent-owned-walk-parent/output.mfd");
    let mut consumer = project();
    consumer.graph.nodes = BTreeMap::from([(
        9,
        Node::SequenceItemAt {
            sequence: walk(),
            index: 1,
        },
    )]);
    assert!(matches!(
        preflight_export(&consumer, path).unwrap_err(),
        MfdError::UnsupportedSourceOwnership {
            node: 9,
            function: None,
            kind: "owned_recursive_collect"
        }
    ));
    let mut scope = project();
    scope.graph.nodes.clear();
    scope.root.iteration = mapping::ScopeIteration::Sequence(walk());
    assert!(matches!(
        preflight_export(&scope, path).unwrap_err(),
        MfdError::UnsupportedSourceIteration { scope }
            if scope == mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
    ));
    let mut failure = project();
    failure.graph.nodes.clear();
    failure.failure_rules = vec![mapping::FailureRule {
        iteration: mapping::FailureIteration::Sequence { sequence: walk() },
        selection: mapping::FailureSelection::All,
        message: None,
    }];
    assert!(matches!(
        preflight_export(&failure, path).unwrap_err(),
        MfdError::UnsupportedFailureSourceIteration { rule: 1 }
    ));
    for project in [&consumer, &scope, &failure] {
        for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
            assert!(export_with_profile(project, path, profile).is_err());
        }
    }
    assert!(!path.exists());
}
