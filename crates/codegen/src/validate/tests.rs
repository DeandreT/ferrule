use ir::{ScalarType, SchemaNode, Value};

use super::*;
use crate::{
    AggregateFunction, AggregateValue, Binding, ExpressionNode, GeneratedSequence, IterationPlan,
    NamedTargetProgram, ScalarFunction, SequenceWindow, SortFilterOrder, SortKey, SortPlan,
    SourceIteration,
};

mod adjacency_tree;
mod collection_find;
mod collections;
mod concatenate;
mod failures;
mod grouping;
mod invariants;
mod joins;
mod path_hierarchy;
mod recursive_filter;
mod sources;
mod user_functions;
mod xml_mixed_content;
mod xml_serialize;

fn program() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Rows", Vec::new()).repeating()],
        ),
        extra_sources: Vec::new(),
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
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Value".into(),
                expression: 2,
                target_type: ScalarType::Int,
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

fn set_target_fields(program: &mut Program, fields: Vec<SchemaNode>) {
    program.target = SchemaNode::group("Target", fields);
}

fn empty_target_scope() -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    }
}

#[test]
fn validates_each_named_target_against_its_own_schema() {
    let mut program = program();
    program.extra_targets.push(NamedTargetProgram {
        name: "Scalar".into(),
        target: SchemaNode::scalar("Scalar", ScalarType::Int),
        root: empty_target_scope(),
    });

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::NamedTarget {
            target: "Scalar".into(),
            error: Box::new(
                ProgramValidationError::GroupConstructionRequiresGroupTarget {
                    target_path: Vec::new(),
                }
            ),
        })
    );
}

#[test]
fn generated_sequence_ownership_is_global_across_named_targets() {
    let generated = GeneratedSequence::Tokenize {
        input: 1,
        delimiter: 1,
        item: 3,
    };
    let mut program = program();
    program.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    program.root.iteration = Some(IterationPlan::generated(generated.clone()));
    let mut named_root = empty_target_scope();
    named_root.iteration = Some(IterationPlan::generated(generated));
    program.extra_targets.push(NamedTargetProgram {
        name: "Audit".into(),
        target: SchemaNode::group("Audit", Vec::new()),
        root: named_root,
    });

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::DuplicateSequenceItem {
            owner: SequenceOwner::NamedTargetScope {
                target: "Audit".into(),
                path: Vec::new(),
            },
            first_owner: SequenceOwner::Scope(Vec::new()),
            expression: 3,
        })
    );
}

#[test]
fn named_target_sequence_diagnostics_retain_the_target_name() {
    let mut program = program();
    program.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    let mut named_root = empty_target_scope();
    named_root.iteration = Some(IterationPlan::generated(
        GeneratedSequence::TokenizeByLength {
            input: 99,
            length: 1,
            item: 3,
        },
    ));
    program.extra_targets.push(NamedTargetProgram {
        name: "Audit".into(),
        target: SchemaNode::group("Audit", Vec::new()),
        root: named_root,
    });

    let error = validate_program(&program).expect_err("the named input is missing");
    assert_eq!(
        error,
        ProgramValidationError::NamedTarget {
            target: "Audit".into(),
            error: Box::new(ProgramValidationError::MissingSequenceExpression {
                owner: SequenceOwner::NamedTargetScope {
                    target: "Audit".into(),
                    path: Vec::new(),
                },
                role: SequenceExpressionRole::Input(0),
                expression: 99,
            }),
        }
    );
    assert!(
        error
            .to_string()
            .contains("named target `Audit` scope <root>")
    );
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
            owner: SequenceOwner::Scope(Vec::new()),
            role: SequenceExpressionRole::Input(0),
            expression: 99,
        })
    );

    let mut missing_recursive_separator = valid.clone();
    missing_recursive_separator.root.iteration = Some(IterationPlan::generated(
        GeneratedSequence::RecursiveCollect {
            collection: Vec::new(),
            children: vec!["children".into()],
            descent_value: vec!["name".into()],
            values: vec!["values".into()],
            value: vec!["name".into()],
            prefix: 1,
            separator: 99,
            item: 3,
        },
    ));
    assert_eq!(
        validate_program(&missing_recursive_separator),
        Err(ProgramValidationError::MissingSequenceExpression {
            owner: SequenceOwner::Scope(Vec::new()),
            role: SequenceExpressionRole::Input(1),
            expression: 99,
        })
    );

    let mut missing_item = program();
    missing_item.root.iteration = Some(generated(99));
    assert_eq!(
        validate_program(&missing_item),
        Err(ProgramValidationError::MissingSequenceExpression {
            owner: SequenceOwner::Scope(Vec::new()),
            role: SequenceExpressionRole::Item,
            expression: 99,
        })
    );

    let mut wrong_item = program();
    wrong_item.root.iteration = Some(generated(2));
    assert_eq!(
        validate_program(&wrong_item),
        Err(ProgramValidationError::InvalidSequenceItem {
            owner: SequenceOwner::Scope(Vec::new()),
            expression: 2,
        })
    );
}

