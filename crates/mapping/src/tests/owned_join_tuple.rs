//! #449 strict owned join tuple reads.
use crate::*;
use serde_json::json;

fn rows(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: vec!["Rows".into()],
    }
}

/// The pre-#449 reader's tagged join node kinds.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousNode {
    JoinField {
        join: JoinId,
        collection: Vec<String>,
        path: Vec<String>,
    },
    JoinPosition {
        join: JoinId,
    },
}

#[test]
fn owned_join_reads_round_trip_with_full_identity() {
    let field = Node::OwnedJoinField(OwnedJoinField {
        join: JoinId::new(7),
        collection: rows(SourceOwner::Named {
            name: " Lookup ".into(),
        }),
        path: vec!["Code".into()],
    });
    let wire = json!({"kind":"owned_join_field","join":7,
        "collection":{"owner":{"kind":"named","name":" Lookup "},"path":["Rows"]},
        "path":["Code"]});
    assert_eq!(serde_json::to_value(&field).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert!(matches!(&decoded, Node::OwnedJoinField(found)
        if Node::OwnedJoinField(found.clone()).owned_source_kind() == Some("owned_join_field")));
    assert!(decoded.dependencies().is_empty());

    let position = Node::OwnedJoinSourcePosition(OwnedJoinSourcePosition {
        join: JoinId::new(7),
        collection: rows(SourceOwner::Primary),
    });
    let wire = json!({"kind":"owned_join_source_position","join":7,
        "collection":{"owner":{"kind":"primary"},"path":["Rows"]}});
    assert_eq!(serde_json::to_value(&position).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert_eq!(
        decoded.owned_source_kind(),
        Some("owned_join_source_position")
    );
    assert!(decoded.dependencies().is_empty());
}

#[test]
fn owned_join_read_wire_is_strict() {
    for wire in [
        // Legacy collection arrays never stand in for an owned identity.
        json!({"kind":"owned_join_field","join":7,"collection":["Rows"],"path":[]}),
        json!({"kind":"owned_join_field","collection":{"owner":{"kind":"primary"},
            "path":["Rows"]},"path":[]}),
        json!({"kind":"owned_join_field","join":7,"collection":{"owner":{"kind":"primary"},
            "path":["Rows"]}}),
        json!({"kind":"owned_join_field","join":7,"collection":{"owner":{"kind":"named",
            "name":""},"path":["Rows"]},"path":[]}),
        json!({"kind":"owned_join_field","join":7,"collection":{"owner":{"kind":"primary"},
            "path":["Rows"]},"path":[],"frame":[]}),
        json!({"kind":"owned_join_source_position","join":7,"collection":["Rows"]}),
        json!({"kind":"owned_join_source_position","join":7}),
        json!({"kind":"owned_join_source_position","join":7,"collection":{"owner":
            {"kind":"primary"},"path":["Rows"]},"path":[]}),
    ] {
        assert!(
            serde_json::from_value::<Node>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn previous_readers_refuse_owned_join_reads_and_legacy_wire_is_unchanged() {
    for node in [
        Node::OwnedJoinField(OwnedJoinField {
            join: JoinId::new(1),
            collection: rows(SourceOwner::Primary),
            path: vec!["Code".into()],
        }),
        Node::OwnedJoinSourcePosition(OwnedJoinSourcePosition {
            join: JoinId::new(1),
            collection: rows(SourceOwner::Primary),
        }),
    ] {
        let wire = serde_json::to_value(&node).unwrap();
        let error = serde_json::from_value::<PreviousNode>(wire).unwrap_err();
        assert!(error.to_string().contains("owned_join_"), "{error}");
    }
    let legacy = Node::JoinField {
        join: JoinId::new(1),
        collection: vec!["Rows".into()],
        path: vec!["Code".into()],
    };
    let wire = json!({"kind":"join_field","join":1,"collection":["Rows"],"path":["Code"]});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), wire);
    assert!(serde_json::from_value::<PreviousNode>(wire).is_ok());
    assert_eq!(legacy.owned_source_kind(), None);
    let legacy = Node::JoinPosition {
        join: JoinId::new(1),
    };
    let wire = json!({"kind":"join_position","join":1});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), wire);
    assert!(serde_json::from_value::<PreviousNode>(wire).is_ok());
    assert_eq!(legacy.owned_source_kind(), None);
}
