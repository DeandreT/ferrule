//! #446 strict owned Lookup and CollectionFind wire forms.
use crate::*;
use serde_json::json;

fn rates(owner: SourceOwner) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: vec!["Rates".into(), "Rate".into()],
    }
}

/// The pre-#446 reader's tagged node kinds relevant here.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousNode {
    Lookup {
        collection: Vec<String>,
        key: Vec<String>,
        matches: NodeId,
        value: Vec<String>,
    },
    CollectionFind {
        collection: Vec<String>,
        predicate: NodeId,
        value: NodeId,
    },
}

#[test]
fn owned_search_round_trips_with_exact_owner_and_relative_paths() {
    let lookup = Node::OwnedLookup(OwnedLookup {
        collection: rates(SourceOwner::Named {
            name: " Rates ".into(),
        }),
        key: vec!["Code".into()],
        matches: 3,
        value: vec![],
    });
    let wire = json!({"kind":"owned_lookup",
        "collection":{"owner":{"kind":"named","name":" Rates "},"path":["Rates","Rate"]},
        "key":["Code"],"matches":3,"value":[]});
    assert_eq!(serde_json::to_value(&lookup).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert!(matches!(&decoded, Node::OwnedLookup(found)
        if Node::OwnedLookup(found.clone()).owned_source_kind() == Some("owned_lookup")));
    assert_eq!(decoded.dependencies(), vec![3]);

    let find = Node::OwnedCollectionFind(OwnedCollectionFind {
        collection: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec![],
        },
        predicate: 4,
        value: 5,
    });
    let wire = json!({"kind":"owned_collection_find",
        "collection":{"owner":{"kind":"primary"},"path":[]},"predicate":4,"value":5});
    assert_eq!(serde_json::to_value(&find).unwrap(), wire);
    let decoded: Node = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.owned_source_kind(), Some("owned_collection_find"));
    assert_eq!(decoded.dependencies(), vec![4, 5]);
}

#[test]
fn owned_search_wire_is_strict() {
    for wire in [
        json!({"kind":"owned_lookup","collection":["Rates"],"key":[],"matches":1,"value":[]}),
        json!({"kind":"owned_lookup","collection":{"owner":{"kind":"primary"},"path":[]},
            "key":[],"value":[]}),
        json!({"kind":"owned_lookup","collection":{"owner":{"kind":"named","name":""},
            "path":[]},"key":[],"matches":1,"value":[]}),
        json!({"kind":"owned_lookup","collection":{"owner":{"kind":"primary"},"path":[]},
            "key":[],"matches":1,"value":[],"frame":[]}),
        json!({"kind":"owned_collection_find","collection":{"owner":{"kind":"primary"}},
            "predicate":1,"value":2}),
        json!({"kind":"owned_collection_find","collection":{"owner":{"kind":"primary"},
            "path":[]},"predicate":1}),
        json!({"kind":"owned_collection_find","collection":{"owner":{"kind":"primary"},
            "path":[]},"predicate":1,"value":2,"extra":true}),
    ] {
        assert!(
            serde_json::from_value::<Node>(wire.clone()).is_err(),
            "{wire}"
        );
    }
}

#[test]
fn previous_readers_refuse_owned_search_and_legacy_wire_is_unchanged() {
    for node in [
        Node::OwnedLookup(OwnedLookup {
            collection: rates(SourceOwner::Primary),
            key: vec!["Code".into()],
            matches: 1,
            value: vec!["Value".into()],
        }),
        Node::OwnedCollectionFind(OwnedCollectionFind {
            collection: rates(SourceOwner::Primary),
            predicate: 1,
            value: 2,
        }),
    ] {
        let wire = serde_json::to_value(&node).unwrap();
        let error = serde_json::from_value::<PreviousNode>(wire).unwrap_err();
        assert!(error.to_string().contains("owned_"), "{error}");
    }
    let legacy = Node::Lookup {
        collection: vec!["Rates".into()],
        key: vec!["Code".into()],
        matches: 1,
        value: vec!["Value".into()],
    };
    let wire = json!({"kind":"lookup","collection":["Rates"],"key":["Code"],"matches":1,
        "value":["Value"]});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), wire);
    assert!(serde_json::from_value::<PreviousNode>(wire).is_ok());
    assert_eq!(legacy.owned_source_kind(), None);
    let legacy = Node::CollectionFind {
        collection: vec!["Rates".into()],
        predicate: 1,
        value: 2,
    };
    let wire = json!({"kind":"collection_find","collection":["Rates"],"predicate":1,"value":2});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), wire);
    assert!(serde_json::from_value::<PreviousNode>(wire).is_ok());
    assert_eq!(legacy.owned_source_kind(), None);
}