#[test]
fn validates_every_recursive_sequence_schema_path() {
    let sequence = || GeneratedSequence::RecursiveCollect {
        collection: Vec::new(),
        children: vec!["children".into()],
        descent_value: vec!["name".into()],
        values: vec!["files".into()],
        value: vec!["name".into()],
        prefix: 1,
        separator: 2,
        item: 3,
    };
    let mut valid = program();
    valid.source = SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group(
                "files",
                vec![SchemaNode::scalar("name", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::recursive_group("children", "Directory").repeating(),
        ],
    );
    valid.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    valid.root.iteration = Some(IterationPlan::generated(sequence()));
    assert_eq!(validate_program(&valid), Ok(()));

    let assert_invalid =
        |sequence: GeneratedSequence, role: RecursiveSequencePathRole, path: Vec<String>| {
            let mut invalid = valid.clone();
            invalid.root.iteration = Some(IterationPlan::generated(sequence));
            assert_eq!(
                validate_program(&invalid),
                Err(ProgramValidationError::InvalidRecursiveSequencePath {
                    owner: SequenceOwner::Scope(Vec::new()),
                    role,
                    path,
                })
            );
        };

    let mut invalid = sequence();
    let GeneratedSequence::RecursiveCollect { collection, .. } = &mut invalid else {
        unreachable!();
    };
    *collection = vec!["missing".into()];
    assert_invalid(
        invalid,
        RecursiveSequencePathRole::Collection,
        vec!["missing".into()],
    );

    let mut invalid = sequence();
    let GeneratedSequence::RecursiveCollect { children, .. } = &mut invalid else {
        unreachable!();
    };
    *children = vec!["missing".into()];
    assert_invalid(
        invalid,
        RecursiveSequencePathRole::Children,
        vec!["missing".into()],
    );

    let mut invalid = sequence();
    let GeneratedSequence::RecursiveCollect { descent_value, .. } = &mut invalid else {
        unreachable!();
    };
    *descent_value = vec!["missing".into()];
    assert_invalid(
        invalid,
        RecursiveSequencePathRole::DescentValue,
        vec!["missing".into()],
    );

    let mut invalid = sequence();
    let GeneratedSequence::RecursiveCollect { values, .. } = &mut invalid else {
        unreachable!();
    };
    *values = vec!["missing".into()];
    assert_invalid(
        invalid,
        RecursiveSequencePathRole::Values,
        vec!["missing".into()],
    );

    let mut invalid = sequence();
    let GeneratedSequence::RecursiveCollect { value, .. } = &mut invalid else {
        unreachable!();
    };
    *value = vec!["missing".into()];
    assert_invalid(
        invalid,
        RecursiveSequencePathRole::Value,
        vec!["missing".into()],
    );
}

#[test]
fn validates_scalar_scope_construction() {
    let mut valid = program();
    valid.target = SchemaNode::scalar("Target", ScalarType::Int);
    valid.root.construction = TargetConstruction::Scalar { expression: 1 };
    valid.root.bindings.clear();
    assert_eq!(validate_program(&valid), Ok(()));

    let mut missing = valid.clone();
    missing.root.construction = TargetConstruction::Scalar { expression: 99 };
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingScalarExpression {
            target_path: Vec::new(),
            expression: 99,
        })
    );

    let mut wrong_target = valid.clone();
    wrong_target.target = SchemaNode::group("Target", Vec::new());
    assert_eq!(
        validate_program(&wrong_target),
        Err(
            ProgramValidationError::ScalarConstructionRequiresScalarTarget {
                target_path: Vec::new(),
            }
        )
    );

    let mut content = valid;
    content.root.bindings.push(Binding {
        target_field: "Value".into(),
        expression: 1,
        target_type: ScalarType::Int,
        repeating: false,
    });
    assert_eq!(
        validate_program(&content),
        Err(ProgramValidationError::ScalarConstructionHasContent {
            target_path: Vec::new(),
        })
    );
}

