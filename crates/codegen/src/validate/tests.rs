use ir::{ScalarType, SchemaNode, Value};

use super::*;
use crate::{
    AggregateFunction, AggregateValue, Binding, ExpressionNode, GeneratedSequence, IterationPlan,
    ScalarFunction, SequenceWindow, SortFilterOrder, SortKey, SortPlan, SourceIteration,
};

fn program() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Rows", Vec::new()).repeating()],
        ),
        target: SchemaNode::group("Target", Vec::new()),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Call {
                    function: ScalarFunction::Add,
                    args: vec![1, 1],
                },
            },
        ],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            bindings: vec![Binding {
                target_field: "Value".into(),
                expression: 2,
                target_type: ScalarType::Int,
                repeating: false,
            }],
            children: Vec::new(),
        },
    }
}

#[test]
fn accepts_valid_repeating_duplicate_bindings() {
    let mut program = program();
    program.root.bindings = vec![
        Binding {
            target_field: "Values".into(),
            expression: 1,
            target_type: ScalarType::Int,
            repeating: true,
        },
        Binding {
            target_field: "Values".into(),
            expression: 2,
            target_type: ScalarType::Int,
            repeating: true,
        },
    ];

    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn accepts_empty_and_named_source_iterations() {
    let mut program = program();
    program.root.iteration = Some(IterationPlan::source(Vec::new()));
    assert_eq!(validate_program(&program), Ok(()));

    program.root.iteration = Some(IterationPlan::source(vec!["Rows".into()]));
    assert_eq!(validate_program(&program), Ok(()));

    program.root.iteration = Some(IterationPlan::source(vec!["Missing".into()]));
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidSourceIteration {
            target_path: Vec::new(),
            source_path: vec!["Missing".into()],
        })
    );
}

#[test]
fn validates_generated_sequence_references_and_item_shape() {
    let generated = |item| {
        IterationPlan::generated(GeneratedSequence::Tokenize {
            input: 1,
            delimiter: 2,
            item,
        })
    };

    let mut valid = program();
    valid.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    valid.root.iteration = Some(generated(3));
    assert_eq!(validate_program(&valid), Ok(()));

    let mut missing_input = valid.clone();
    missing_input.root.iteration = Some(IterationPlan::generated(
        GeneratedSequence::TokenizeByLength {
            input: 99,
            length: 2,
            item: 3,
        },
    ));
    assert_eq!(
        validate_program(&missing_input),
        Err(ProgramValidationError::MissingSequenceExpression {
            target_path: Vec::new(),
            role: SequenceExpressionRole::Input(0),
            expression: 99,
        })
    );

    let mut missing_item = program();
    missing_item.root.iteration = Some(generated(99));
    assert_eq!(
        validate_program(&missing_item),
        Err(ProgramValidationError::MissingSequenceExpression {
            target_path: Vec::new(),
            role: SequenceExpressionRole::Item,
            expression: 99,
        })
    );

    let mut wrong_item = program();
    wrong_item.root.iteration = Some(generated(2));
    assert_eq!(
        validate_program(&wrong_item),
        Err(ProgramValidationError::InvalidSequenceItem {
            target_path: Vec::new(),
            expression: 2,
        })
    );
}

#[test]
fn generated_items_are_unique_and_lexically_scoped() {
    let sequence = |input, item| {
        IterationPlan::generated(GeneratedSequence::Range {
            from: Some(input),
            to: 2,
            item,
        })
    };
    let child = |name: &str, iteration: IterationPlan, binding: Option<NodeId>| TargetScope {
        target_field: name.into(),
        repeating: true,
        iteration: Some(iteration),
        bindings: binding
            .into_iter()
            .map(|expression| Binding {
                target_field: "Value".into(),
                expression,
                target_type: ScalarType::Int,
                repeating: false,
            })
            .collect(),
        children: Vec::new(),
    };
    let item = |id| ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    };

    let mut duplicate = program();
    duplicate.expressions.push(item(3));
    duplicate.root.children = vec![
        child("First", sequence(1, 3), None),
        child("Second", sequence(1, 3), None),
    ];
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateSequenceItem {
            target_path: vec!["Second".into()],
            first_target_path: vec!["First".into()],
            expression: 3,
        })
    );

    let mut sibling_leak = program();
    sibling_leak.expressions.extend([item(3), item(4)]);
    sibling_leak.root.children = vec![
        child("First", sequence(1, 3), Some(3)),
        child("Second", sequence(1, 4), Some(3)),
    ];
    assert_eq!(
        validate_program(&sibling_leak),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            target_path: vec!["Second".into()],
            expression: 3,
            item: 3,
        })
    );

    let mut own_input = program();
    own_input.expressions.push(item(3));
    own_input.root.iteration = Some(sequence(3, 3));
    assert_eq!(
        validate_program(&own_input),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            target_path: Vec::new(),
            expression: 3,
            item: 3,
        })
    );

    let mut nested = program();
    nested.expressions.extend([item(3), item(4)]);
    nested.root.iteration = Some(sequence(1, 3));
    nested.root.children = vec![child("Nested", sequence(3, 4), Some(4))];
    assert_eq!(validate_program(&nested), Ok(()));
}

