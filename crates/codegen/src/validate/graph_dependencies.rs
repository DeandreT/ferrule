use mapping::NodeId;

use crate::Expression;

pub(crate) fn of(expression: &Expression) -> Vec<NodeId> {
    match expression {
        Expression::OwnedSourceField(_)
        | Expression::OwnedPosition(_)
        | Expression::OwnedRowDocumentPath(_)
        | Expression::OwnedXmlSerialize(_)
        | Expression::OwnedJoinField(_)
        | Expression::OwnedJoinSourcePosition(_)
        | Expression::SourceField { .. }
        | Expression::SourceRootXmlTypeEquals { .. }
        | Expression::SourceRootField { .. }
        | Expression::XmlSerialize { .. }
        | Expression::SourceDocumentPath
        | Expression::Position { .. }
        | Expression::JoinField { .. }
        | Expression::JoinPosition { .. }
        | Expression::Const { .. }
        | Expression::FunctionParameter { .. }
        | Expression::RuntimeValue { .. }
        | Expression::RuntimeParameter { .. } => Vec::new(),
        Expression::RuntimeParameterDefault { default, .. } => vec![*default],
        Expression::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => [Some(*start), Some(*step), *restart]
            .into_iter()
            .flatten()
            .collect(),
        Expression::DynamicSourceField { key, .. } => vec![*key],
        Expression::OwnedDynamicSourceField(field) => vec![field.key],
        Expression::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Expression::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Expression::Call { args, .. } | Expression::UserFunctionCall { args, .. } => args.clone(),
        Expression::DelimitedTextField { input, .. } => vec![*input],
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Expression::Raise { message } => message.iter().copied().collect(),
        Expression::ValueMap { input, .. } => vec![*input],
        Expression::Lookup { matches, .. } => vec![*matches],
        Expression::OwnedLookup(lookup) => vec![lookup.matches],
        Expression::OwnedCollectionFind(find) => vec![find.predicate, find.value],
        Expression::OwnedAggregate(aggregate) => aggregate.inputs(),
        Expression::OwnedJoinAggregate(aggregate) => aggregate.inputs(),
        Expression::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Expression::DistinctTextMapLast { expression, .. } => vec![*expression],
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
        Expression::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
    }
}
