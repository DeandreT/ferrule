//! `Node::dependencies` and the `.mfd` export's and GUI's per-node input lists
//! read inputs through the shared role-aware node input visitor. These checks
//! compare them with the hand-written lists they replaced over every node
//! kind, the graph and user-function bodies of every in-tree `.mfd` import,
//! and every in-tree Project file.

use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, FilterMapCapture, FilterMapV1, FunctionId, FunctionParameterId, JoinConditions,
    JoinId, JoinKey, JoinPlan, JoinSource, Node, NodeId, NodeInputSlot, OwnedAggregate,
    OwnedAggregateValue, OwnedCollectionFind, OwnedDynamicSourceField, OwnedJoinAggregate,
    OwnedJoinConditions, OwnedJoinField, OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource,
    OwnedJoinSourcePosition, OwnedLookup, OwnedPosition, OwnedRecursiveCollect,
    OwnedRowDocumentPath, OwnedSourceField, OwnedSourceRef, OwnedXmlMixedContent,
    OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement, OwnedXmlSerialize, RuntimeValue,
    SequenceExpr, SourceOwner, XmlMixedContentReplacement,
};

/// `Node::dependencies` as it was written by hand.
fn legacy_dependencies(node: &Node) -> Vec<NodeId> {
    match node {
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::SourceField { .. }
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Raise { message } => message.iter().copied().collect(),
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(lookup) => vec![lookup.matches],
        Node::OwnedCollectionFind(find) => vec![find.predicate, find.value],
        Node::OwnedAggregate(aggregate) => aggregate.inputs(),
        Node::OwnedJoinAggregate(aggregate) => aggregate.inputs(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain(sequence.owned_items())
            .chain([*predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain(sequence.owned_items())
            .chain([*index])
            .collect(),
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(sequence.owned_items())
            .chain([*predicate, *expression, *arg].into_iter().flatten())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The `.mfd` export auto-number pass's input list as it was written by hand.
fn legacy_auto_number(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(
                [Some(sequence.item()), *predicate, *expression, *arg]
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}

/// The `.mfd` export recursive pass's input list as it was written by hand.
fn legacy_recursive(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(
                [Some(sequence.item()), *predicate, *expression, *arg]
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The `.mfd` export join pass's input list as it was written by hand.
fn legacy_join(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
    }
}

/// The GUI graph references' input list as it was written by hand.
fn legacy_graph_references(node: &Node) -> Vec<NodeId> {
    match node {
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => Vec::new(),
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedXmlMixedContent(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// The GUI canvas's input list as it was written by hand.
fn legacy_canvas(node: &Node) -> Vec<NodeId> {
    match node {
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedXmlSerialize(_)
        | Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::XmlSerialize { .. } => vec![],
        Node::Raise { message } => message.iter().copied().collect(),
        Node::RuntimeParameterDefault { default, .. } => vec![*default],
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![*input],
        Node::OwnedLookup(_)
        | Node::OwnedAggregate(_)
        | Node::OwnedCollectionFind(_)
        | Node::OwnedJoinField(_)
        | Node::OwnedJoinSourcePosition(_)
        | Node::OwnedXmlMixedContent(_)
        | Node::OwnedJoinAggregate(_) => node.dependencies(),
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::OwnedDynamicSourceField(field) => vec![field.key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .chain(predicate.iter().copied())
            .chain(expression.iter().copied())
            .chain(arg.iter().copied())
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// Every node kind, numbered; adding a kind fails to compile here until the
/// generator covers it.
fn kind(node: &Node) -> usize {
    match node {
        Node::OwnedSourceField(_) => 0,
        Node::OwnedPosition(_) => 1,
        Node::OwnedRowDocumentPath(_) => 2,
        Node::OwnedLookup(_) => 3,
        Node::OwnedCollectionFind(_) => 4,
        Node::OwnedXmlSerialize(_) => 5,
        Node::OwnedAggregate(_) => 6,
        Node::OwnedJoinField(_) => 7,
        Node::OwnedJoinSourcePosition(_) => 8,
        Node::OwnedXmlMixedContent(_) => 9,
        Node::OwnedJoinAggregate(_) => 10,
        Node::SourceField { .. } => 11,
        Node::SourceRootXmlTypeEquals { .. } => 12,
        Node::SourceRootField { .. } => 13,
        Node::SourceDocumentPath => 14,
        Node::Position { .. } => 15,
        Node::JoinField { .. } => 16,
        Node::JoinPosition { .. } => 17,
        Node::Unconnected => 18,
        Node::Const { .. } => 19,
        Node::FunctionParameter { .. } => 20,
        Node::RuntimeValue { .. } => 21,
        Node::RuntimeParameter { .. } => 22,
        Node::RuntimeParameterDefault { .. } => 23,
        Node::Call { .. } => 24,
        Node::UserFunctionCall { .. } => 25,
        Node::If { .. } => 26,
        Node::Raise { .. } => 27,
        Node::ValueMap { .. } => 28,
        Node::Lookup { .. } => 29,
        Node::DynamicSourceField { .. } => 30,
        Node::OwnedDynamicSourceField(_) => 31,
        Node::XmlMixedContent { .. } => 32,
        Node::XmlSerialize { .. } => 33,
        Node::CollectionFind { .. } => 34,
        Node::SequenceExists { .. } => 35,
        Node::SequenceItemAt { .. } => 36,
        Node::SequenceAggregate { .. } => 37,
        Node::Aggregate { .. } => 38,
        Node::JoinAggregate { .. } => 39,
    }
}
const KINDS: usize = 40;

fn sequence_kind(sequence: &SequenceExpr) -> usize {
    match sequence {
        SequenceExpr::Tokenize { .. } => 0,
        SequenceExpr::TokenizeByLength { .. } => 1,
        SequenceExpr::TokenizeRegex { .. } => 2,
        SequenceExpr::Generate { .. } => 3,
        SequenceExpr::FilterMapV1(_) => 4,
        SequenceExpr::RecursiveCollect { .. } => 5,
        SequenceExpr::OwnedRecursiveCollect(_) => 6,
    }
}
const SEQUENCE_KINDS: usize = 7;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn owned(parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(parts),
    }
}

/// Supplies input ids from a repeating pattern, so inputs are distinct when
/// the pattern is long and shared when it repeats.
struct Ids {
    pattern: Vec<NodeId>,
    next: usize,
}

impl Ids {
    fn id(&mut self) -> NodeId {
        let id = self.pattern[self.next % self.pattern.len()];
        self.next += 1;
        id
    }
    fn optional(&mut self, present: bool) -> Option<NodeId> {
        present.then(|| self.id())
    }
}

fn sequences(ids: &mut Ids, present: bool) -> Vec<SequenceExpr> {
    let mut base = vec![
        SequenceExpr::Tokenize {
            input: ids.id(),
            delimiter: ids.id(),
            item: ids.id(),
        },
        SequenceExpr::TokenizeByLength {
            input: ids.id(),
            length: ids.id(),
            item: ids.id(),
        },
        SequenceExpr::TokenizeRegex {
            input: ids.id(),
            pattern: ids.id(),
            flags: ids.optional(present),
            item: ids.id(),
        },
        SequenceExpr::Generate {
            from: ids.optional(present),
            to: ids.id(),
            item: ids.id(),
        },
        SequenceExpr::RecursiveCollect {
            collection: path(&["Tree"]),
            children: path(&["Children"]),
            descent_value: path(&["Name"]),
            values: path(&["Leaves"]),
            value: path(&["Value"]),
            prefix: ids.id(),
            separator: ids.id(),
            item: ids.id(),
        },
        SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
            collection: owned(&["Tree"]),
            children: path(&["Children"]),
            descent_value: path(&["Name"]),
            values: path(&["Leaves"]),
            value: path(&["Value"]),
            prefix: ids.id(),
            separator: ids.id(),
            item: ids.id(),
        }),
    ];
    let compose = |source: SequenceExpr, ids: &mut Ids| {
        SequenceExpr::FilterMapV1(FilterMapV1 {
            source: Box::new(source),
            item: ids.id(),
            predicate: FunctionId::new(1),
            mapper: FunctionId::new(2),
            output_type: ScalarType::String,
            captures: (0..usize::from(present) * 2)
                .map(|_| FilterMapCapture {
                    node: ids.id(),
                    ty: ScalarType::Int,
                })
                .collect(),
        })
    };
    let composed: Vec<_> = base
        .iter()
        .cloned()
        .map(|source| compose(source, ids))
        .collect();
    let twice = compose(composed[0].clone(), ids);
    base.extend(composed);
    base.push(twice);
    base
}

fn join_plan() -> JoinPlan {
    JoinPlan::new(
        JoinSource::new(path(&["A"])),
        JoinSource::new(path(&["B"])),
        JoinConditions::new(JoinKey::new(path(&["A"]), path(&["Id"]), path(&["Id"]))),
    )
    .expect("distinct collections with a joined left key")
}

fn owned_join_plan() -> OwnedJoinPlan {
    OwnedJoinPlan::new(
        OwnedJoinSource::new(owned(&["A"])),
        OwnedJoinSource::new(owned(&["B"])),
        OwnedJoinConditions::new(OwnedJoinKey::new(
            owned(&["A"]),
            path(&["Id"]),
            path(&["Id"]),
        )),
    )
    .expect("distinct collections with a joined left key")
}

/// Every node kind, with optional inputs present or absent.
fn nodes(ids: &mut Ids, present: bool) -> Vec<Node> {
    let schema = Box::new(SchemaNode::group("Element", Vec::new()));
    let mut nodes = vec![
        Node::OwnedSourceField(OwnedSourceField {
            source: owned(&["Value"]),
            frame: None,
        }),
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        }),
        Node::OwnedRowDocumentPath(OwnedRowDocumentPath {
            owner: SourceOwner::Primary,
            collection: None,
        }),
        Node::OwnedLookup(OwnedLookup {
            collection: owned(&["Rows"]),
            key: path(&["Key"]),
            matches: ids.id(),
            value: path(&["Value"]),
        }),
        Node::OwnedCollectionFind(OwnedCollectionFind {
            collection: owned(&["Rows"]),
            predicate: ids.id(),
            value: ids.id(),
        }),
        Node::OwnedXmlSerialize(OwnedXmlSerialize {
            source: owned(&["Element"]),
            frame: None,
            schema: schema.clone(),
            declaration: false,
            indent: false,
            namespace: None,
        }),
        Node::OwnedAggregate(OwnedAggregate {
            function: AggregateOp::Join,
            collection: owned(&["Rows"]),
            value: OwnedAggregateValue::Path(path(&["Value"])),
            arg: ids.optional(present),
        }),
        Node::OwnedAggregate(OwnedAggregate {
            function: AggregateOp::Join,
            collection: owned(&["Rows"]),
            value: OwnedAggregateValue::Expression(ids.id()),
            arg: ids.optional(present),
        }),
        Node::OwnedJoinField(OwnedJoinField {
            join: JoinId::new(1),
            collection: owned(&["A"]),
            path: path(&["Id"]),
        }),
        Node::OwnedJoinSourcePosition(OwnedJoinSourcePosition {
            join: JoinId::new(1),
            collection: owned(&["A"]),
        }),
        Node::OwnedXmlMixedContent(OwnedXmlMixedContent {
            source: owned(&["Text"]),
            frame: None,
            replacements: [
                ("b", OwnedXmlMixedContentContext::Parent),
                ("i", OwnedXmlMixedContentContext::Occurrence),
            ]
            .into_iter()
            .take(usize::from(present) * 2)
            .map(|(element, context)| OwnedXmlMixedContentReplacement {
                element: element.into(),
                context,
                expression: ids.id(),
            })
            .collect(),
        }),
        Node::OwnedJoinAggregate(OwnedJoinAggregate {
            function: AggregateOp::Sum,
            join: JoinId::new(1),
            plan: owned_join_plan(),
            expression: ids.optional(present),
            arg: ids.optional(present),
        }),
        Node::SourceField {
            path: path(&["Value"]),
            frame: None,
        },
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Type".into(),
        },
        Node::SourceRootField {
            path: path(&["Value"]),
            required: false,
        },
        Node::SourceDocumentPath,
        Node::Position {
            collection: Vec::new(),
        },
        Node::JoinField {
            join: JoinId::new(1),
            collection: path(&["A"]),
            path: path(&["Id"]),
        },
        Node::JoinPosition {
            join: JoinId::new(1),
        },
        Node::Unconnected,
        Node::Const { value: Value::Null },
        Node::FunctionParameter {
            parameter: FunctionParameterId::new(1),
        },
        Node::RuntimeValue {
            value: RuntimeValue::CurrentDateTime,
        },
        Node::RuntimeParameter {
            name: "Name".into(),
            ty: ScalarType::String,
            preview: None,
        },
        Node::RuntimeParameterDefault {
            name: "Name".into(),
            ty: ScalarType::String,
            default: ids.id(),
            preview: None,
        },
        Node::Call {
            function: "concat".into(),
            args: (0..usize::from(present) * 3).map(|_| ids.id()).collect(),
        },
        Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: (0..usize::from(present) * 2).map(|_| ids.id()).collect(),
        },
        Node::If {
            condition: ids.id(),
            then: ids.id(),
            else_: ids.id(),
        },
        Node::Raise {
            message: ids.optional(present),
        },
        Node::ValueMap {
            input: ids.id(),
            input_type: None,
            table: Vec::new(),
            default: None,
        },
        Node::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Key"]),
            matches: ids.id(),
            value: path(&["Value"]),
        },
        Node::DynamicSourceField {
            object: path(&["Object"]),
            frame: None,
            key: ids.id(),
        },
        Node::OwnedDynamicSourceField(OwnedDynamicSourceField {
            object: owned(&["Object"]),
            frame: None,
            key: ids.id(),
        }),
        Node::XmlMixedContent {
            path: path(&["Text"]),
            frame: None,
            replacements: [("b", &[][..]), ("i", &["Text", "i"])]
                .into_iter()
                .take(usize::from(present) * 2)
                .map(|(element, collection)| XmlMixedContentReplacement {
                    element: element.into(),
                    collection: path(collection),
                    expression: ids.id(),
                })
                .collect(),
        },
        Node::XmlSerialize {
            path: path(&["Element"]),
            frame: None,
            schema,
            declaration: false,
            indent: false,
            namespace: None,
        },
        Node::CollectionFind {
            collection: path(&["Rows"]),
            predicate: ids.id(),
            value: ids.id(),
        },
        Node::Aggregate {
            function: AggregateOp::Join,
            collection: path(&["Rows"]),
            value: Vec::new(),
            expression: ids.optional(present),
            arg: ids.optional(present),
        },
        Node::JoinAggregate {
            function: AggregateOp::Sum,
            join: JoinId::new(1),
            plan: join_plan(),
            expression: ids.optional(present),
            arg: ids.optional(present),
        },
    ];
    for sequence in sequences(ids, present) {
        nodes.push(Node::SequenceExists {
            sequence: sequence.clone(),
            predicate: ids.id(),
        });
        nodes.push(Node::SequenceItemAt {
            sequence: sequence.clone(),
            index: ids.id(),
        });
        nodes.push(Node::SequenceAggregate {
            function: AggregateOp::Join,
            sequence,
            predicate: ids.optional(present),
            expression: ids.optional(present),
            arg: ids.optional(present),
        });
    }
    nodes
}

/// Every synthetic node under distinct, pairwise-shared and fully shared
/// input ids.
fn synthetic_nodes() -> Vec<Node> {
    let patterns = [(1..=64).collect::<Vec<_>>(), vec![1, 2], vec![7]];
    let mut all = Vec::new();
    for pattern in patterns {
        for present in [true, false] {
            let mut ids = Ids {
                pattern: pattern.clone(),
                next: 0,
            };
            all.extend(nodes(&mut ids, present));
        }
    }
    all
}

fn check(node: &Node, origin: &str) {
    assert_eq!(
        node.dependencies(),
        legacy_dependencies(node),
        "{origin}: {node:?}"
    );
    let kept_items = node.inputs_where(|input| input.slot != NodeInputSlot::SequenceReservedItem);
    assert_eq!(kept_items, legacy_auto_number(node), "{origin}: {node:?}");
    assert_eq!(kept_items, legacy_recursive(node), "{origin}: {node:?}");
    let evaluated = node.evaluated_inputs();
    assert_eq!(evaluated, legacy_join(node), "{origin}: {node:?}");
    assert_eq!(
        evaluated,
        legacy_graph_references(node),
        "{origin}: {node:?}"
    );
    assert_eq!(evaluated, legacy_canvas(node), "{origin}: {node:?}");
}

fn collect(directory: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let skipped = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| ["target", "node_modules", ".git", "bin", "obj"].contains(&name));
        if kind.is_dir() && !skipped {
            collect(&path, extension, files);
        } else if kind.is_file() && path.extension().is_some_and(|found| found == extension) {
            files.push(path);
        }
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_node_kind_reads_the_same_node_inputs() {
    let nodes = synthetic_nodes();
    let mut kinds = [false; KINDS];
    let mut sequence_kinds = [false; SEQUENCE_KINDS];
    for node in &nodes {
        kinds[kind(node)] = true;
        if let Node::SequenceExists { sequence, .. } = node {
            sequence_kinds[sequence_kind(sequence)] = true;
        }
        check(node, "synthetic");
    }
    assert!(kinds.iter().all(|covered| *covered), "{kinds:?}");
    assert!(sequence_kinds.iter().all(|covered| *covered));
}

#[test]
fn every_in_tree_mfd_import_reads_the_same_node_inputs() {
    let mut files = Vec::new();
    collect(&workspace().join("crates"), "mfd", &mut files);
    files.sort();
    let mut imported = 0;
    let mut nodes = 0;
    for file in &files {
        let Ok(import) = mfd::import(file) else {
            continue;
        };
        imported += 1;
        let project = import.project;
        let bodies = project
            .user_functions
            .values()
            .flat_map(|function| function.body.nodes.values());
        for node in project.graph.nodes.values().chain(bodies) {
            check(node, &file.display().to_string());
            nodes += 1;
        }
    }
    assert!(
        imported > 300,
        "imported {imported} of {} designs",
        files.len()
    );
    assert!(nodes > 1000, "checked {nodes} nodes");
}

#[test]
fn every_in_tree_project_reads_the_same_node_inputs() {
    let mut files = Vec::new();
    for root in ["conformance", "crates", "examples"] {
        collect(&workspace().join(root), "json", &mut files);
    }
    files.sort();
    let mut nodes = 0;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !text.contains("\"graph\"") || !text.contains("\"root\"") {
            continue;
        }
        let Ok(project) = serde_json::from_str::<mapping::Project>(&text) else {
            continue;
        };
        let bodies = project
            .user_functions
            .values()
            .flat_map(|function| function.body.nodes.values());
        for node in project.graph.nodes.values().chain(bodies) {
            check(node, &file.display().to_string());
            nodes += 1;
        }
    }
    assert!(nodes > 500, "checked {nodes} nodes");
}