#[test]
fn accepts_framed_fields_positions_and_source_filters() {
    let mut program = program();
    program.expressions.extend([
        ExpressionNode {
            id: 3,
            expression: Expression::SourceField {
                frame: Some(Vec::new()),
                path: vec!["Value".into()],
            },
        },
        ExpressionNode {
            id: 4,
            expression: Expression::Position {
                collection: vec!["Rows".into()],
            },
        },
        ExpressionNode {
            id: 5,
            expression: Expression::Position {
                collection: Vec::new(),
            },
        },
        ExpressionNode {
            id: 6,
            expression: Expression::Const {
                value: Value::Bool(true),
            },
        },
    ]);
    program.root.iteration = Some(IterationPlan::new(
        SourceIteration::new(vec!["Rows".into()]),
        Some(6),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));

    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn rejects_invalid_filters_at_the_exact_target_path() {
    let child = |filter| TargetScope {
        target_field: "Child".into(),
        repeating: true,
        iteration: Some(IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            filter,
            None,
            Vec::new(),
            IterationOutput::Repeated,
        )),
        bindings: Vec::new(),
        children: Vec::new(),
    };

    let mut missing = program();
    missing.root.children.push(child(Some(99)));
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingFilterExpression {
            target_path: vec!["Child".into()],
            expression: 99,
        })
    );
}

#[test]
fn validates_sort_window_and_iteration_output_controls() {
    let child = |iteration, repeating| TargetScope {
        target_field: "Child".into(),
        repeating,
        iteration: Some(iteration),
        bindings: Vec::new(),
        children: Vec::new(),
    };
    let sort = |then| {
        SortPlan::new(
            SortKey {
                expression: 1,
                descending: false,
            },
            then,
            SortFilterOrder::SortThenFilter,
        )
    };

    let mut missing_sort = program();
    missing_sort.root.children.push(child(
        IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            None,
            Some(sort(vec![SortKey {
                expression: 99,
                descending: true,
            }])),
            Vec::new(),
            IterationOutput::Repeated,
        ),
        true,
    ));
    assert_eq!(
        validate_program(&missing_sort),
        Err(ProgramValidationError::MissingSortExpression {
            target_path: vec!["Child".into()],
            key: 1,
            expression: 99,
        })
    );

    let mut missing_window = program();
    missing_window.root.children.push(child(
        IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            None,
            None,
            vec![SequenceWindow::FromTo { first: 1, last: 99 }],
            IterationOutput::Repeated,
        ),
        true,
    ));
    assert_eq!(
        validate_program(&missing_window),
        Err(ProgramValidationError::MissingWindowExpression {
            target_path: vec!["Child".into()],
            window: 0,
            bound: 1,
            expression: 99,
        })
    );

    let mut invalid_first = program();
    invalid_first.root.children.push(child(
        IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            None,
            None,
            Vec::new(),
            IterationOutput::First,
        ),
        true,
    ));
    assert_eq!(
        validate_program(&invalid_first),
        Err(ProgramValidationError::InvalidIterationOutput {
            target_path: vec!["Child".into()],
            output: IterationOutput::First,
        })
    );

    let mut mapped_root = program();
    mapped_root.root.iteration = Some(IterationPlan::new(
        SourceIteration::new(Vec::new()),
        None,
        None,
        Vec::new(),
        IterationOutput::MappedSequence,
    ));
    assert_eq!(
        validate_program(&mapped_root),
        Err(ProgramValidationError::InvalidIterationOutput {
            target_path: Vec::new(),
            output: IterationOutput::MappedSequence,
        })
    );

    let mut scalar_first = program();
    scalar_first.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Child", ScalarType::String)],
    );
    scalar_first.root.children.push(child(
        IterationPlan::new(
            SourceIteration::new(vec!["Rows".into()]),
            None,
            None,
            Vec::new(),
            IterationOutput::First,
        ),
        false,
    ));
    assert_eq!(
        validate_program(&scalar_first),
        Err(ProgramValidationError::InvalidIterationOutput {
            target_path: vec!["Child".into()],
            output: IterationOutput::First,
        })
    );

    let mut group_first = scalar_first;
    group_first.target = SchemaNode::group("Target", vec![SchemaNode::group("Child", Vec::new())]);
    assert_eq!(validate_program(&group_first), Ok(()));
}

