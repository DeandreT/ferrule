use std::collections::BTreeMap;

use mapping::NodeId;

use crate::{
    Expression, GroupingExpressionRole, GroupingPlan, IterationPlan, IterationSource,
    ProgramValidationError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupingExpressionContext {
    Item(NodeId),
    Parent(NodeId),
}

impl GroupingExpressionContext {
    pub(super) const fn node(self) -> NodeId {
        match self {
            Self::Item(node) | Self::Parent(node) => node,
        }
    }

    pub(super) const fn is_parent_context(self) -> bool {
        matches!(self, Self::Parent(_))
    }
}

pub(super) fn validate(
    iteration: &IterationPlan,
    expressions: &BTreeMap<NodeId, &Expression>,
    target_path: &[String],
) -> Result<Option<GroupingExpressionContext>, ProgramValidationError> {
    let Some(grouping) = iteration.grouping() else {
        return Ok(None);
    };
    if let IterationSource::InnerJoin(join) = iteration.input() {
        return Err(ProgramValidationError::JoinGroupingUnsupported {
            target_path: target_path.to_vec(),
            join: join.id(),
        });
    }
    if matches!(iteration.input(), IterationSource::Concatenate(_)) {
        return Err(ProgramValidationError::InvalidScopeSequenceWrapper {
            target_path: target_path.to_vec(),
        });
    }
    let (role, context) = match grouping {
        GroupingPlan::By { key } => (
            GroupingExpressionRole::Key,
            GroupingExpressionContext::Item(key),
        ),
        GroupingPlan::AdjacentBy { key } => (
            GroupingExpressionRole::AdjacentKey,
            GroupingExpressionContext::Item(key),
        ),
        GroupingPlan::StartingWith { predicate } => (
            GroupingExpressionRole::StartingPredicate,
            GroupingExpressionContext::Item(predicate),
        ),
        GroupingPlan::EndingWith { predicate } => (
            GroupingExpressionRole::EndingPredicate,
            GroupingExpressionContext::Item(predicate),
        ),
        GroupingPlan::IntoBlocks { size } => (
            GroupingExpressionRole::BlockSize,
            GroupingExpressionContext::Parent(size),
        ),
    };
    if !expressions.contains_key(&context.node()) {
        return Err(ProgramValidationError::MissingGroupingExpression {
            target_path: target_path.to_vec(),
            role,
            expression: context.node(),
        });
    }
    Ok(Some(context))
}
