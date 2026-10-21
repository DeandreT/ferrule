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

fn options() -> crate::Options {
    crate::Options {
        package_name: "owned-aggregates".into(),
        runtime_dependency: crate::RuntimeDependency::Path("../codegen-runtime".into()),
    }
}

#[test]
fn ordered_text_owned_emission_refuses_new_selectors() {
    for function in [mapping::AggregateOp::LastPresentText, mapping::AggregateOp::LastDistinctText] {
        let mut program = owned_aggregate_program();
        let node = program.expressions.iter_mut().find(|node| node.id == 2).unwrap();
        let codegen::Expression::OwnedAggregate(aggregate) = &mut node.expression else {
            panic!("expected owned aggregate");
        };
        aggregate.function = function;
        let result = crate::emit_with_basic_owned_collection_aggregates(&program, &options());
        assert!(matches!(&result, Err(crate::EmitError::InvalidProgram(error)) if *error == closed_aggregate()), "{result:?}");
    }
}

#[test]
fn decimal_sum_owned_emission_folds_each_value_immediately() {
    let mut program = owned_aggregate_program();
    let node = program
        .expressions
        .iter_mut()
        .find(|node| node.id == 2)
        .unwrap();
    let codegen::Expression::OwnedAggregate(aggregate) = &mut node.expression else {
        panic!("expected owned aggregate");
    };
    aggregate.function = mapping::AggregateOp::DecimalSum;
    let artifacts =
        crate::emit_with_basic_owned_collection_aggregates(&program, &options()).unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .unwrap();
    assert!(String::from_utf8_lossy(&source.contents).contains(
        "collect_aggregate_value(codegen_runtime::AggregateFunction::DecimalSum, &mut values,"
    ));
}

#[test]
fn owned_aggregates_render_item_values_before_the_parent_argument() {
    let program = owned_aggregate_program();
    let artifacts = crate::emit_with_basic_owned_collection_aggregates(&program, &options())
        .expect("explicit owned aggregates emit");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .expect("generated library");
    let sum = function(&source, "fn expression_2(");
    assert!(sum.contains(
        "context.walk_owned_source_cohort(SourceOwnerRef::Primary, &[\"Rows\"]).map_err(|error| error.with_owned_source_node(2))?"
    ));
    assert!(sum.contains(
        "item_context.owned_field(SourceOwnerRef::Primary, &[\"Rows\", \"Rank\"], None).map_err(|error| error.with_owned_source_node(2))?"
    ));
    assert!(sum.contains("let arg = None;"));
    assert!(sum.contains(
        "codegen_runtime::aggregate(codegen_runtime::AggregateFunction::Sum, &values, arg)"
    ));
    let join = function(&source, "fn expression_3(");
    let walk = join
        .find("context.walk_owned_source_cohort(SourceOwnerRef::Named(\"Ref\"), &[\"Rows\"])")
        .expect("walk");
    let value = join
        .find("values.push(expression_5(item_context)?);")
        .expect("item value");
    let arg = join
        .find("let arg = Some(expression_4(context)?);")
        .expect("parent argument");
    assert!(walk < value && value < arg, "{join}");
    // Count never reads its value path.
    let count = function(&source, "fn expression_6(");
    assert!(count.contains("let values = vec![Value::Null; cohort.len()];"));
    assert!(!count.contains("owned_field"));
    for result in [
        crate::emit_with_basic_owned_failure_sources(&program, &options()),
        crate::emit_with_basic_owned_group_into_blocks(&program, &options()),
        crate::emit_with_basic_owned_recursive_filter(&program, &options()),
        crate::emit_with_basic_owned_lookup_find(&program, &options()),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::InvalidProgram(error)) if *error == closed_aggregate()),
            "{result:?}"
        );
    }
    assert!(crate::emit(&program, &options()).is_err());
}
