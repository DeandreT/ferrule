//! #450 strict owned joined aggregate wire form.
use crate::*;
use serde_json::json;

fn rows(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    }
}

fn plan() -> OwnedJoinPlan {
    OwnedJoinPlan::new(
        OwnedJoinSource::new(rows(SourceOwner::Primary)),
        OwnedJoinSource::new(rows(SourceOwner::Named {
            name: "Lookup".into(),
        })),
        OwnedJoinConditions::new(OwnedJoinKey::new(
            rows(SourceOwner::Primary),
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap()
}

fn plan_wire() -> serde_json::Value {
    json!({"first":{"collection":{"owner":{"kind":"primary"},"path":["Rows"]}},
        "second":{"source":{"collection":{"owner":{"kind":"named","name":"Lookup"},"path":["Rows"]}},
            "conditions":{"first":{"left_collection":{"owner":{"kind":"primary"},"path":["Rows"]},
                "left_path":["id"],"right_path":["id"]}}}})
}

/// The pre-#450 reader's tagged aggregate node kinds.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousNode {
    JoinAggregate {
        function: AggregateOp,
        join: JoinId,
        plan: JoinPlan,
        expression: Option<NodeId>,
        arg: Option<NodeId>,
    },
    OwnedAggregate(OwnedAggregate),
}

#[test]
fn owned_join_aggregate_round_trips_with_optional_inputs() {
    let node = Node::OwnedJoinAggregate(OwnedJoinAggregate {
        function: AggregateOp::Join,
        join: JoinId::new(9),
        plan: plan(),
        expression: Some(4),
        arg: Some(5),
    });
    let wire = json!({"kind":"owned_join_aggregate","function":"join","join":9,
        "plan":plan_wire(),"expression":4,"arg":5});
    assert_eq!(serde_json::to_value(&node).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.owned_source_kind(), Some("owned_join_aggregate"));
    // The tuple expression precedes the parent argument.
    assert_eq!(decoded.dependencies(), vec![4, 5]);
    assert!(
        matches!(&decoded, Node::OwnedJoinAggregate(found) if *found == OwnedJoinAggregate {
            function: AggregateOp::Join,
            join: JoinId::new(9),
            plan: plan(),
            expression: Some(4),
            arg: Some(5),
        })
    );

    let count = Node::OwnedJoinAggregate(OwnedJoinAggregate {
        function: AggregateOp::Count,
        join: JoinId::new(9),
        plan: plan(),
        expression: None,
        arg: None,
    });
    let wire = json!({"kind":"owned_join_aggregate","function":"count","join":9,
        "plan":plan_wire()});
    assert_eq!(serde_json::to_value(&count).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert!(decoded.dependencies().is_empty());
}

#[test]
fn owned_join_aggregate_wire_is_strict() {
    let legacy_plan = json!({"first":{"collection":["Rows"]},
        "second":{"source":{"collection":["Lookup","Rows"]},
            "conditions":{"first":{"left_collection":["Rows"],"left_path":["id"],"right_path":["id"]}}}});
    let mut duplicate = plan_wire();
    duplicate["second"]["source"]["collection"]["owner"] = json!({"kind":"primary"});
    let mut empty_owner = plan_wire();
    empty_owner["second"]["source"]["collection"]["owner"] = json!({"kind":"named","name":""});
    for wire in [
        // Legacy collection arrays never stand in for owned identities.
        json!({"kind":"owned_join_aggregate","function":"count","join":9,"plan":legacy_plan}),
        json!({"kind":"owned_join_aggregate","function":"count","join":9,"plan":duplicate}),
        json!({"kind":"owned_join_aggregate","function":"count","join":9,"plan":empty_owner}),
        json!({"kind":"owned_join_aggregate","function":"count","plan":plan_wire()}),
        json!({"kind":"owned_join_aggregate","join":9,"plan":plan_wire()}),
        json!({"kind":"owned_join_aggregate","function":"count","join":9}),
        json!({"kind":"owned_join_aggregate","function":"median","join":9,"plan":plan_wire()}),
        // Collection-aggregate and legacy-only keys are unknown here.
        json!({"kind":"owned_join_aggregate","function":"count","join":9,"plan":plan_wire(),
            "value":[]}),
        json!({"kind":"owned_join_aggregate","function":"count","join":9,"plan":plan_wire(),
            "collection":{"owner":{"kind":"primary"},"path":["Rows"]}}),
    ] {
        assert!(
            serde_json::from_value::<Node>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn previous_readers_refuse_owned_join_aggregates_and_legacy_wire_is_unchanged() {
    let node = Node::OwnedJoinAggregate(OwnedJoinAggregate {
        function: AggregateOp::Sum,
        join: JoinId::new(1),
        plan: plan(),
        expression: Some(2),
        arg: None,
    });
    let wire = serde_json::to_value(&node).unwrap();
    let error = serde_json::from_value::<PreviousNode>(wire).unwrap_err();
    assert!(
        error.to_string().contains("owned_join_aggregate"),
        "{error}"
    );

    let legacy_plan = JoinPlan::new(
        JoinSource::new(vec!["Rows".into()]),
        JoinSource::new(vec!["Lookup".into(), "Rows".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Rows".into()],
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    let legacy = Node::JoinAggregate {
        function: AggregateOp::Sum,
        join: JoinId::new(1),
        plan: legacy_plan,
        expression: Some(2),
        arg: None,
    };
    let wire = serde_json::to_value(&legacy).unwrap();
    assert_eq!(wire["kind"], "join_aggregate");
    assert_eq!(wire["plan"]["first"]["collection"], json!(["Rows"]));
    assert!(serde_json::from_value::<PreviousNode>(wire.clone()).is_ok());
    let decoded: Node = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
    assert_eq!(legacy.owned_source_kind(), None);
    assert_eq!(legacy.dependencies(), vec![2]);
}