#[test]
fn validates_copy_current_source_construction() {
    let source = SchemaNode::group("Source", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    let mut target = source.clone();
    target.name = "Target".into();

    let mut valid = program();
    valid.source = source;
    valid.target = target;
    valid.root.construction = TargetConstruction::CopyCurrentSource;
    valid.root.bindings.clear();
    assert_eq!(validate_program(&valid), Ok(()));

    let mut scalar_source = valid.clone();
    scalar_source.source = SchemaNode::scalar("Source", ScalarType::Int);
    assert_eq!(
        validate_program(&scalar_source),
        Err(
            ProgramValidationError::CopyConstructionRequiresGroupSource {
                target_path: Vec::new(),
            }
        )
    );

    let mut scalar_target = valid.clone();
    scalar_target.target = SchemaNode::scalar("Target", ScalarType::Int);
    assert_eq!(
        validate_program(&scalar_target),
        Err(
            ProgramValidationError::CopyConstructionRequiresGroupTarget {
                target_path: Vec::new(),
            }
        )
    );

    let mut mismatched = valid.clone();
    mismatched.target =
        SchemaNode::group("Target", vec![SchemaNode::scalar("Other", ScalarType::Int)]);
    assert_eq!(
        validate_program(&mismatched),
        Err(
            ProgramValidationError::CopyConstructionRequiresMatchingGroups {
                target_path: Vec::new(),
            }
        )
    );

    let mut content = valid.clone();
    content.root.bindings.push(Binding {
        target_field: "Value".into(),
        expression: 1,
        target_type: ScalarType::Int,
        repeating: false,
    });
    assert_eq!(
        validate_program(&content),
        Err(ProgramValidationError::CopyConstructionHasContent {
            target_path: Vec::new(),
        })
    );

    let mut generated = valid;
    generated.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    generated.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 1,
        item: 3,
    }));
    assert_eq!(
        validate_program(&generated),
        Err(
            ProgramValidationError::CopyConstructionRequiresGroupSource {
                target_path: Vec::new(),
            }
        )
    );
}

