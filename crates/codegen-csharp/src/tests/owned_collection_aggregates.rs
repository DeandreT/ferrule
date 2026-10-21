//! #447 owned collection aggregates render one exact owned walk.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn rows(owner: mapping::SourceOwner) -> mapping::OwnedSourceRef {
    mapping::OwnedSourceRef {
        owner,
        path: path(&["Rows"]),
    }
}
fn named() -> mapping::SourceOwner {
    mapping::SourceOwner::Named { name: "Ref".into() }
}
/// `Value` sums Primary ranks, `Other` joins computed named names with a
/// parent separator and `Count` counts named group items without reading them.
fn owned_aggregate_program() -> codegen::Program {
    let source = ir::SchemaNode::group(
        "Input",
        vec![
            ir::SchemaNode::group(
                "Rows",
                vec![
                    ir::SchemaNode::scalar("Name", ir::ScalarType::String),
                    ir::SchemaNode::scalar("Rank", ir::ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    );
    let bind = |target_field: &str, node| mapping::Binding {
        target_field: target_field.into(),
        node,
    };
    let aggregate = |function, owner, value, arg| {
        mapping::Node::OwnedAggregate(mapping::OwnedAggregate {
            function,
            collection: rows(owner),
            value,
            arg,
        })
    };
    let project = mapping::Project {
        source: source.clone(),
        target: ir::SchemaNode::group(
            "Output",
            vec![
                ir::SchemaNode::scalar("Value", ir::ScalarType::String),
                ir::SchemaNode::scalar("Other", ir::ScalarType::String),
                ir::SchemaNode::scalar("Count", ir::ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: source,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        root: mapping::Scope {
            bindings: vec![bind("Value", 2), bind("Other", 3), bind("Count", 6)],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: [
                (
                    2,
                    aggregate(
                        mapping::AggregateOp::Sum,
                        mapping::SourceOwner::Primary,
                        mapping::OwnedAggregateValue::Path(path(&["Rank"])),
                        None,
                    ),
                ),
                (
                    3,
                    aggregate(
                        mapping::AggregateOp::Join,
                        named(),
                        mapping::OwnedAggregateValue::Expression(5),
                        Some(4),
                    ),
                ),
                (
                    4,
                    mapping::Node::Const {
                        value: ir::Value::String(",".into()),
                    },
                ),
                (
                    5,
                    mapping::Node::OwnedSourceField(mapping::OwnedSourceField {
                        source: mapping::OwnedSourceRef {
                            owner: named(),
                            path: path(&["Rows", "Name"]),
                        },
                        frame: None,
                    }),
                ),
                (
                    6,
                    aggregate(
                        mapping::AggregateOp::Count,
                        named(),
                        mapping::OwnedAggregateValue::Path(Vec::new()),
                        None,
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        },
        failure_rules: Vec::new(),
    };
    codegen::lower_with_basic_owned_collection_aggregates(&project).expect("owned aggregates lower")
}
fn closed_aggregate() -> codegen::ProgramValidationError {
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 2,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::AggregateLookup,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 2,
            },
        }),
    }
}
/// The body of one generated function, from its signature to the next item.
fn function<'a>(source: &'a str, start: &str) -> &'a str {
    let body = &source[source.find(start).expect("rendered function")..];
    &body[..body[1..].find("\n}").map_or(body.len(), |end| end + 2)]
}

#[test]
fn decimal_sum_stays_closed_for_owned_emission() {
    assert_owned_reducer_refused(mapping::AggregateOp::DecimalSum);
}

#[test]
fn ordered_text_owned_emission_refuses_new_selectors() {
    for function in [
        mapping::AggregateOp::LastPresentText,
        mapping::AggregateOp::LastDistinctText,
    ] {
        assert_owned_reducer_refused(function);
    }
}

fn assert_owned_reducer_refused(function: mapping::AggregateOp) {
    let mut program = owned_aggregate_program();
    let node = program
        .expressions
        .iter_mut()
        .find(|node| node.id == 2)
        .unwrap();
    let codegen::Expression::OwnedAggregate(aggregate) = &mut node.expression else {
        panic!("expected owned aggregate");
    };
    aggregate.function = function;
    let result = crate::emit_with_basic_owned_collection_aggregates(&program);
    assert!(
        matches!(&result, Err(crate::EmitError::ProgramValidation(error)) if *error == closed_aggregate()),
        "{result:?}"
    );
}

#[test]
fn owned_aggregates_render_kept_alive_cohorts_before_the_parent_argument() {
    let program = owned_aggregate_program();
    let artifacts = crate::emit_with_basic_owned_collection_aggregates(&program)
        .expect("explicit owned aggregates emit");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str().ends_with("GeneratedMapping.cs"))
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .expect("generated mapping");
    let sum = function(&source, "FerruleValue NodeBody_2(");
    assert!(sum.contains(
        "cohort_2 = context.WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary"
    ));
    assert!(sum.contains("item_context_2.ResolveOwnedField("));
    assert!(sum.contains("error_2.WithOwnedSourceNode(2U)"));
    assert!(sum.contains("finally { global::System.GC.KeepAlive(cohort_2); }"));
    assert!(sum.contains("argument_2 = null;"));
    assert!(sum.contains("FerruleAggregateOperation.Sum, values_2, argument_2"));
    let join = function(&source, "FerruleValue NodeBody_3(");
    let walk = join
        .find("FerruleOwnedSourceOwner.Named(\"Ref\")")
        .expect("walk");
    let value = join
        .find("values_3.Add(Node_5(candidate_3.Context));")
        .expect("item value");
    let arg = join
        .find("argument_3 = Node_4(context);")
        .expect("parent argument");
    assert!(walk < value && value < arg, "{join}");
    // Count never reads its value path.
    let count = function(&source, "FerruleValue NodeBody_6(");
    assert!(count.contains("values_6.Add(global::Ferrule.Runtime.FerruleValue.Null);"));
    assert!(!count.contains("ResolveOwnedField"));
    for result in [
        crate::emit_with_basic_owned_failure_sources(&program),
        crate::emit_with_basic_owned_group_into_blocks(&program),
        crate::emit_with_basic_owned_recursive_filter(&program),
        crate::emit_with_basic_owned_lookup_find(&program),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::ProgramValidation(error)) if *error == closed_aggregate()),
            "{result:?}"
        );
    }
    assert!(crate::emit(&program).is_err());
}
