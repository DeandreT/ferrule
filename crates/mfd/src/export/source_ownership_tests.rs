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
fn physical_row_path_export_remains_explicitly_refused() {
    let mut project = project();
    project.graph.nodes.insert(
        9,
        Node::OwnedRowDocumentPath(mapping::OwnedRowDocumentPath {
            owner: SourceOwner::Primary,
            collection: Some(vec![]),
        }),
    );
    let path = Path::new("nonexistent-owned-export-parent/row-path.mfd");
    for result in [
        preflight_export(&project, path).map(|_| ()),
        export(&project, path).map(|_| ()),
        export_with_profile(&project, path, ExportProfile::NativeMfd).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(MfdError::UnsupportedSourceOwnership {
                node: 9,
                function: None,
                kind: "owned_row_document_path",
            })
        ));
    }
    assert!(!path.exists());
}

#[test]
fn document_set_path_export_remains_explicitly_refused() {
    let mut project = project();
    project.graph.nodes.insert(
        9,
        Node::OwnedSourceDocumentPath(mapping::OwnedSourceDocumentPath {
            owner: SourceOwner::Primary,
        }),
    );
    let path = Path::new("nonexistent-owned-export-parent/document-path.mfd");
    for result in [
        preflight_export(&project, path).map(|_| ()),
        export(&project, path).map(|_| ()),
        export_with_profile(&project, path, ExportProfile::NativeMfd).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(MfdError::UnsupportedSourceOwnership {
                node: 9,
                function: None,
                kind: "owned_source_document_path",
            })
        ));
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

#[test]
fn owned_recursive_filters_refuse_before_writing_in_every_profile() {
    let directory = || {
        SchemaNode::group(
            "Dir",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                    .repeating(),
                SchemaNode::recursive_group("Dir", "Dir").repeating(),
            ],
        )
    };
    let mut project = project();
    project.source = SchemaNode::group("Input", vec![directory().repeating()]);
    project.target = SchemaNode::group("Output", vec![directory().repeating()]);
    project.graph.nodes = BTreeMap::from([(
        9,
        Node::Const {
            value: ir::Value::Bool(true),
        },
    )]);
    project.root.children = vec![Scope {
        target_field: "Dir".into(),
        iteration: mapping::ScopeIteration::OwnedSource {
            source: mapping::OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Dir".into()],
            },
            output_path: None,
        },
        construction: mapping::ScopeConstruction::RecursiveFilter {
            plan: mapping::RecursiveFilterPlan::new("Dir".into(), "file".into(), 9).unwrap(),
        },
        ..Scope::default()
    }];
    let path = Path::new("nonexistent-owned-filter-parent/output.mfd");
    let expected = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0));
    let refused = |error: MfdError| matches!(error, MfdError::UnsupportedSourceIteration { scope } if scope == expected);
    assert!(refused(preflight_export(&project, path).unwrap_err()));
    assert!(refused(export(&project, path).unwrap_err()));
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        assert!(refused(
            export_with_profile(&project, path, profile).unwrap_err()
        ));
    }
    assert!(!path.exists());
}

#[test]
fn owned_lookup_and_find_refuse_before_writing_anything() {
    let collection = mapping::OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "Rates".into(),
        },
        path: vec!["Rate".into()],
    };
    for (node, kind) in [
        (
            Node::OwnedLookup(mapping::OwnedLookup {
                collection: collection.clone(),
                key: vec!["Code".into()],
                matches: 1,
                value: vec!["Value".into()],
            }),
            "owned_lookup",
        ),
        (
            Node::OwnedCollectionFind(mapping::OwnedCollectionFind {
                collection: collection.clone(),
                predicate: 1,
                value: 1,
            }),
            "owned_collection_find",
        ),
    ] {
        let mut project = project();
        project.graph.nodes.insert(9, node);
        let path = Path::new("nonexistent-owned-export-parent/output.mfd");
        for error in [
            preflight_export(&project, path).unwrap_err(),
            export(&project, path).unwrap_err(),
            export_with_profile(&project, path, ExportProfile::NativeMfd).unwrap_err(),
        ] {
            assert!(
                matches!(error, MfdError::UnsupportedSourceOwnership { node: 9, function: None, kind: found } if found == kind),
                "{error:?}"
            );
        }
        assert!(!path.exists());
    }
}

