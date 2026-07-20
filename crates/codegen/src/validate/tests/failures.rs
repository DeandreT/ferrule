use super::*;
use crate::{
    FailureIteration, FailureRule, FailureSelection, NamedSourceProgram, SequenceExpressionRole,
};

fn source_rule(path: &[&str]) -> FailureRule {
    FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(
            path.iter().map(|segment| (*segment).into()).collect(),
        )),
        selection: FailureSelection::All,
        message: None,
    }
}

#[test]
fn validates_empty_primary_and_named_source_collections() {
    let mut valid = program();
    valid.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Rows", Vec::new()).repeating(),
            SchemaNode::scalar("Code", ScalarType::String),
        ],
    );
    valid.extra_sources.push(NamedSourceProgram {
        name: "Catalog".into(),
        source: SchemaNode::group(
            "CatalogDocument",
            vec![SchemaNode::group("Entries", Vec::new()).repeating()],
        ),
    });
    valid.failure_rules = vec![
        source_rule(&[]),
        source_rule(&["Rows"]),
        source_rule(&["Catalog", "Entries"]),
    ];
    assert_eq!(validate_program(&valid), Ok(()));

    valid.failure_rules = vec![source_rule(&["Code"])];
    assert_eq!(
        validate_program(&valid),
        Err(ProgramValidationError::InvalidFailureSourceIteration {
            rule: 1,
            source_path: vec!["Code".into()],
        })
    );
}

#[test]
fn validates_selection_and_message_presence_for_each_polarity() {
    let mut valid = program();
    valid.failure_rules = vec![
        source_rule(&[]),
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
            selection: FailureSelection::WhenTrue(1),
            message: None,
        },
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
            selection: FailureSelection::WhenFalse(1),
            message: Some(2),
        },
    ];
    assert_eq!(validate_program(&valid), Ok(()));

    let mut missing_predicate = valid.clone();
    missing_predicate.failure_rules[1].selection = FailureSelection::WhenTrue(99);
    assert_eq!(
        validate_program(&missing_predicate),
        Err(ProgramValidationError::MissingFailurePredicate {
            rule: 2,
            expression: 99,
        })
    );

    let mut missing_message = valid;
    missing_message.failure_rules[2].message = Some(99);
    assert_eq!(
        validate_program(&missing_message),
        Err(ProgramValidationError::MissingFailureMessage {
            rule: 3,
            expression: 99,
        })
    );
}

#[test]
fn generated_items_are_owned_globally_and_lexically() {
    let mut valid = program();
    valid.expressions.extend([
        ExpressionNode {
            id: 3,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        },
        ExpressionNode {
            id: 4,
            expression: Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![3, 1],
            },
        },
    ]);
    valid.failure_rules.push(FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::Range {
            from: Some(1),
            to: 1,
            item: 3,
        }),
        selection: FailureSelection::WhenTrue(4),
        message: Some(3),
    });
    assert_eq!(validate_program(&valid), Ok(()));

    let mut own_input = valid.clone();
    let FailureIteration::Generated(GeneratedSequence::Range { from, .. }) =
        &mut own_input.failure_rules[0].iteration
    else {
        unreachable!();
    };
    *from = Some(3);
    assert_eq!(
        validate_program(&own_input),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::FailureRule(1),
            expression: 3,
            item: 3,
        })
    );

    let mut duplicate = valid.clone();
    duplicate.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 1,
        item: 3,
    }));
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateSequenceItem {
            owner: SequenceOwner::FailureRule(1),
            first_owner: SequenceOwner::Scope(Vec::new()),
            expression: 3,
        })
    );

    let mut foreign = program();
    foreign.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    foreign.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 1,
        item: 3,
    }));
    foreign.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
        selection: FailureSelection::All,
        message: Some(3),
    });
    assert_eq!(
        validate_program(&foreign),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::FailureRule(1),
            expression: 3,
            item: 3,
        })
    );
}

#[test]
fn reports_missing_generated_inputs_and_items_with_rule_ownership() {
    let mut missing_input = program();
    missing_input.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    missing_input.failure_rules.push(FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::Range {
            from: Some(99),
            to: 1,
            item: 3,
        }),
        selection: FailureSelection::All,
        message: None,
    });
    assert_eq!(
        validate_program(&missing_input),
        Err(ProgramValidationError::MissingSequenceExpression {
            owner: SequenceOwner::FailureRule(1),
            role: SequenceExpressionRole::Input(0),
            expression: 99,
        })
    );

    let mut missing_item = program();
    missing_item.failure_rules.push(FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::Range {
            from: None,
            to: 1,
            item: 99,
        }),
        selection: FailureSelection::All,
        message: None,
    });
    assert_eq!(
        validate_program(&missing_item),
        Err(ProgramValidationError::MissingSequenceExpression {
            owner: SequenceOwner::FailureRule(1),
            role: SequenceExpressionRole::Item,
            expression: 99,
        })
    );

    let mut wrong_item = program();
    wrong_item.failure_rules.push(FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::Range {
            from: None,
            to: 1,
            item: 2,
        }),
        selection: FailureSelection::All,
        message: None,
    });
    assert_eq!(
        validate_program(&wrong_item),
        Err(ProgramValidationError::InvalidSequenceItem {
            owner: SequenceOwner::FailureRule(1),
            expression: 2,
        })
    );
}

#[test]
fn validates_recursive_paths_against_a_named_source_root() {
    let mut program = program();
    program.extra_sources.push(NamedSourceProgram {
        name: "Tree".into(),
        source: SchemaNode::group(
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
        ),
    });
    program.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    program.failure_rules.push(FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::RecursiveCollect {
            collection: vec!["Tree".into()],
            children: vec!["children".into()],
            descent_value: vec!["name".into()],
            values: vec!["files".into()],
            value: vec!["name".into()],
            prefix: 1,
            separator: 1,
            item: 3,
        }),
        selection: FailureSelection::All,
        message: Some(3),
    });

    assert_eq!(validate_program(&program), Ok(()));
}
