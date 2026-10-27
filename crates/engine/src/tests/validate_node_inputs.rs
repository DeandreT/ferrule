//! Validation reads node inputs through the shared role-aware visitor. These
//! checks compare it with the hand-written derivations it replaced, over every
//! node kind with distinct, shared and absent inputs, and over every graph of
//! the in-tree Project files.

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, FilterMapCapture, FilterMapV1, FunctionId, FunctionParameterId, JoinConditions,
    JoinId, JoinKey, JoinPlan, JoinSource, Node, NodeId, NodeInputRole, OwnedAggregate,
    OwnedAggregateValue, OwnedCollectionFind, OwnedDynamicSourceField, OwnedJoinAggregate,
    OwnedJoinConditions, OwnedJoinField, OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource,
    OwnedJoinSourcePosition, OwnedLookup, OwnedPosition, OwnedRecursiveCollect,
    OwnedRowDocumentPath, OwnedSourceDocumentPath, OwnedSourceField, OwnedSourceRef,
    OwnedXmlMixedContent, OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement,
    OwnedXmlSerialize, RuntimeValue, SequenceExpr, SourceOwner, XmlMixedContentReplacement,
};

use super::*;
use crate::validate::join::item_context_inputs;

/// `Node::dependencies` as it was written by hand.
fn legacy_dependencies(node: &Node) -> Vec<NodeId> {
    match node {
        Node::GeneratedPosition { item } => vec![*item],
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedSourceDocumentPath(_)
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
        Node::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => [Some(*start), Some(*step), *restart]
            .into_iter()
            .flatten()
            .collect(),
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

/// `context_node_inputs` as it was written by hand.
fn legacy_context_node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::GeneratedPosition { .. } => Vec::new(),
        Node::SequenceExists { sequence, .. } => sequence.inputs(),
        Node::SequenceAggregate { sequence, arg, .. } => {
            sequence.inputs().into_iter().chain(*arg).collect()
        }
        _ => legacy_node_inputs(node)
            .into_iter()
            .map(|(_, referenced)| referenced)
            .collect(),
    }
}

/// `node_inputs` as it was written by hand.
fn legacy_node_inputs(node: &Node) -> Vec<(String, NodeId)> {
    match node {
        Node::GeneratedPosition { item } => vec![("sequence item".into(), *item)],
        Node::OwnedSourceField(_)
        | Node::OwnedPosition(_)
        | Node::OwnedRowDocumentPath(_)
        | Node::OwnedSourceDocumentPath(_)
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
        Node::RuntimeParameterDefault { default, .. } => {
            vec![("default".into(), *default)]
        }
        Node::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => {
            let mut inputs = vec![("start".into(), *start), ("step".into(), *step)];
            inputs.extend(restart.map(|id| ("restart".into(), id)));
            inputs
        }
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args
            .iter()
            .enumerate()
            .map(|(index, &id)| (format!("argument {index}"), id))
            .collect(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![
            ("condition".into(), *condition),
            ("then branch".into(), *then),
            ("else branch".into(), *else_),
        ],
        Node::Raise { message } => message
            .iter()
            .map(|&message| ("message".to_string(), message))
            .collect(),
        Node::ValueMap { input, .. } => vec![("input".into(), *input)],
        Node::Lookup { matches, .. } => vec![("matches".into(), *matches)],
        Node::OwnedLookup(lookup) => vec![("matches".into(), lookup.matches)],
        Node::OwnedCollectionFind(find) => vec![
            ("predicate".into(), find.predicate),
            ("value expression".into(), find.value),
        ],
        Node::OwnedAggregate(aggregate) => aggregate
            .expression()
            .map(|id| ("value expression".to_string(), id))
            .into_iter()
            .chain(aggregate.arg.map(|id| ("argument".to_string(), id)))
            .collect(),
        Node::DynamicSourceField { key, .. } => vec![("property name".into(), *key)],
        Node::OwnedDynamicSourceField(field) => vec![("property name".into(), field.key)],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| {
                (
                    format!("replacement for `{}`", replacement.element),
                    replacement.expression,
                )
            })
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| {
                (
                    format!("replacement for `{}`", replacement.element),
                    replacement.expression,
                )
            })
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![
            ("predicate".into(), *predicate),
            ("value expression".into(), *value),
        ],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .enumerate()
            .map(|(index, id)| (format!("sequence argument {index}"), id))
            .chain([("predicate".to_string(), *predicate)])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .enumerate()
            .map(|(argument, id)| (format!("sequence argument {argument}"), id))
            .chain([("index".to_string(), *index)])
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
            .enumerate()
            .map(|(argument, id)| (format!("sequence argument {argument}"), id))
            .chain(predicate.iter().map(|&id| ("predicate".to_string(), id)))
            .chain(
                expression
                    .iter()
                    .map(|&id| ("aggregate expression".to_string(), id)),
            )
            .chain(arg.iter().map(|&id| ("argument".to_string(), id)))
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        } => expression
            .iter()
            .map(|&id| ("value expression".to_string(), id))
            .chain(arg.iter().map(|&id| ("argument".to_string(), id)))
            .collect(),
        Node::JoinAggregate {
            expression, arg, ..
        } => expression
            .iter()
            .map(|&id| ("value expression".to_string(), id))
            .chain(arg.iter().map(|&id| ("argument".to_string(), id)))
            .collect(),
        Node::OwnedJoinAggregate(aggregate) => aggregate
            .expression
            .map(|id| ("value expression".to_string(), id))
            .into_iter()
            .chain(aggregate.arg.map(|id| ("argument".to_string(), id)))
            .collect(),
    }
}