#[test]
fn owned_dynamic_reads_refuse_before_writing_in_every_profile() {
    let read = Node::OwnedDynamicSourceField(mapping::OwnedDynamicSourceField {
        object: mapping::OwnedSourceRef {
            owner: SourceOwner::Named {
                name: "Lookup".into(),
            },
            path: vec!["Props".into()],
        },
        frame: None,
        key: 1,
    });
    let path = Path::new("nonexistent-owned-dynamic-parent/output.mfd");
    let mut graph = project();
    graph.graph.nodes = BTreeMap::from([
        (
            1,
            Node::Const {
                value: ir::Value::String("color".into()),
            },
        ),
        (9, read.clone()),
    ]);
    assert!(matches!(
        preflight_export(&graph, path).unwrap_err(),
        MfdError::UnsupportedSourceOwnership {
            node: 9,
            function: None,
            kind: "owned_dynamic_source_field"
        }
    ));
    let mut function = project();
    function.graph.nodes.clear();
    let mut body = Graph::default();
    body.nodes.insert(9, read);
    function.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "user".into(),
            name: "read".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body,
            output: 9,
        },
    );
    assert!(matches!(
        preflight_export(&function, path).unwrap_err(),
        MfdError::UnsupportedSourceOwnership {
            node: 9,
            function: Some(_),
            kind: "owned_dynamic_source_field"
        }
    ));
    for project in [&graph, &function] {
        for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
            assert!(export_with_profile(project, path, profile).is_err());
        }
    }
    assert!(!path.exists());
}

#[test]
fn owned_xml_serializers_are_refused_before_writing_anything() {
    let mut project = project();
    project.graph.nodes.insert(
        9,
        Node::OwnedXmlSerialize(mapping::OwnedXmlSerialize {
            source: mapping::OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "audit".into(),
                },
                path: vec!["Header".into()],
            },
            frame: None,
            schema: Box::new(SchemaNode::group("Header", vec![])),
            declaration: false,
            indent: true,
            namespace: None,
        }),
    );
    let path = Path::new("nonexistent-owned-xml-export-parent/output.mfd");
    let refused = |error: MfdError| {
        assert!(matches!(
            error,
            MfdError::UnsupportedSourceOwnership {
                node: 9,
                function: None,
                kind: "owned_xml_serialize",
            }
        ));
    };
    refused(preflight_export(&project, path).unwrap_err());
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        refused(export_with_profile(&project, path, profile).unwrap_err());
    }
    assert!(!path.exists());
}

#[test]
fn owned_aggregates_refuse_before_writing_anything() {
    let collection = mapping::OwnedSourceRef {
        owner: SourceOwner::Named {
            name: "Rates".into(),
        },
        path: vec!["Rate".into()],
    };
    for value in [
        mapping::OwnedAggregateValue::Path(vec!["Value".into()]),
        mapping::OwnedAggregateValue::Expression(1),
    ] {
        let mut project = project();
        project.graph.nodes.insert(
            9,
            Node::OwnedAggregate(mapping::OwnedAggregate {
                function: mapping::AggregateOp::Sum,
                collection: collection.clone(),
                value,
                arg: None,
            }),
        );
        let path = Path::new("nonexistent-owned-export-parent/output.mfd");
        for error in [
            preflight_export(&project, path).unwrap_err(),
            export(&project, path).unwrap_err(),
            export_with_profile(&project, path, ExportProfile::NativeMfd).unwrap_err(),
        ] {
            assert!(
                matches!(
                    error,
                    MfdError::UnsupportedSourceOwnership {
                        node: 9,
                        function: None,
                        kind: "owned_aggregate"
                    }
                ),
                "{error:?}"
            );
        }
        assert!(!path.exists());
    }
}

#[test]
fn owned_xml_mixed_content_is_refused_before_writing_anything() {
    let mut project = project();
    project.graph.nodes.insert(
        9,
        Node::OwnedXmlMixedContent(mapping::OwnedXmlMixedContent {
            source: mapping::OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "audit".into(),
                },
                path: vec!["Header".into()],
            },
            frame: None,
            replacements: vec![],
        }),
    );
    let path = Path::new("nonexistent-owned-xml-mixed-export-parent/output.mfd");
    let refused = |error: MfdError| {
        assert!(matches!(
            error,
            MfdError::UnsupportedSourceOwnership {
                node: 9,
                function: None,
                kind: "owned_xml_mixed_content",
            }
        ));
    };
    refused(preflight_export(&project, path).unwrap_err());
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        refused(export_with_profile(&project, path, profile).unwrap_err());
    }
    assert!(!path.exists());
}

