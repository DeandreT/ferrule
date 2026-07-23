use super::*;
use crate::{
    GeneratedSequence, GroupingExpressionRole, GroupingPlan, InnerJoin, JoinConditions, JoinId,
    JoinKey, JoinPlan, JoinSource,
};

fn grouped_program(grouping: GroupingPlan) -> Program {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Key", ScalarType::String)])
                .repeating(),
        ],
    );
    program.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::group("Group", Vec::new()).repeating()],
    );
    program.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: vec!["Key".into()],
        },
    });
    program.root.children.push(TargetScope {
        target_field: "Group".into(),
        repeating: true,
        iteration: Some(IterationPlan::source(vec!["Rows".into()]).with_grouping(grouping)),
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    });
    program
}

#[test]
fn accepts_each_grouping_mode_over_source_iteration() {
    for grouping in [
        GroupingPlan::By { key: 3 },
        GroupingPlan::AdjacentBy { key: 3 },
        GroupingPlan::StartingWith { predicate: 1 },
        GroupingPlan::EndingWith { predicate: 1 },
        GroupingPlan::IntoBlocks { size: 1 },
    ] {
        assert_eq!(validate_program(&grouped_program(grouping)), Ok(()));
    }
}

#[test]
fn validates_post_group_filters_in_the_candidate_context() {
    let mut valid = grouped_program(GroupingPlan::By { key: 3 });
    valid.root.children[0].iteration = Some(
        IterationPlan::source(vec!["Rows".into()])
            .with_filtered_grouping(GroupingPlan::By { key: 3 }, 1),
    );
    assert_eq!(validate_program(&valid), Ok(()));

    valid.root.children[0].iteration = Some(
        IterationPlan::source(vec!["Rows".into()])
            .with_filtered_grouping(GroupingPlan::By { key: 3 }, 99),
    );
    assert_eq!(
        validate_program(&valid),
        Err(ProgramValidationError::MissingPostGroupFilterExpression {
            target_path: vec!["Group".into()],
            expression: 99,
        })
    );
}

#[test]
fn reports_the_missing_expression_role_for_each_grouping_mode() {
    for (grouping, role) in [
        (GroupingPlan::By { key: 99 }, GroupingExpressionRole::Key),
        (
            GroupingPlan::AdjacentBy { key: 99 },
            GroupingExpressionRole::AdjacentKey,
        ),
        (
            GroupingPlan::StartingWith { predicate: 99 },
            GroupingExpressionRole::StartingPredicate,
        ),
        (
            GroupingPlan::EndingWith { predicate: 99 },
            GroupingExpressionRole::EndingPredicate,
        ),
        (
            GroupingPlan::IntoBlocks { size: 99 },
            GroupingExpressionRole::BlockSize,
        ),
    ] {
        assert_eq!(
            validate_program(&grouped_program(grouping)),
            Err(ProgramValidationError::MissingGroupingExpression {
                target_path: vec!["Group".into()],
                role,
                expression: 99,
            })
        );
    }
}

#[test]
fn generated_item_is_available_to_per_item_grouping_only() {
    let mut per_item = grouped_program(GroupingPlan::By { key: 3 });
    per_item.expressions[2].expression = Expression::SourceField {
        frame: None,
        path: Vec::new(),
    };
    per_item.root.children[0].iteration = Some(
        IterationPlan::generated(GeneratedSequence::Range {
            from: Some(1),
            to: 2,
            item: 3,
        })
        .with_grouping(GroupingPlan::StartingWith { predicate: 3 }),
    );
    assert_eq!(validate_program(&per_item), Ok(()));

    per_item.root.children[0].iteration = Some(
        IterationPlan::generated(GeneratedSequence::Range {
            from: Some(1),
            to: 2,
            item: 3,
        })
        .with_grouping(GroupingPlan::IntoBlocks { size: 3 }),
    );
    assert_eq!(
        validate_program(&per_item),
        Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: SequenceOwner::Scope(vec!["Group".into()]),
            expression: 3,
            item: 3,
        })
    );
}

#[test]
fn rejects_grouping_on_inner_joins_before_backend_emission() {
    let mut program = grouped_program(GroupingPlan::By { key: 3 });
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group("A", vec![SchemaNode::scalar("id", ScalarType::Int)]).repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("aid", ScalarType::Int)]).repeating(),
        ],
    );
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["id".into()],
            vec!["aid".into()],
        )),
    )
    .expect("test join plan is valid");
    program.root.children[0].iteration = Some(
        IterationPlan::join(InnerJoin::new(JoinId::new(17), plan))
            .with_grouping(GroupingPlan::By { key: 3 }),
    );

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::JoinGroupingUnsupported {
            target_path: vec!["Group".into()],
            join: JoinId::new(17),
        })
    );
}

#[test]
fn rejects_grouping_with_copy_current_source_construction() {
    let mut program = grouped_program(GroupingPlan::By { key: 3 });
    program.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group("Group", vec![SchemaNode::scalar("Key", ScalarType::String)])
                .repeating(),
        ],
    );
    program.root.children[0].construction = TargetConstruction::CopyCurrentSource;

    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::CopyConstructionHasGrouping {
            target_path: vec!["Group".into()],
        })
    );
}
