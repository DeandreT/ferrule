use crate::*;
use serde_json::json;

fn owned(owner: SourceOwner, object: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedDynamicSourceField(OwnedDynamicSourceField {
        object: OwnedSourceRef {
            owner,
            path: object.iter().map(|segment| (*segment).into()).collect(),
        },
        frame: frame.map(|frame| frame.iter().map(|segment| (*segment).into()).collect()),
        key: 7,
    })
}

/// The pre-#467 reader's exact tagged representation of dynamic reads.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousNode {
    Const {
        value: ir::Value,
    },
    DynamicSourceField {
        object: Vec<String>,
        #[serde(default)]
        frame: Option<Vec<String>>,
        key: NodeId,
    },
}

#[test]
fn owned_dynamic_field_round_trips_owner_object_and_frame_distinctions() {
    for (node, expected) in [
        (
            owned(SourceOwner::Primary, &["Rows", "Props"], None),
            json!({"kind":"owned_dynamic_source_field",
                "object":{"owner":{"kind":"primary"},"path":["Rows","Props"]},"key":7}),
        ),
        (
            owned(
                SourceOwner::Named {
                    name: " Lookup ".into(),
                },
                &["Rows", "Props"],
                Some(&["Rows"]),
            ),
            json!({"kind":"owned_dynamic_source_field",
                "object":{"owner":{"kind":"named","name":" Lookup "},"path":["Rows","Props"]},
                "frame":["Rows"],"key":7}),
        ),
        // An open endpoint root and an explicit root pin stay distinct from
        // an absent frame.
        (
            owned(
                SourceOwner::Named {
                    name: "Settings".into(),
                },
                &[],
                None,
            ),
            json!({"kind":"owned_dynamic_source_field",
                "object":{"owner":{"kind":"named","name":"Settings"},"path":[]},"key":7}),
        ),
        (
            owned(SourceOwner::Primary, &["Props"], Some(&[])),
            json!({"kind":"owned_dynamic_source_field",
                "object":{"owner":{"kind":"primary"},"path":["Props"]},"frame":[],"key":7}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&node).unwrap(), expected);
        let decoded: Node = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), expected);
        assert_eq!(
            decoded.owned_source_kind(),
            Some("owned_dynamic_source_field")
        );
        assert_eq!(decoded.dependencies(), vec![7]);
        let (Node::OwnedDynamicSourceField(decoded), Node::OwnedDynamicSourceField(node)) =
            (&decoded, &node)
        else {
            unreachable!("authored owned dynamic read");
        };
        assert_eq!(decoded, node);
    }
}

#[test]
fn owned_dynamic_field_is_strict() {
    let valid = json!({"kind":"owned_dynamic_source_field",
        "object":{"owner":{"kind":"primary"},"path":["Props"]},"frame":[],"key":7});
    assert!(serde_json::from_value::<Node>(valid.clone()).is_ok());
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
        with("object", json!(["Props"])),
        with("object", json!({"owner":{"kind":"primary"}})),
        with("object", json!({"path":["Props"]})),
        with(
            "object",
            json!({"owner":{"kind":"named","name":""},"path":["Props"]}),
        ),
        with("object", json!({"owner":{"kind":"named"},"path":["Props"]})),
        with(
            "object",
            json!({"owner":{"kind":"primary","name":"x"},"path":["Props"]}),
        ),
        with(
            "object",
            json!({"owner":{"kind":"unknown"},"path":["Props"]}),
        ),
        with(
            "object",
            json!({"owner":{"kind":"primary"},"path":["Props"],"frame":[]}),
        ),
        with("object", serde_json::Value::Null),
        with("frame", json!("Props")),
        with("key", json!("name")),
        with("owner", json!({"kind":"primary"})),
        with("extra", json!(true)),
        without("object"),
        without("key"),
    ] {
        assert!(
            serde_json::from_value::<Node>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
    // Duplicate body keys and duplicate owner fields are refused as well.
    for duplicate in [
        r#"{"kind":"owned_dynamic_source_field","object":{"owner":{"kind":"primary"},"path":[]},"key":7,"key":8}"#,
        r#"{"kind":"owned_dynamic_source_field","object":{"owner":{"kind":"primary"},"path":[]},"frame":[],"frame":[],"key":7}"#,
        r#"{"kind":"owned_dynamic_source_field","object":{"owner":{"kind":"primary"},"path":[]},"object":{"owner":{"kind":"primary"},"path":[]},"key":7}"#,
        r#"{"kind":"owned_dynamic_source_field","object":{"owner":{"kind":"primary"},"owner":{"kind":"primary"},"path":[]},"key":7}"#,
    ] {
        assert!(
            serde_json::from_str::<Node>(duplicate).is_err(),
            "{duplicate}"
        );
    }
    let empty_name = owned(
        SourceOwner::Named {
            name: String::new(),
        },
        &["Props"],
        None,
    );
    assert!(serde_json::to_value(&empty_name).is_err());
}

#[test]
fn legacy_dynamic_field_wire_is_unchanged_and_old_readers_refuse_owned() {
    let legacy = Node::DynamicSourceField {
        object: vec!["Props".into()],
        frame: Some(vec!["Rows".into()]),
        key: 7,
    };
    let legacy_wire =
        json!({"kind":"dynamic_source_field","object":["Props"],"frame":["Rows"],"key":7});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), legacy_wire);
    assert_eq!(legacy.owned_source_kind(), None);
    let unframed = Node::DynamicSourceField {
        object: vec![],
        frame: None,
        key: 7,
    };
    assert_eq!(
        serde_json::to_value(&unframed).unwrap(),
        json!({"kind":"dynamic_source_field","object":[],"key":7})
    );
    // The legacy reader stays tolerant; an owned-looking key never grants an owner.
    let mut tolerant = legacy_wire.clone();
    tolerant["owner"] = json!({"kind":"named","name":"Lookup"});
    let decoded = serde_json::from_value::<Node>(tolerant).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), legacy_wire);
    assert_eq!(decoded.owned_source_kind(), None);
    assert!(serde_json::from_value::<PreviousNode>(legacy_wire).is_ok());

    let owned_wire = serde_json::to_value(owned(SourceOwner::Primary, &["Props"], None)).unwrap();
    let error = serde_json::from_value::<PreviousNode>(owned_wire).unwrap_err();
    assert!(
        error.to_string().contains("owned_dynamic_source_field"),
        "{error}"
    );
}