#[test]
fn owned_mixed_content_constructions_refuse_before_writing_anything() {
    let mut project = project();
    let paragraph = || {
        SchemaNode::group(
            "Para",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::scalar("b", ScalarType::String).repeating(),
            ],
        )
        .repeating()
    };
    project.source = SchemaNode::group("Input", vec![paragraph()]);
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Para",
                vec![
                    SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                    SchemaNode::scalar("Strong", ScalarType::String).repeating(),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes = BTreeMap::new();
    project.root.children = vec![Scope {
        target_field: "Para".into(),
        iteration: mapping::ScopeIteration::OwnedSource {
            source: mapping::OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Para".into()],
            },
            output_path: None,
        },
        construction: mapping::ScopeConstruction::XmlMixedContent {
            elements: vec![mapping::XmlMixedContentElement {
                source: "b".into(),
                target: "Strong".into(),
            }],
        },
        ..Scope::default()
    }];
    let path = Path::new("nonexistent-owned-mixed-construction-parent/output.mfd");
    let expected = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0));
    let refused = |error: MfdError| matches!(error, MfdError::UnsupportedSourceIteration { scope } if scope == expected);
    assert!(refused(preflight_export(&project, path).unwrap_err()));
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        assert!(refused(
            export_with_profile(&project, path, profile).unwrap_err()
        ));
    }
    assert!(!path.exists());
}

#[test]
fn owned_joined_aggregates_refuse_before_writing_anything() {
    let rows = |owner| mapping::OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    };
    let named = || SourceOwner::Named {
        name: "Rates".into(),
    };
    let plan = mapping::OwnedJoinPlan::new(
        mapping::OwnedJoinSource::new(rows(SourceOwner::Primary)),
        mapping::OwnedJoinSource::new(rows(named())),
        mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
            rows(SourceOwner::Primary),
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    for expression in [None, Some(1)] {
        let mut project = project();
        project.graph.nodes.insert(
            9,
            Node::OwnedJoinAggregate(mapping::OwnedJoinAggregate {
                function: mapping::AggregateOp::Count,
                join: mapping::JoinId::new(3),
                plan: plan.clone(),
                expression,
                arg: None,
            }),
        );
        let path = Path::new("nonexistent-owned-join-aggregate-export-parent/output.mfd");
        for error in [
            preflight_export(&project, path).unwrap_err(),
            export(&project, path).unwrap_err(),
            export_with_profile(&project, path, ExportProfile::NativeMfd).unwrap_err(),
        ] {
            assert!(
                matches!(
                    error,
                    MfdError::UnsupportedSourceOwnership {
                        node: 9,
                        function: None,
                        kind: "owned_join_aggregate"
                    }
                ),
                "{error:?}"
            );
        }
        assert!(!path.exists());
    }
}

#[test]
fn owned_dynamic_drivers_refuse_before_writing_in_every_profile() {
    let driver = mapping::OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: vec!["Files".into()],
    };
    let mut declared = project();
    declared.source = SchemaNode::group(
        "Batch",
        vec![
            SchemaNode::group(
                "Files",
                vec![SchemaNode::scalar("Path", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    declared.graph.nodes = BTreeMap::from([(
        9,
        Node::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Files".into(), "Path".into()],
            },
            frame: Some(vec!["Files".into()]),
        }),
    )]);
    declared.extra_sources = vec![mapping::NamedSource {
        name: "Docs".into(),
        path: String::new(),
        schema: SchemaNode::group("Doc", vec![SchemaNode::scalar("Title", ScalarType::String)]),
        options: Default::default(),
        dynamic_path: Some(mapping::DynamicSourcePath::owned(9, driver)),
    }];
    let path = Path::new("nonexistent-owned-driver-parent/output.mfd");
    let refused = |error: MfdError| matches!(error, MfdError::UnsupportedDynamicSourceDriver { source_name } if source_name == "Docs");
    assert!(refused(preflight_export(&declared, path).unwrap_err()));
    assert!(refused(export(&declared, path).unwrap_err()));
    for profile in [ExportProfile::default(), ExportProfile::NativeMfd] {
        assert!(refused(
            export_with_profile(&declared, path, profile).unwrap_err()
        ));
    }
    assert!(!path.exists());
}