#[test]
fn rejects_missing_target_scopes_and_cardinality_mismatches() {
    let child = |repeating| TargetScope {
        target_field: "Child".into(),
        repeating,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    };

    let mut missing = program();
    missing.root.children.push(child(false));
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingTargetScope {
            target_path: vec!["Child".into()],
        })
    );

    let mut repeated_schema = program();
    set_target_fields(
        &mut repeated_schema,
        vec![SchemaNode::group("Child", Vec::new()).repeating()],
    );
    repeated_schema.root.children.push(child(false));
    assert_eq!(
        validate_program(&repeated_schema),
        Err(ProgramValidationError::TargetCardinalityMismatch {
            target_path: vec!["Child".into()],
            scope_repeating: false,
            target_repeating: true,
        })
    );

    let mut singular_schema = program();
    set_target_fields(
        &mut singular_schema,
        vec![SchemaNode::group("Child", Vec::new())],
    );
    singular_schema.root.children.push(child(true));
    assert_eq!(
        validate_program(&singular_schema),
        Err(ProgramValidationError::TargetCardinalityMismatch {
            target_path: vec!["Child".into()],
            scope_repeating: true,
            target_repeating: false,
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
        construction: TargetConstruction::Group,
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
    set_target_fields(
        &mut duplicate,
        vec![
            SchemaNode::group("First", Vec::new()).repeating(),
            SchemaNode::group("Second", Vec::new()).repeating(),
        ],
    );
    duplicate.expressions.push(item(3));
    duplicate.root.children = vec![
        child("First", sequence(1, 3), None),
        child("Second", sequence(1, 3), None),
    ];
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateSequenceItem {
            owner: SequenceOwner::Scope(vec!["Second".into()]),
            first_owner: SequenceOwner::Scope(vec!["First".into()]),
            expression: 3,
        })
    );

    let mut sibling_leak = program();
    set_target_fields(
        &mut sibling_leak,
        vec![
            SchemaNode::group("First", Vec::new()).repeating(),
            SchemaNode::group("Second", Vec::new()).repeating(),
        ],
    );
    sibling_leak.expressions.extend([item(3), item(4)]);
    sibling_leak.root.children = vec![
        child("First", sequence(1, 3), Some(3)),
        child("Second", sequence(1, 4), Some(3)),
    ];
    assert_eq!(
        validate_program(&sibling_leak),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Scope(vec!["Second".into()]),
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
            owner: SequenceOwner::Scope(Vec::new()),
            expression: 3,
            item: 3,
        })
    );

    let mut nested = program();
    set_target_fields(
        &mut nested,
        vec![SchemaNode::group("Nested", Vec::new()).repeating()],
    );
    nested.expressions.extend([item(3), item(4)]);
    nested.root.iteration = Some(sequence(1, 3));
    nested.root.children = vec![child("Nested", sequence(3, 4), Some(4))];
    assert_eq!(validate_program(&nested), Ok(()));
}

#[test]
fn reducer_items_are_private_unique_and_boundary_aware() {
    let item = |id| ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    };
    let exists = |item, predicate| Expression::SequenceExists {
        sequence: GeneratedSequence::Range {
            from: None,
            to: 1,
            item,
        },
        predicate,
    };

    let mut valid = program();
    valid.expressions.extend([
        item(3),
        ExpressionNode {
            id: 4,
            expression: Expression::Call {
                function: ScalarFunction::Equal,
                args: vec![3, 1],
            },
        },
        ExpressionNode {
            id: 5,
            expression: exists(3, 4),
        },
    ]);
    valid.root.bindings[0].expression = 5;
    assert_eq!(validate_program(&valid), Ok(()));

    let mut own_input = valid.clone();
    let Some(ExpressionNode {
        expression: Expression::SequenceExists { sequence, .. },
        ..
    }) = own_input.expressions.iter_mut().find(|node| node.id == 5)
    else {
        panic!("expected sequence-exists expression");
    };
    *sequence = GeneratedSequence::Range {
        from: None,
        to: 3,
        item: 3,
    };
    assert_eq!(
        validate_program(&own_input),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(5),
            expression: 5,
            item: 3,
        })
    );

    let mut leaked = valid;
    leaked.root.bindings.push(Binding {
        target_field: "Leaked".into(),
        expression: 3,
        target_type: ScalarType::Int,
        repeating: false,
    });
    assert_eq!(
        validate_program(&leaked),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Scope(Vec::new()),
            expression: 3,
            item: 3,
        })
    );
}

