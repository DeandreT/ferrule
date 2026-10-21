//! #447 strict owned collection aggregate wire form.
use crate::*;
use serde_json::json;

fn rows(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    }
}

/// The pre-#447 reader's tagged node kinds relevant here.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousNode {
    Aggregate {
        function: AggregateOp,
        collection: Vec<String>,
        #[serde(default)]
        value: Vec<String>,
        expression: Option<NodeId>,
        arg: Option<NodeId>,
    },
}

#[test]
fn owned_aggregate_round_trips_both_value_forms() {
    let path = Node::OwnedAggregate(OwnedAggregate {
        function: AggregateOp::Join,
        collection: rows(SourceOwner::Named {
            name: " Ref ".into(),
        }),
        value: OwnedAggregateValue::Path(vec!["Name".into()]),
        arg: Some(4),
    });
    let wire = json!({"kind":"owned_aggregate","function":"join",
        "collection":{"owner":{"kind":"named","name":" Ref "},"path":["Rows"]},
        "value":["Name"],"arg":4});
    assert_eq!(serde_json::to_value(&path).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.owned_source_kind(), Some("owned_aggregate"));
    assert_eq!(decoded.dependencies(), vec![4]);
    assert!(matches!(decoded, Node::OwnedAggregate(found)
        if found.value == OwnedAggregateValue::Path(vec!["Name".into()])));

    // The empty path stays explicit and distinct from a computed value.
    let count = Node::OwnedAggregate(OwnedAggregate {
        function: AggregateOp::Count,
        collection: rows(SourceOwner::Primary),
        value: OwnedAggregateValue::Path(vec![]),
        arg: None,
    });
    let wire = json!({"kind":"owned_aggregate","function":"count",
        "collection":{"owner":{"kind":"primary"},"path":["Rows"]},"value":[]});
    assert_eq!(serde_json::to_value(&count).unwrap(), wire);
    assert!(serde_json::from_value::<Node>(wire).is_ok());

    let computed = Node::OwnedAggregate(OwnedAggregate {
        function: AggregateOp::Sum,
        collection: rows(SourceOwner::Primary),
        value: OwnedAggregateValue::Expression(5),
        arg: Some(6),
    });
    let wire = json!({"kind":"owned_aggregate","function":"sum",
        "collection":{"owner":{"kind":"primary"},"path":["Rows"]},"expression":5,"arg":6});
    assert_eq!(serde_json::to_value(&computed).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    // The item expression is evaluated before the parent argument.
    assert_eq!(decoded.dependencies(), vec![5, 6]);
}

#[test]
fn owned_aggregate_wire_is_strict() {
    for wire in [
        // Exactly one of value and expression.
        json!({"kind":"owned_aggregate","function":"sum",
            "collection":{"owner":{"kind":"primary"},"path":["Rows"]}}),
        json!({"kind":"owned_aggregate","function":"sum",
            "collection":{"owner":{"kind":"primary"},"path":["Rows"]},"value":[],"expression":1}),
        // Legacy array collections, malformed owners and unknown fields.
        json!({"kind":"owned_aggregate","function":"sum","collection":["Rows"],"value":[]}),
        json!({"kind":"owned_aggregate","function":"sum",
            "collection":{"owner":{"kind":"named","name":""},"path":[]},"value":[]}),
        json!({"kind":"owned_aggregate","function":"sum",
            "collection":{"owner":{"kind":"primary"}},"value":[]}),
        json!({"kind":"owned_aggregate","function":"sum",
            "collection":{"owner":{"kind":"primary"},"path":[]},"value":[],"frame":[]}),
        json!({"kind":"owned_aggregate","function":"median",
            "collection":{"owner":{"kind":"primary"},"path":[]},"value":[]}),
        json!({"kind":"owned_aggregate",
            "collection":{"owner":{"kind":"primary"},"path":[]},"value":[]}),
    ] {
        assert!(
            serde_json::from_value::<Node>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn previous_readers_refuse_owned_aggregates_and_legacy_wire_is_unchanged() {
    let owned = Node::OwnedAggregate(OwnedAggregate {
        function: AggregateOp::Count,
        collection: rows(SourceOwner::Primary),
        value: OwnedAggregateValue::Path(vec![]),
        arg: None,
    });
    let error =
        serde_json::from_value::<PreviousNode>(serde_json::to_value(&owned).unwrap()).unwrap_err();
    assert!(error.to_string().contains("owned_aggregate"), "{error}");
    let legacy = Node::Aggregate {
        function: AggregateOp::Sum,
        collection: vec!["Rows".into()],
        value: vec!["Rank".into()],
        expression: None,
        arg: Some(2),
    };
    let wire = json!({"kind":"aggregate","function":"sum","collection":["Rows"],
        "value":["Rank"],"arg":2});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), wire);
    assert!(serde_json::from_value::<PreviousNode>(wire.clone()).is_ok());
    assert!(matches!(
        serde_json::from_value::<Node>(wire).unwrap(),
        Node::Aggregate { .. }
    ));
    assert_eq!(legacy.owned_source_kind(), None);
    // A legacy aggregate without `value` keeps defaulting to the item itself.
    let wire = json!({"kind":"aggregate","function":"count","collection":["Rows"]});
    assert!(matches!(
        serde_json::from_value::<Node>(wire).unwrap(),
        Node::Aggregate { value, .. } if value.is_empty()
    ));
}
