use mapping::{Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};

#[test]
fn owned_node_wire_is_strict_without_changing_legacy_defaults() {
    let field = Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
            path: vec!["Rows".into(), "Value".into()],
        },
        frame: Some(vec!["Rows".into()]),
    });
    let encoded = serde_json::to_string(&field).unwrap();
    assert_eq!(
        encoded,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"named","name":" Ref 雪 "},"path":["Rows","Value"]},"frame":["Rows"]}"#
    );
    let decoded: Node = serde_json::from_str(&encoded).unwrap();
    let Node::OwnedSourceField(decoded_field) = &decoded else {
        panic!("owned source field variant expected");
    };
    let SourceOwner::Named { name } = &decoded_field.source.owner else {
        panic!("named source owner expected");
    };
    assert_eq!(name, " Ref 雪 ");
    assert_eq!(
        decoded_field.source.path,
        vec!["Rows".to_string(), "Value".to_string()]
    );
    assert_eq!(decoded_field.frame, Some(vec!["Rows".to_string()]));
    assert_eq!(
        serde_json::to_string(&decoded).unwrap(),
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"named","name":" Ref 雪 "},"path":["Rows","Value"]},"frame":["Rows"]}"#
    );
    assert!(field.dependencies().is_empty());
    let legacy: Node =
        serde_json::from_str(r#"{"kind":"position","extra":"still accepted"}"#).unwrap();
    let Node::Position { collection } = &legacy else {
        panic!("legacy position variant expected");
    };
    assert!(collection.is_empty());
    assert_eq!(
        serde_json::to_string(&legacy).unwrap(),
        r#"{"kind":"position"}"#
    );
    let legacy: Node =
        serde_json::from_str(r#"{"kind":"source_field","path":[],"extra":0}"#).unwrap();
    let Node::SourceField { path, frame } = &legacy else {
        panic!("legacy source field variant expected");
    };
    assert!(path.is_empty());
    assert!(frame.is_none());
    assert_eq!(
        serde_json::to_string(&legacy).unwrap(),
        r#"{"kind":"source_field","path":[]}"#
    );
}

#[test]
fn owned_position_preserves_unresolved_and_explicit_root_states() {
    for (collection, expected) in [
        (
            None,
            r#"{"kind":"owned_position","owner":{"kind":"primary"}}"#,
        ),
        (
            Some(vec![]),
            r#"{"kind":"owned_position","owner":{"kind":"primary"},"collection":[]}"#,
        ),
        (
            Some(vec!["雪".into()]),
            r#"{"kind":"owned_position","owner":{"kind":"primary"},"collection":["雪"]}"#,
        ),
    ] {
        let expected_collection: Option<Vec<String>> = collection;
        let node = Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: expected_collection.clone(),
        });
        assert_eq!(serde_json::to_string(&node).unwrap(), expected);
        let decoded: Node = serde_json::from_str(expected).unwrap();
        let Node::OwnedPosition(decoded_position) = &decoded else {
            panic!("owned position variant expected");
        };
        assert!(matches!(decoded_position.owner, SourceOwner::Primary));
        assert_eq!(decoded_position.collection, expected_collection);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), expected);
        assert!(node.dependencies().is_empty());
    }
    let null: Node = serde_json::from_str(
        r#"{"kind":"owned_position","owner":{"kind":"primary"},"collection":null}"#,
    )
    .unwrap();
    let Node::OwnedPosition(null_position) = &null else {
        panic!("owned position variant expected");
    };
    assert!(matches!(null_position.owner, SourceOwner::Primary));
    assert!(null_position.collection.is_none());
    assert_eq!(
        serde_json::to_string(&null).unwrap(),
        r#"{"kind":"owned_position","owner":{"kind":"primary"}}"#
    );
}

#[test]
fn new_outer_and_nested_maps_reject_unknown_duplicate_and_missing_fields() {
    for invalid in [
        r#"{"kind":"owned_source_field"}"#,
        r#"{"kind":"owned_source_field","source":null}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"},"path":[]},"extra":0}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"},"path":[]},"source":{"owner":{"kind":"primary"},"path":[]}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"},"path":[]},"frame":[],"frame":[]}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"}}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"},"path":[],"extra":0}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"},"path":[],"path":[]}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary"},"owner":{"kind":"primary"},"path":[]}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"named","name":""},"path":[]}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"named","name":"Ref","name":"Ref"},"path":[]}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary","kind":"primary"},"path":[]}}"#,
        r#"{"kind":"owned_source_field","source":{"owner":{"kind":"primary","extra":0},"path":[]}}"#,
        r#"{"kind":"owned_position","owner":null}"#,
        r#"{"kind":"owned_position","owner":{"kind":"primary"},"owner":{"kind":"primary"}}"#,
        r#"{"kind":"owned_position","owner":{"kind":"primary"},"collection":[],"collection":[]}"#,
        r#"{"kind":"owned_position","owner":{"kind":"primary"},"collection":"Rows"}"#,
        r#"{"kind":"owned_position","owner":{"kind":"primary"},"extra":0}"#,
    ] {
        assert!(serde_json::from_str::<Node>(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn a_legacy_kind_reader_refuses_the_new_wire_kinds() {
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum LegacyKind {
        SourceField,
        Position,
    }
    assert!(serde_json::from_str::<LegacyKind>(r#"{"kind":"source_field","path":[]}"#).is_ok());
    assert!(serde_json::from_str::<LegacyKind>(r#"{"kind":"position","collection":[]}"#).is_ok());
    for kind in ["owned_source_field", "owned_position"] {
        let body = format!(r#"{{"kind":"{kind}"}}"#);
        assert!(serde_json::from_str::<LegacyKind>(&body).is_err());
    }
}