#[test]
fn rejects_duplicate_and_missing_expressions() {
    let mut duplicate = program();
    duplicate.expressions.push(duplicate.expressions[0].clone());
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateExpression { node: 1 })
    );

    let mut missing = program();
    missing.expressions[1].expression = Expression::Call {
        function: ScalarFunction::Add,
        args: vec![1, 99],
    };
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );
}

#[test]
fn validates_aggregate_projection_and_argument_dependencies() {
    let aggregate = |value, arg| Expression::Aggregate {
        function: AggregateFunction::Sum,
        collection: vec!["Rows".into()],
        value,
        arg,
    };

    let mut missing_projection = program();
    missing_projection.expressions[1].expression =
        aggregate(AggregateValue::Expression(99), Some(98));
    assert_eq!(
        validate_program(&missing_projection),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    let mut missing_argument = program();
    missing_argument.expressions[1].expression = aggregate(AggregateValue::Expression(1), Some(99));
    assert_eq!(
        validate_program(&missing_argument),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    let mut cycle = program();
    cycle.expressions[1].expression = aggregate(AggregateValue::Expression(2), Some(1));
    assert_eq!(
        validate_program(&cycle),
        Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
    );
}

#[test]
fn validates_aggregate_collection_and_direct_value_paths() {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Amount", ScalarType::Int),
                    SchemaNode::group("Nested", Vec::new()),
                ],
            )
            .repeating(),
        ],
    );
    let aggregate = |collection: &[&str], value: &[&str]| Expression::Aggregate {
        function: AggregateFunction::Sum,
        collection: collection.iter().map(|segment| (*segment).into()).collect(),
        value: AggregateValue::Path(value.iter().map(|segment| (*segment).into()).collect()),
        arg: None,
    };

    program.expressions[1].expression = aggregate(&["Rows"], &["Amount"]);
    assert_eq!(validate_program(&program), Ok(()));

    program.expressions[1].expression = aggregate(&["Missing"], &["Amount"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidAggregateCollection {
            node: 2,
            collection: vec!["Missing".into()],
        })
    );

    program.expressions[1].expression = aggregate(&["Rows"], &["Missing"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidAggregateValuePath {
            node: 2,
            collection: vec!["Rows".into()],
            value: vec!["Missing".into()],
        })
    );

    program.expressions[1].expression = aggregate(&["Rows"], &["Nested"]);
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidAggregateValuePath { node: 2, .. })
    ));

    // Empty value paths are valid for count and sum because a scalar
    // collection item is used directly and a structural item becomes Null.
    program.expressions[1].expression = aggregate(&["Rows"], &[]);
    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn rejects_self_and_multi_expression_cycles() {
    let mut self_cycle = program();
    self_cycle.expressions[1].expression = Expression::Call {
        function: ScalarFunction::Add,
        args: vec![2, 1],
    };
    assert_eq!(
        validate_program(&self_cycle),
        Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
    );

    let mut multi_cycle = program();
    multi_cycle.expressions[0].expression = Expression::If {
        condition: 2,
        then: 2,
        else_: 2,
    };
    assert_eq!(
        validate_program(&multi_cycle),
        Err(ProgramValidationError::ExpressionCycle {
            cycle: vec![1, 2, 1],
        })
    );
}

#[test]
fn rejects_invalid_target_scope_states() {
    let mut missing = program();
    missing.root.bindings[0].expression = 99;
    assert!(matches!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingBindingExpression { expression: 99, .. })
    ));

    let mut duplicate_binding = program();
    duplicate_binding.root.bindings.push(Binding {
        target_field: "Value".into(),
        expression: 1,
        target_type: ScalarType::Int,
        repeating: false,
    });
    assert!(matches!(
        validate_program(&duplicate_binding),
        Err(ProgramValidationError::InvalidDuplicateBinding {
            first_binding: 0,
            duplicate_binding: 1,
            ..
        })
    ));

    let child = TargetScope {
        target_field: "Child".into(),
        repeating: false,
        iteration: None,
        bindings: Vec::new(),
        children: Vec::new(),
    };
    let mut duplicate_child = program();
    duplicate_child.root.children = vec![child.clone(), child.clone()];
    assert!(matches!(
        validate_program(&duplicate_child),
        Err(ProgramValidationError::DuplicateChildTarget {
            first_child: 0,
            duplicate_child: 1,
            ..
        })
    ));

    let mut collision = program();
    collision.root.bindings[0].target_field = "Child".into();
    collision.root.children.push(child);
    assert!(matches!(
        validate_program(&collision),
        Err(ProgramValidationError::BindingChildCollision {
            binding: 0,
            child: 0,
            ..
        })
    ));
}
