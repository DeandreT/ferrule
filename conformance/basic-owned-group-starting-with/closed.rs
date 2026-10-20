//! Negative capability products are not execution cases or fake Projects.
use super::*;
pub fn route() -> mapping::SourceScopeLocation {
    mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary)
        .descendant(mapping::SourceScopeStep::Child(0))
}
pub fn grouping_error() -> codegen::ProgramValidationError {
    iteration_error(route(), codegen::OwnedSourceCapabilityReason::Grouping)
}
fn iteration_error(
    scope: mapping::SourceScopeLocation,
    reason: codegen::OwnedSourceCapabilityReason,
) -> codegen::ProgramValidationError {
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: projects::path(&["Rows"]),
            }),
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope },
        }),
    }
}
pub(super) fn expression_error(
    node: u32,
    reason: codegen::OwnedSourceCapabilityReason,
) -> codegen::ProgramValidationError {
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
                scope: Some(route()),
            },
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}
pub(super) fn diagnostic(error: codegen::ProgramValidationError) -> codegen::Diagnostic {
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("owned error")
    };
    codegen::Diagnostic::OwnedSource { diagnostic }
}
fn validation(location: &str, message: &str) -> codegen::Diagnostic {
    codegen::Diagnostic::Validation {
        location: location.into(),
        message: message.into(),
    }
}
fn tree_schema() -> SchemaNode {
    SchemaNode::group(
        "Tree",
        vec![
            string("Name"),
            SchemaNode::group("Values", vec![string("Name")]).repeating(),
            SchemaNode::recursive_group("Children", "Tree").repeating(),
        ],
    )
}
pub fn projects() -> Vec<(&'static str, Project, Vec<codegen::Diagnostic>)> {
    let mut controls = Vec::new();
    for (id, name) in [("E21-unknown-owner", "Unknown"), ("E21-trimmed-owner", "Røws💠"), ("E21-case-owner", "rows")] {
        let mut p = projects::base();
        p.graph
            .nodes
            .insert(1, projects::field(name, &["Rows", "Start"], Some(&["Rows"])));
        controls.push((
            id,
            p,
            vec![validation(
                "graph node 1",
                &format!("unknown source owner `{name}`"),
            )],
        ));
    }
    for (mode, id) in [(1, "Q02-ending"), (2, "Q02-blocks")] {
        let mut p = projects::base();
        let g = &mut p.root.children[0];
        g.group_starting_with = None;
        match mode {
            1 => g.group_ending_with = Some(901),
            _ => g.group_into_blocks = Some(912),
        }
        controls.push((id, p, vec![diagnostic(grouping_error())]));
    }
    for (mode, id, message) in [
        (
            0,
            "Q03-post-without-group",
            "post-group filter requires one grouping control",
        ),
        (
            1,
            "Q03-conflicting-By-Starting",
            "scope grouping modes are mutually exclusive",
        ),
        (
            2,
            "Q03-missing-predicate",
            "group-starting-with predicate references missing node 99",
        ),
        (
            3,
            "Q03-missing-post",
            "post-group filter references missing node 99",
        ),
        (4, "Q03-conflicting-Adjacent-Starting", "scope grouping modes are mutually exclusive"),
    ] {
        let mut p = projects::base();
        let g = &mut p.root.children[0];
        match mode {
            0 => {
                g.group_starting_with = None;
                g.post_group_filter = Some(901);
            }
            1 => g.group_by = Some(1),
            2 => g.group_starting_with = Some(99),
            3 => g.post_group_filter = Some(99),
            _ => g.group_adjacent_by = Some(1),
        }
        controls.push((id, p, vec![validation("scope `Groups`", message)]));
    }
    let mut source_free = projects::base();
    source_free.root.children[0].iteration = ScopeIteration::None;
    controls.push((
        "Q03-source-free",
        source_free,
        vec![validation(
            "scope `Groups`",
            "group-starting-with predicate has no iterated source",
        )],
    ));
    for (id, node, reason) in [
        (
            "Q04-project-aggregate",
            Node::Aggregate {
                function: mapping::AggregateOp::Count,
                collection: projects::path(&["Rows"]),
                value: vec![],
                expression: None,
                arg: None,
            },
            codegen::OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            "Q04-project-lookup",
            Node::Lookup {
                collection: projects::path(&["Rows"]),
                key: projects::path(&["Key"]),
                matches: 911,
                value: projects::path(&["Label"]),
            },
            codegen::OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            "Q04-project-find",
            Node::CollectionFind {
                collection: projects::path(&["Rows"]),
                predicate: 901,
                value: 2,
            },
            codegen::OwnedSourceCapabilityReason::AggregateLookup,
        ),
        (
            "Q04-project-exists",
            Node::SequenceExists {
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 912,
                    item: 990,
                },
                predicate: 901,
            },
            codegen::OwnedSourceCapabilityReason::GeneratedItem,
        ),
        (
            "Q04-project-sequence-aggregate",
            Node::SequenceAggregate {
                function: mapping::AggregateOp::Count,
                sequence: mapping::SequenceExpr::Generate {
                    from: None,
                    to: 912,
                    item: 990,
                },
                predicate: None,
                expression: None,
                arg: None,
            },
            codegen::OwnedSourceCapabilityReason::GeneratedItem,
        ),
    ] {
        let mut p = projects::base();
        p.graph.nodes.insert(
            990,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
        p.graph.nodes.insert(991, node);
        p.root.children[0].bindings[0].node = 991;
        controls.push((id, p, vec![diagnostic(expression_error(991, reason))]));
    }
    let mut p = projects::base();
    p.root.children[0].children[0].construction = mapping::ScopeConstruction::CopyCurrentSource;
    p.root.children[0].children[0].bindings.clear();
    let kind = projects::row_schema().kind;
    let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    children[3].kind = kind;
    controls.push((
        "Q05-project-copy",
        p,
        vec![diagnostic(iteration_error(
            route().descendant(mapping::SourceScopeStep::Child(0)),
            codegen::OwnedSourceCapabilityReason::SpecialConstruction,
        ))],
    ));
    let mut p = projects::base();
    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
        unreachable!()
    };
    children.push(tree_schema());
    p.graph.nodes.insert(
        990,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    p.graph.nodes.insert(
        991,
        Node::SequenceItemAt {
            sequence: mapping::SequenceExpr::RecursiveCollect {
                collection: projects::path(&["Tree"]),
                children: projects::path(&["Children"]),
                descent_value: projects::path(&["Name"]),
                values: projects::path(&["Values"]),
                value: projects::path(&["Name"]),
                prefix: 923,
                separator: 900,
                item: 990,
            },
            index: 5,
        },
    );
    p.root.children[0].bindings[0].node = 991;
    controls.push((
        "Q05-project-recursive-ItemAt",
        p,
        vec![diagnostic(expression_error(
            991,
            codegen::OwnedSourceCapabilityReason::GeneratedItem,
        ))],
    ));
    controls
}
pub fn programs(
    base: &codegen::Program,
) -> Vec<(
    &'static str,
    codegen::Program,
    codegen::ProgramValidationError,
)> {
    use codegen::{Expression as E, GeneratedSequence as G, OwnedSourceCapabilityReason as R};
    // A01 lowering prunes unused graph nodes. Direct-Program probes supply
    // their own valid dependencies so capability gates, rather than accidental
    // missing-node errors, decide the outcome. Node 99 remains intentionally absent.
    let mut seeded = base.clone();
    let mut dependencies = vec![(
        3,
        E::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: projects::path(&["Rows", "Keep"]),
            },
            frame: Some(projects::path(&["Rows"])),
        }),
    )];
    dependencies.extend(
        [
            (900, Value::String("unused.json".into())),
            (901, Value::Bool(true)),
            (911, Value::Int(1)),
            (912, Value::Int(2)),
            (913, Value::Int(3)),
            (923, Value::String("audit".into())),
        ]
        .into_iter()
        .map(|(id, value)| (id, E::Const { value })),
    );
    for (id, expression) in dependencies {
        if let Some(existing) = seeded.expressions.iter().find(|node| node.id == id) {
            assert_eq!(existing.expression, expression, "authored dependency {id}");
        } else {
            seeded
                .expressions
                .push(codegen::ExpressionNode { id, expression });
        }
    }
    seeded.expressions.sort_by_key(|node| node.id);
    assert!(!seeded.expressions.iter().any(|node| node.id == 99));
    let base = &seeded;
    let mut controls = Vec::new();
    for (id, grouping) in [
        (
            "Q02-direct-ending",
            codegen::GroupingPlan::EndingWith { predicate: 901 },
        ),
        (
            "Q02-direct-blocks",
            codegen::GroupingPlan::IntoBlocks { size: 912 },
        ),
    ] {
        let mut p = base.clone();
        p.root.children[0].iteration = Some(
            codegen::IterationPlan::owned_source(mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: projects::path(&["Rows"]),
            })
            .with_grouping(grouping),
        );
        controls.push((id, p, grouping_error()));
    }
    for (id, key, post, expected) in [
        (
            "Q03-direct-missing-predicate",
            99,
            None,
            codegen::ProgramValidationError::MissingGroupingExpression {
                target_path: projects::path(&["Groups"]),
                role: codegen::GroupingExpressionRole::StartingPredicate,
                expression: 99,
            },
        ),
        (
            "Q03-direct-missing-post",
            1,
            Some(99),
            codegen::ProgramValidationError::MissingPostGroupFilterExpression {
                target_path: projects::path(&["Groups"]),
                expression: 99,
            },
        ),
    ] {
        let mut p = base.clone();
        let mut plan = codegen::IterationPlan::owned_source(mapping::OwnedSourceRef {
            owner: mapping::SourceOwner::Primary,
            path: projects::path(&["Rows"]),
        })
        .with_grouping(codegen::GroupingPlan::StartingWith { predicate:key });
        if let Some(post) = post {
            plan = plan.with_filtered_grouping(codegen::GroupingPlan::StartingWith { predicate:key }, post);
        }
        p.root.children[0].iteration = Some(plan);
        controls.push((id, p, expected));
    }
    for (id, expression, reason) in [
        (
            "Q04-aggregate",
            E::Aggregate {
                function: codegen::AggregateFunction::Count,
                collection: projects::path(&["Rows"]),
                value: codegen::AggregateValue::Path(vec![]),
                arg: None,
            },
            R::AggregateLookup,
        ),
        (
            "Q04-lookup",
            E::Lookup {
                collection: projects::path(&["Rows"]),
                key: projects::path(&["Key"]),
                matches: 911,
                value: projects::path(&["Label"]),
            },
            R::AggregateLookup,
        ),
        (
            "Q04-find",
            E::CollectionFind {
                collection: projects::path(&["Rows"]),
                predicate: 901,
                value: 2,
            },
            R::AggregateLookup,
        ),
        (
            "Q04-exists",
            E::SequenceExists {
                sequence: G::Range {
                    from: None,
                    to: 912,
                    item: 990,
                },
                predicate: 901,
            },
            R::GeneratedItem,
        ),
        (
            "Q04-sequence-aggregate",
            E::SequenceAggregate {
                function: codegen::AggregateFunction::Count,
                sequence: G::Range {
                    from: None,
                    to: 912,
                    item: 990,
                },
                predicate: None,
                expression: None,
                arg: None,
            },
            R::GeneratedItem,
        ),
    ] {
        let mut p = base.clone();
        p.expressions.push(codegen::ExpressionNode {
            id: 990,
            expression: E::SourceField {
                path: vec![],
                frame: None,
            },
        });
        p.expressions.push(codegen::ExpressionNode {
            id: 991,
            expression,
        });
        p.root.children[0].bindings[0].expression = 991;
        let mut nested = p.clone();
        nested.expressions.push(codegen::ExpressionNode {
            id: 992,
            expression: E::SourceField {
                path: vec![],
                frame: None,
            },
        });
        nested.expressions.push(codegen::ExpressionNode {
            id: 993,
            expression: E::SequenceItemAt {
                sequence: G::Range {
                    from: None,
                    to: 912,
                    item: 992,
                },
                index: 991,
            },
        });
        nested.root.children[0].bindings[0].expression = 993;
        let nested_id = match id {
            "Q04-aggregate" => "Q04-aggregate-under-ItemAt",
            "Q04-lookup" => "Q04-lookup-under-ItemAt",
            "Q04-find" => "Q04-find-under-ItemAt",
            "Q04-exists" => "Q04-exists-under-ItemAt",
            _ => "Q04-sequence-aggregate-under-ItemAt",
        };
        controls.push((id, p, expression_error(991, reason)));
        controls.push((nested_id, nested, expression_error(991, reason)));
    }
    let mut p = base.clone();
    p.root.children[0].children[0].iteration = Some(codegen::IterationPlan::generated(G::Range {
        from: None,
        to: 912,
        item: 990,
    }));
    p.expressions.push(codegen::ExpressionNode {
        id: 990,
        expression: E::SourceField {
            path: vec![],
            frame: None,
        },
    });
    controls.push((
        "Q05-generated-iteration",
        p,
        iteration_error(
            route().descendant(mapping::SourceScopeStep::Child(0)),
            R::GeneratedItem,
        ),
    ));
    let mut p = base.clone();
    p.root.children[0].children[0].construction = codegen::TargetConstruction::CopyCurrentSource;
    p.root.children[0].children[0].bindings.clear();
    let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    children[3].kind = projects::row_schema().kind;
    controls.push((
        "Q05-direct-copy",
        p,
        iteration_error(
            route().descendant(mapping::SourceScopeStep::Child(0)),
            R::SpecialConstruction,
        ),
    ));
    let mut p = base.clone();
    p.expressions
        .iter_mut()
        .find(|e| e.id == 1)
        .unwrap()
        .expression = E::OwnedSourceField(mapping::OwnedSourceField {
        source: mapping::OwnedSourceRef {
            owner: projects::owner("unavailableDynamic"),
            path: projects::path(&["Rows", "Start"]),
        },
        frame: Some(projects::path(&["Rows"])),
    });
    controls.push((
        "Q05-dynamic-owner",
        p,
        codegen::ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 1,
                    scope: Some(route()),
                },
                request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Named {
                        name: "unavailableDynamic".into(),
                    },
                    path: projects::path(&["Rows", "Start"]),
                    frame: Some(projects::path(&["Rows"])),
                }),
                reason: R::DynamicOwner,
                origin: codegen::OwnedSourceCapabilityOrigin::DynamicSource {
                    index: 3,
                    name: "unavailableDynamic".into(),
                },
            }),
        },
    ));
    let mut recursive = base.clone();
    let ir::SchemaKind::Group { children, .. } = &mut recursive.source.kind else {
        unreachable!()
    };
    children.push(tree_schema());
    recursive.expressions.push(codegen::ExpressionNode {
        id: 990,
        expression: E::SourceField {
            path: vec![],
            frame: None,
        },
    });
    recursive.expressions.push(codegen::ExpressionNode {
        id: 991,
        expression: E::SequenceItemAt {
            sequence: G::RecursiveCollect {
                collection: projects::path(&["Tree"]),
                children: projects::path(&["Children"]),
                descent_value: projects::path(&["Name"]),
                values: projects::path(&["Values"]),
                value: projects::path(&["Name"]),
                prefix: 923,
                separator: 900,
                item: 990,
            },
            index: 5,
        },
    });
    recursive.root.children[0].bindings[0].expression = 991;
    controls.push((
        "Q05-direct-recursive-ItemAt",
        recursive,
        expression_error(991, R::GeneratedItem),
    ));
    let mut filter_map = base.clone();
    for id in [990, 994] {
        filter_map.expressions.push(codegen::ExpressionNode {
            id,
            expression: E::SourceField {
                path: vec![],
                frame: None,
            },
        });
    }
    for (id, ty, output) in [
        (
            1,
            ScalarType::Bool,
            E::Const {
                value: Value::Bool(true),
            },
        ),
        (
            2,
            ScalarType::Int,
            E::FunctionParameter {
                parameter: mapping::FunctionParameterId::new(1),
            },
        ),
    ] {
        filter_map
            .user_functions
            .push(codegen::UserFunctionProgram {
                id: mapping::FunctionId::new(id),
                library: "corpus443".into(),
                name: format!("stage{id}"),
                parameters: vec![
                    codegen::UserFunctionParameter {
                        id: mapping::FunctionParameterId::new(1),
                        ty: ScalarType::Int,
                    },
                    codegen::UserFunctionParameter {
                        id: mapping::FunctionParameterId::new(2),
                        ty: ScalarType::Int,
                    },
                ],
                output_type: ty,
                expressions: vec![codegen::ExpressionNode {
                    id: 0,
                    expression: output,
                }],
                output: 0,
            });
    }
    filter_map.expressions.push(codegen::ExpressionNode {
        id: 991,
        expression: E::SequenceItemAt {
            sequence: G::FilterMapV1(codegen::GeneratedFilterMapV1 {
                source: codegen::GeneratedRange {
                    from: None,
                    to: 912,
                    item: 990,
                },
                item: 994,
                predicate: mapping::FunctionId::new(1),
                mapper: mapping::FunctionId::new(2),
                output_type: ScalarType::Int,
                captures: vec![],
            }),
            index: 5,
        },
    });
    filter_map.root.children[0].bindings[0].expression = 991;
    controls.push((
        "Q05-direct-FilterMap-ItemAt",
        filter_map,
        expression_error(991, R::GeneratedItem),
    ));
    // A failure rule has its own permission boundary; the target's orthogonal
    // ItemAt admission must not leak into either predicate or message.
    for (id, predicate) in [
        ("Q06-failure-predicate", true),
        ("Q06-failure-message", false),
    ] {
        let mut p = base.clone();
        p.expressions.push(codegen::ExpressionNode {
            id: 990,
            expression: E::SourceField {
                path: vec![],
                frame: None,
            },
        });
        p.expressions.push(codegen::ExpressionNode {
            id: 991,
            expression: E::SequenceItemAt {
                sequence: G::Range {
                    from: Some(911),
                    to: 913,
                    item: 990,
                },
                index: 5,
            },
        });
        p.failure_rules.push(codegen::FailureRule {
            iteration: codegen::FailureIteration::Source(
                codegen::IterationPlan::source(projects::path(&["Rows"]))
                    .source_iteration()
                    .unwrap()
                    .clone(),
            ),
            selection: codegen::FailureSelection::WhenTrue(if predicate { 991 } else { 3 }),
            message: (!predicate).then_some(991),
        });
        controls.push((
            id,
            p,
            codegen::ProgramValidationError::OwnedSource {
                diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                    location: codegen::OwnedSourceDiagnosticLocation::Expression {
                        namespace: codegen::OwnedSourceNamespace::Main,
                        node: if predicate { 5 } else { 3 },
                        scope: None,
                    },
                    request: Some(if predicate {
                        codegen::OwnedSourceDiagnosticRequest::Position {
                            owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                            collection: Some(projects::path(&["Rows"])),
                        }
                    } else {
                        codegen::OwnedSourceDiagnosticRequest::Field {
                            owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                            path: projects::path(&["Rows", "Keep"]),
                            frame: Some(projects::path(&["Rows"])),
                        }
                    }),
                    reason: R::FailureRule,
                    origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
                }),
            },
        ));
    }
    controls
}

/// Old profiles are independently checked against one complete literal error.
/// Returned results are retained before any comparison by the caller.
pub fn old_lower(project: &Project) -> Vec<Result<codegen::Program, codegen::LowerError>> {
    vec![
        codegen::lower(project),
        codegen::lower_with_basic_owned_filters(project),
        codegen::lower_with_basic_owned_sort(project),
        codegen::lower_with_basic_owned_windows(project),
        codegen::lower_with_basic_owned_row_failures(project),
        codegen::lower_with_basic_owned_group_by(project),
        codegen::lower_with_basic_owned_sequence_item_at(project),
        codegen::lower_with_basic_owned_group_adjacent_by(project),
    ]
}
