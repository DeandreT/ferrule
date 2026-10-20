use crate::*;
use serde_json::json;

fn owned(owner: SourceOwner, path: &[&str]) -> SequenceExpr {
    SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner,
            path: path.iter().map(|segment| (*segment).into()).collect(),
        },
        children: vec!["Dir".into()],
        descent_value: vec!["Note".into()],
        values: vec!["file".into()],
        value: vec!["Note".into()],
        prefix: 1,
        separator: 2,
        item: 3,
    })
}

fn wire(owner: serde_json::Value, path: &[&str]) -> serde_json::Value {
    json!({"kind":"owned_recursive_collect",
        "collection":{"owner":owner,"path":path},
        "children":["Dir"],"descent_value":["Note"],"values":["file"],"value":["Note"],
        "prefix":1,"separator":2,"item":3})
}

/// The pre-#463 reader's exact tagged representation.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousSequence {
    Generate {
        #[serde(default)]
        from: Option<NodeId>,
        to: NodeId,
        item: NodeId,
    },
    RecursiveCollect {
        collection: Vec<String>,
        children: Vec<String>,
        descent_value: Vec<String>,
        values: Vec<String>,
        value: Vec<String>,
        prefix: NodeId,
        separator: NodeId,
        item: NodeId,
    },
}

#[test]
fn owned_recursive_collect_round_trips_exact_owner_and_relative_paths() {
    for (sequence, expected) in [
        (
            owned(SourceOwner::Primary, &["Dir"]),
            wire(json!({"kind":"primary"}), &["Dir"]),
        ),
        (
            owned(
                SourceOwner::Named {
                    name: " Lookup ".into(),
                },
                &["Batches", "Dir"],
            ),
            wire(
                json!({"kind":"named","name":" Lookup "}),
                &["Batches", "Dir"],
            ),
        ),
        (
            owned(
                SourceOwner::Named {
                    name: "Tree".into(),
                },
                &[],
            ),
            wire(json!({"kind":"named","name":"Tree"}), &[]),
        ),
    ] {
        assert_eq!(serde_json::to_value(&sequence).unwrap(), expected);
        let decoded: SequenceExpr = serde_json::from_value(expected).unwrap();
        assert_eq!(decoded, sequence);
        assert_eq!(decoded.kind_name(), "owned_recursive_collect");
        assert_eq!(decoded.inputs(), vec![1, 2]);
        // The generated item stays a private declaration, never an owner.
        assert_eq!(decoded.item(), 3);
        assert_eq!(decoded.owned_items(), vec![3]);
        assert_eq!(decoded.owned_source(), Some(&sequence_source(&sequence)));
    }
}

fn sequence_source(sequence: &SequenceExpr) -> OwnedSourceRef {
    let SequenceExpr::OwnedRecursiveCollect(collect) = sequence else {
        unreachable!("authored owned sequence");
    };
    collect.collection.clone()
}

#[test]
fn owned_recursive_collect_is_strict() {
    let valid = wire(json!({"kind":"primary"}), &["Dir"]);
    assert!(serde_json::from_value::<SequenceExpr>(valid.clone()).is_ok());
    let with = |key: &str, value: serde_json::Value| {
        let mut wire = valid.clone();
        wire[key] = value;
        wire
    };
    let without = |key: &str| {
        let mut wire = valid.clone();
        wire.as_object_mut().unwrap().remove(key);
        wire
    };
    for invalid in [
        // A legacy path array is not an owner.
        with("collection", json!(["Dir"])),
        with("collection", json!({"owner":{"kind":"primary"}})),
        with("collection", json!({"path":["Dir"]})),
        with(
            "collection",
            json!({"owner":{"kind":"named","name":""},"path":["Dir"]}),
        ),
        with(
            "collection",
            json!({"owner":{"kind":"named"},"path":["Dir"]}),
        ),
        with(
            "collection",
            json!({"owner":{"kind":"primary","name":"x"},"path":["Dir"]}),
        ),
        with(
            "collection",
            json!({"owner":{"kind":"primary"},"path":["Dir"],"frame":[]}),
        ),
        with("collection", serde_json::Value::Null),
        // Relative paths and item declarations keep their legacy shape.
        with(
            "children",
            json!({"owner":{"kind":"primary"},"path":["Dir"]}),
        ),
        with("owner", json!({"kind":"primary"})),
        with("extra", json!(true)),
        without("collection"),
        without("children"),
        without("value"),
        without("prefix"),
        without("item"),
    ] {
        assert!(
            serde_json::from_value::<SequenceExpr>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
    let empty_name = owned(
        SourceOwner::Named {
            name: String::new(),
        },
        &["Dir"],
    );
    assert!(serde_json::to_value(&empty_name).is_err());
}

#[test]
fn legacy_recursive_collect_wire_is_unchanged_and_old_readers_refuse_owned() {
    let legacy = SequenceExpr::RecursiveCollect {
        collection: vec!["Dir".into()],
        children: vec!["Dir".into()],
        descent_value: vec!["Note".into()],
        values: vec!["file".into()],
        value: vec!["Note".into()],
        prefix: 1,
        separator: 2,
        item: 3,
    };
    let legacy_wire = json!({"kind":"recursive_collect","collection":["Dir"],
        "children":["Dir"],"descent_value":["Note"],"values":["file"],"value":["Note"],
        "prefix":1,"separator":2,"item":3});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), legacy_wire);
    assert!(legacy.owned_source().is_none());
    // The legacy reader stays tolerant; an owned-looking key never grants an owner.
    let mut tolerant = legacy_wire.clone();
    tolerant["owner"] = json!({"kind":"named","name":"Lookup"});
    assert_eq!(
        serde_json::from_value::<SequenceExpr>(tolerant).unwrap(),
        legacy
    );
    assert!(serde_json::from_value::<PreviousSequence>(legacy_wire).is_ok());

    let owned_wire = serde_json::to_value(owned(SourceOwner::Primary, &["Dir"])).unwrap();
    let error = serde_json::from_value::<PreviousSequence>(owned_wire).unwrap_err();
    assert!(
        error.to_string().contains("owned_recursive_collect"),
        "{error}"
    );
}

#[test]
fn owned_producers_mark_their_consumers_and_scopes() {
    let item_at = Node::SequenceItemAt {
        sequence: owned(SourceOwner::Primary, &["Dir"]),
        index: 4,
    };
    assert_eq!(item_at.owned_source_kind(), Some("owned_recursive_collect"));
    let exists = Node::SequenceExists {
        sequence: owned(SourceOwner::Primary, &["Dir"]),
        predicate: 4,
    };
    assert_eq!(exists.owned_source_kind(), Some("owned_recursive_collect"));
    let legacy = Node::SequenceItemAt {
        sequence: SequenceExpr::Generate {
            from: None,
            to: 1,
            item: 3,
        },
        index: 4,
    };
    assert_eq!(legacy.owned_source_kind(), None);

    let mut root = Scope::default();
    root.children.push(Scope {
        target_field: "Files".into(),
        iteration: ScopeIteration::Sequence(owned(SourceOwner::Primary, &["Dir"])),
        ..Scope::default()
    });
    assert_eq!(
        first_owned_source_scope(&root, SourceScopeLocation::root(SourceScopeTarget::Primary)),
        Some(
            SourceScopeLocation::root(SourceScopeTarget::Primary)
                .descendant(SourceScopeStep::Child(0))
        )
    );
}