/// Join validation's item-context inputs as they were written by hand, which
/// classified lookups, mixed content and owned searches by node kind.
fn legacy_item_context_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Aggregate { expression, .. } => expression.iter().copied().collect(),
        Node::OwnedAggregate(aggregate) => aggregate.expression().into_iter().collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists { predicate, .. } => vec![*predicate],
        Node::SequenceAggregate {
            predicate,
            expression,
            ..
        } => predicate.iter().chain(expression).copied().collect(),
        Node::Lookup { matches, .. } => vec![*matches],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        _ => Vec::new(),
    }
}

/// The parent inputs the sequence-context validators listed by hand.
fn legacy_sequence_parent_inputs(node: &Node) -> Vec<(String, NodeId)> {
    let arguments = |sequence: &SequenceExpr| {
        sequence
            .inputs()
            .into_iter()
            .enumerate()
            .map(|(index, input)| (format!("sequence argument {index}"), input))
            .collect::<Vec<_>>()
    };
    match node {
        Node::SequenceItemAt { sequence, index } => {
            let mut inputs = arguments(sequence);
            inputs.push(("index".into(), *index));
            inputs
        }
        Node::SequenceAggregate { sequence, arg, .. } => {
            let mut inputs = arguments(sequence);
            inputs.extend(arg.map(|arg| ("aggregate argument".to_string(), arg)));
            inputs
        }
        _ => Vec::new(),
    }
}

