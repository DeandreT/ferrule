//! Every node kind and generated-sequence kind, with distinct, shared and
//! absent inputs.

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, FilterMapCapture, FilterMapV1, FunctionId, FunctionParameterId, JoinConditions,
    JoinId, JoinKey, JoinPlan, JoinSource, Node, NodeId, OwnedAggregate, OwnedAggregateValue,
    OwnedCollectionFind, OwnedDynamicSourceField, OwnedJoinAggregate, OwnedJoinConditions,
    OwnedJoinField, OwnedJoinKey, OwnedJoinPlan, OwnedJoinSource, OwnedJoinSourcePosition,
    OwnedLookup, OwnedPosition, OwnedRecursiveCollect, OwnedRowDocumentPath,
    OwnedSourceDocumentPath, OwnedSourceField, OwnedSourceRef, OwnedXmlMixedContent,
    OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement, OwnedXmlSerialize, RuntimeValue,
    SequenceExpr, SourceOwner, XmlMixedContentReplacement,
};

/// Every node kind, numbered; adding a kind fails to compile here until the
/// generator covers it.
pub(crate) fn kind(node: &Node) -> usize {
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
    }
}
pub(crate) const KINDS: usize = 42;

pub(crate) fn sequence_kind(sequence: &SequenceExpr) -> usize {
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
pub(crate) const SEQUENCE_KINDS: usize = 7;

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
pub(crate) fn synthetic_nodes() -> Vec<Node> {
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
