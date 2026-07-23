use mapping::NodeId;

use crate::Expression;

pub(super) fn of(expression: &Expression) -> Vec<NodeId> {
    match expression {
        Expression::SourceField { .. }
        | Expression::SourceDocumentPath
        | Expression::Position { .. }
        | Expression::JoinField { .. }
        | Expression::JoinPosition { .. }
        | Expression::Const { .. }
        | Expression::FunctionParameter { .. }
        | Expression::RuntimeValue { .. } => Vec::new(),
        Expression::Call { args, .. } | Expression::UserFunctionCall { args, .. } => args.clone(),
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Expression::ValueMap { input, .. } => vec![*input],
        Expression::Lookup { matches, .. } => vec![*matches],
        Expression::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Expression::Aggregate { value, arg, .. } => {
            value.expression().into_iter().chain(*arg).collect()
        }
        Expression::JoinAggregate {
            expression, arg, ..
        } => expression.iter().copied().chain(*arg).collect(),
        Expression::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().chain([*predicate]).collect(),
        Expression::SequenceItemAt { sequence, index } => {
            sequence.inputs().chain([*index]).collect()
        }
    }
}