/// The generated-item roots the sequence-context validators listed by hand.
fn legacy_generated_inputs(node: &Node) -> Vec<(String, NodeId)> {
    match node {
        Node::SequenceExists { predicate, .. } => vec![("predicate".into(), *predicate)],
        Node::SequenceAggregate {
            predicate,
            expression,
            ..
        } => [
            ("predicate", *predicate),
            ("aggregate expression", *expression),
        ]
        .into_iter()
        .filter_map(|(label, root)| root.map(|root| (label.to_string(), root)))
        .collect(),
        _ => Vec::new(),
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
        Node::OwnedSourceDocumentPath(_) => 40,
        Node::InvocationCounter { .. } => 41,
        Node::GeneratedPosition { .. } => 42,
    }
}
const KINDS: usize = 43;

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
        Node::OwnedSourceDocumentPath(OwnedSourceDocumentPath {
            owner: SourceOwner::Primary,
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
        Node::GeneratedPosition { item: ids.id() },
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
        Node::InvocationCounter {
            key: "counter".into(),
            start: ids.id(),
            step: ids.id(),
            restart: ids.optional(present),
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

/// Graph nodes and user-function body nodes of every in-tree Project file.
fn corpus_nodes() -> Vec<Node> {
    let mut nodes = Vec::new();
    for project in crate::scope_slots::project_corpus::projects() {
        nodes.extend(project.graph.nodes.into_values());
        for function in project.user_functions.into_values() {
            nodes.extend(function.body.nodes.into_values());
        }
    }
    nodes
}

/// The item-context inputs join validation reads by role: the hand-written
/// classification everywhere except lookups and mixed content, legacy and
/// owned, and owned searches, which now follow their evaluation context.
fn role_item_context_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        // A needle is evaluated once in the consumer's context.
        Node::Lookup { .. } | Node::OwnedLookup(_) => Vec::new(),
        // Only a replacement with a collection gets an occurrence frame.
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .filter(|replacement| !replacement.collection.is_empty())
            .map(|replacement| replacement.expression)
            .collect(),
        Node::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .filter(|replacement| replacement.context == OwnedXmlMixedContentContext::Occurrence)
            .map(|replacement| replacement.expression)
            .collect(),
        // Each candidate is the latest frame of the predicate and value.
        Node::OwnedCollectionFind(find) => vec![find.predicate, find.value],
        _ => legacy_item_context_inputs(node),
    }
}

fn check(node: &Node) {
    let new_labelled = node_inputs(node);
    assert_eq!(node.dependencies(), legacy_dependencies(node), "{node:?}");
    assert_eq!(new_labelled, legacy_node_inputs(node), "{node:?}");
    assert_eq!(
        context_node_inputs(node),
        legacy_context_node_inputs(node),
        "{node:?}"
    );
    assert_eq!(
        item_context_inputs(node),
        role_item_context_inputs(node),
        "{node:?}"
    );
    if matches!(
        node,
        Node::SequenceItemAt { .. } | Node::SequenceAggregate { .. }
    ) {
        assert_eq!(
            sequence_parent_inputs(node),
            legacy_sequence_parent_inputs(node),
            "{node:?}"
        );
    }
    if let Node::SequenceExists { sequence, .. } = node {
        assert_eq!(role_inputs(node, NodeInputRole::Parent), sequence.inputs());
    }
    if matches!(
        node,
        Node::SequenceExists { .. } | Node::SequenceAggregate { .. }
    ) {
        assert_eq!(
            labelled_inputs(node, |input| input.role == NodeInputRole::GeneratedItem),
            legacy_generated_inputs(node),
            "{node:?}"
        );
        assert_eq!(
            role_inputs(node, NodeInputRole::GeneratedItem),
            legacy_generated_inputs(node)
                .into_iter()
                .map(|(_, input)| input)
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn every_node_kind_reads_the_inputs_its_hand_written_derivations_read() {
    let nodes = synthetic_nodes();
    let mut kinds = [false; KINDS];
    let mut sequence_kinds = [false; SEQUENCE_KINDS];
    for node in &nodes {
        kinds[kind(node)] = true;
        if let Node::SequenceExists { sequence, .. } = node {
            sequence_kinds[sequence_kind(sequence)] = true;
        }
        check(node);
    }
    assert!(kinds.iter().all(|covered| *covered), "{kinds:?}");
    assert!(sequence_kinds.iter().all(|covered| *covered));
}

#[test]
fn every_in_tree_project_graph_reads_the_same_inputs() {
    let nodes = corpus_nodes();
    for node in &nodes {
        check(node);
    }
    assert!(nodes.len() > 500, "{} corpus nodes", nodes.len());
}