#[test]
fn reducer_items_share_global_ownership_with_scope_sequences() {
    let mut program = program();
    program.expressions.extend([
        ExpressionNode {
            id: 3,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        },
        ExpressionNode {
            id: 4,
            expression: Expression::SequenceItemAt {
                sequence: GeneratedSequence::Range {
                    from: None,
                    to: 1,
                    item: 3,
                },
                index: 1,
            },
        },
    ]);
    program.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 2,
        item: 3,
    }));

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::DuplicateSequenceItem {
            owner: SequenceOwner::Scope(Vec::new()),
            first_owner: SequenceOwner::Expression(4),
            expression: 3,
        })
    );
}

#[test]
fn nested_reducers_can_read_active_scope_items_but_not_foreign_private_items() {
    let item = |id| ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    };
    let mut nested = program();
    nested.expressions.extend([
        item(3),
        item(4),
        ExpressionNode {
            id: 5,
            expression: Expression::SequenceExists {
                sequence: GeneratedSequence::Range {
                    from: Some(3),
                    to: 2,
                    item: 4,
                },
                predicate: 4,
            },
        },
    ]);
    nested.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 2,
        item: 3,
    }));
    nested.root.bindings[0].expression = 5;
    assert_eq!(validate_program(&nested), Ok(()));

    let mut shadowed = nested.clone();
    let Some(ExpressionNode {
        expression: Expression::SequenceExists { predicate, .. },
        ..
    }) = shadowed.expressions.iter_mut().find(|node| node.id == 5)
    else {
        panic!("expected sequence-exists expression");
    };
    *predicate = 3;
    assert_eq!(
        validate_program(&shadowed),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(5),
            expression: 5,
            item: 3,
        })
    );

    let mut item_at_outer = nested.clone();
    item_at_outer.expressions.extend([
        item(6),
        ExpressionNode {
            id: 7,
            expression: Expression::SequenceItemAt {
                sequence: GeneratedSequence::Range {
                    from: Some(3),
                    to: 2,
                    item: 6,
                },
                index: 1,
            },
        },
    ]);
    item_at_outer.root.bindings[0].expression = 7;
    assert_eq!(
        validate_program(&item_at_outer),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(7),
            expression: 7,
            item: 3,
        })
    );

    nested.root.iteration = None;
    nested.expressions.push(ExpressionNode {
        id: 6,
        expression: Expression::SequenceItemAt {
            sequence: GeneratedSequence::Range {
                from: None,
                to: 2,
                item: 3,
            },
            index: 1,
        },
    });
    assert_eq!(
        validate_program(&nested),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Expression(5),
            expression: 5,
            item: 3,
        })
    );
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
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    };

    let mut missing = program();
    set_target_fields(
        &mut missing,
        vec![SchemaNode::group("Child", Vec::new()).repeating()],
    );
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
        construction: TargetConstruction::Group,
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
    set_target_fields(
        &mut missing_sort,
        vec![SchemaNode::group("Child", Vec::new()).repeating()],
    );
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
    set_target_fields(
        &mut missing_window,
        vec![SchemaNode::group("Child", Vec::new()).repeating()],
    );
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
    set_target_fields(
        &mut invalid_first,
        vec![SchemaNode::group("Child", Vec::new()).repeating()],
    );
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

    let mut missing_value_map_input = program();
    missing_value_map_input.expressions[1].expression = Expression::ValueMap {
        input: 99,
        input_type: Some(ScalarType::String),
        table: Vec::new(),
        default: None,
    };
    assert_eq!(
        validate_program(&missing_value_map_input),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    let mut missing_lookup_match = program();
    missing_lookup_match.expressions[1].expression = Expression::Lookup {
        collection: vec!["Rows".into()],
        key: Vec::new(),
        matches: 99,
        value: Vec::new(),
    };
    assert_eq!(
        validate_program(&missing_lookup_match),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );
}
