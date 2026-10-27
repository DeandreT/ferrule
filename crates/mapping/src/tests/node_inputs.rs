use ir::ScalarType;

use super::*;
use crate::{
    AggregateOp, FilterMapV1, FunctionId, OwnedSourceRef, OwnedXmlMixedContent,
    OwnedXmlMixedContentReplacement, SourceOwner, XmlMixedContentReplacement,
};

fn inputs(node: &Node) -> Vec<(NodeId, NodeInputSlot, NodeInputRole)> {
    let mut found = Vec::new();
    node.visit_inputs(&mut |input| found.push((input.node, input.slot, input.role)));
    found
}

use NodeInputRole::{
    GeneratedItem, Item, ItemIdentity, JoinTuple, Lazy, Occurrence, Parent, ParentOccurrence,
};
use NodeInputSlot as Slot;

#[test]
fn generated_position_references_but_never_evaluates_its_owner() {
    let node = Node::GeneratedPosition { item: 7 };
    assert_eq!(
        inputs(&node),
        [(7, Slot::SequenceItem, NodeInputRole::ItemReference)]
    );
    assert_eq!(node.dependencies(), [7]);
    assert!(node.evaluated_inputs().is_empty());
    let encoded = serde_json::to_string(&node).unwrap();
    assert_eq!(encoded, r#"{"kind":"generated_position","item":7}"#);
    assert!(matches!(
        serde_json::from_str::<Node>(&encoded).unwrap(),
        Node::GeneratedPosition { item: 7 }
    ));
}

#[test]
fn aggregate_arguments_per_item_expressions_and_lazy_branches_stay_distinct() {
    let aggregate = Node::Aggregate {
        function: AggregateOp::Join,
        collection: vec!["Rows".into()],
        value: Vec::new(),
        expression: Some(1),
        arg: Some(2),
    };
    assert_eq!(
        inputs(&aggregate),
        [
            (1, Slot::Expression, Item),
            (2, Slot::AggregateArgument, Parent),
        ]
    );
    let branch = Node::If {
        condition: 1,
        then: 2,
        else_: 3,
    };
    assert_eq!(
        inputs(&branch),
        [
            (1, Slot::Condition, Parent),
            (2, Slot::Then, Lazy),
            (3, Slot::Else, Lazy),
        ]
    );
    let default = Node::RuntimeParameterDefault {
        name: "Name".into(),
        ty: ScalarType::String,
        default: 4,
        preview: None,
    };
    assert_eq!(inputs(&default), [(4, Slot::Default, Lazy)]);
}

#[test]
fn a_lookup_needle_is_evaluated_once_but_a_search_reads_each_item() {
    let lookup = Node::Lookup {
        collection: vec!["Rows".into()],
        key: vec!["Key".into()],
        matches: 1,
        value: vec!["Value".into()],
    };
    assert_eq!(inputs(&lookup), [(1, Slot::Matches, Parent)]);
    let find = Node::CollectionFind {
        collection: vec!["Rows".into()],
        predicate: 2,
        value: 3,
    };
    assert_eq!(
        inputs(&find),
        [(2, Slot::Predicate, Item), (3, Slot::Value, Item)]
    );
}

#[test]
fn a_join_aggregate_reads_its_expression_per_tuple() {
    let plan = crate::JoinPlan::new(
        crate::JoinSource::new(vec!["A".into()]),
        crate::JoinSource::new(vec!["B".into()]),
        crate::JoinConditions::new(crate::JoinKey::new(
            vec!["A".into()],
            vec!["Id".into()],
            vec!["Id".into()],
        )),
    )
    .expect("distinct collections with a joined left key");
    let aggregate = Node::JoinAggregate {
        function: AggregateOp::Sum,
        join: crate::JoinId::new(1),
        plan,
        expression: Some(1),
        arg: None,
    };
    assert_eq!(inputs(&aggregate), [(1, Slot::Expression, JoinTuple)]);
}

/// A composed sequence's inputs come first, then the reserved source item
/// and its own item, then the consumer's own inputs.
#[test]
fn sequence_consumers_own_their_item_identities() {
    let sequence = SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(SequenceExpr::Tokenize {
            input: 1,
            delimiter: 2,
            item: 3,
        }),
        item: 4,
        predicate: FunctionId::new(1),
        mapper: FunctionId::new(2),
        output_type: ScalarType::String,
        captures: vec![crate::FilterMapCapture {
            node: 5,
            ty: ScalarType::Int,
        }],
    });
    let aggregate = Node::SequenceAggregate {
        function: AggregateOp::Join,
        sequence: sequence.clone(),
        predicate: Some(6),
        expression: Some(7),
        arg: Some(8),
    };
    assert_eq!(
        inputs(&aggregate),
        [
            (1, Slot::SequenceInput(0), Parent),
            (2, Slot::SequenceInput(1), Parent),
            (5, Slot::SequenceInput(2), Parent),
            (3, Slot::SequenceReservedItem, ItemIdentity),
            (4, Slot::SequenceItem, ItemIdentity),
            (6, Slot::Predicate, GeneratedItem),
            (7, Slot::Expression, GeneratedItem),
            (8, Slot::AggregateArgument, Parent),
        ]
    );
    let item_at = Node::SequenceItemAt { sequence, index: 9 };
    assert_eq!(inputs(&item_at).last(), Some(&(9, Slot::Index, Parent)));
    assert_eq!(item_at.dependencies(), [1, 2, 5, 3, 4, 9]);
}

#[test]
fn mixed_content_replacements_read_their_occurrence_or_the_parent() {
    let legacy = Node::XmlMixedContent {
        path: vec!["Text".into()],
        frame: None,
        replacements: vec![
            XmlMixedContentReplacement {
                element: "b".into(),
                collection: Vec::new(),
                expression: 1,
            },
            XmlMixedContentReplacement {
                element: "i".into(),
                collection: vec!["Text".into(), "i".into()],
                expression: 2,
            },
        ],
    };
    assert_eq!(
        inputs(&legacy),
        [
            (1, Slot::Replacement(0), ParentOccurrence),
            (2, Slot::Replacement(1), Occurrence),
        ]
    );
    let owned = Node::OwnedXmlMixedContent(OwnedXmlMixedContent {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec!["Text".into()],
        },
        frame: None,
        replacements: [
            (OwnedXmlMixedContentContext::Occurrence, 3),
            (OwnedXmlMixedContentContext::Parent, 4),
        ]
        .into_iter()
        .map(|(context, expression)| OwnedXmlMixedContentReplacement {
            element: "b".into(),
            context,
            expression,
        })
        .collect(),
    });
    assert_eq!(
        inputs(&owned),
        [
            (3, Slot::Replacement(0), Occurrence),
            (4, Slot::Replacement(1), ParentOccurrence),
        ]
    );
}
