use crate::*;
use serde_json::json;

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn owned(owner: SourceOwner, segments: &[&str]) -> DynamicSourcePath {
    DynamicSourcePath::owned(
        5,
        OwnedSourceRef {
            owner,
            path: path(segments),
        },
    )
}

/// The pre-#475 reader's exact representation.
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct PreviousDynamicSourcePath {
    node: NodeId,
    iteration: Vec<String>,
}

#[test]
fn legacy_driver_wire_and_tolerance_are_unchanged() {
    let legacy = DynamicSourcePath::legacy(5, path(&["Files", "File"]));
    let wire = json!({"node":5,"iteration":["Files","File"]});
    assert_eq!(serde_json::to_value(&legacy).unwrap(), wire);
    let decoded: DynamicSourcePath = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        decoded.legacy_driver(),
        Some(path(&["Files", "File"]).as_slice())
    );
    assert!(decoded.owned_driver().is_none());
    // Legacy readers ignored unknown keys; an owned-looking key is not a driver.
    let tolerant: DynamicSourcePath = serde_json::from_value(json!({"node":5,
        "iteration":[],"driver":{"owner":{"kind":"primary"},"path":["Rows"]}}))
    .unwrap();
    assert_eq!(tolerant.legacy_driver(), Some(&[][..]));
    assert!(serde_json::from_value::<PreviousDynamicSourcePath>(wire).is_ok());
    assert!(serde_json::from_value::<DynamicSourcePath>(json!({"node":5})).is_err());
}

#[test]
fn owned_driver_round_trips_with_exact_owner_and_path() {
    for (dynamic, wire) in [
        (
            owned(SourceOwner::Primary, &["Rows"]),
            json!({"node":5,"iteration":{"owner":{"kind":"primary"},"path":["Rows"]}}),
        ),
        (
            owned(
                SourceOwner::Named {
                    name: " Catalog ".into(),
                },
                &["Files", "File"],
            ),
            json!({"node":5,"iteration":{"owner":{"kind":"named","name":" Catalog "},
                "path":["Files","File"]}}),
        ),
        (
            owned(SourceOwner::Primary, &[]),
            json!({"node":5,"iteration":{"owner":{"kind":"primary"},"path":[]}}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&dynamic).unwrap(), wire);
        let decoded: DynamicSourcePath = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded.owned_driver(), dynamic.owned_driver());
        assert!(decoded.legacy_driver().is_none());
    }
}

#[test]
fn owned_driver_is_strict() {
    for wire in [
        json!({"node":5,"iteration":{"owner":{"kind":"primary"}}}),
        json!({"node":5,"iteration":{"path":["Rows"]}}),
        json!({"node":5,"iteration":{"owner":{"kind":"named","name":""},"path":["Rows"]}}),
        json!({"node":5,"iteration":{"owner":{"kind":"primary","name":"x"},"path":[]}}),
        json!({"node":5,"iteration":{"owner":{"kind":"primary"},"path":["Rows"],"frame":[]}}),
        json!({"node":5,"iteration":{"owner":{"kind":"primary"},"path":["Rows"],"extra":1}}),
        json!({"node":5,"iteration":{"owner":"primary","path":["Rows"]}}),
        json!({"node":5,"iteration":null}),
        json!({"node":5,"iteration":"Rows"}),
    ] {
        assert!(
            serde_json::from_value::<DynamicSourcePath>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let empty_name = owned(
        SourceOwner::Named {
            name: String::new(),
        },
        &["Rows"],
    );
    assert!(serde_json::to_value(&empty_name).is_err());
}

#[test]
fn previous_readers_refuse_owned_drivers() {
    let wire = serde_json::to_value(owned(SourceOwner::Primary, &["Rows"])).unwrap();
    let error = serde_json::from_value::<PreviousDynamicSourcePath>(wire).unwrap_err();
    assert!(error.to_string().contains("expected a sequence"), "{error}");
}

#[test]
fn named_source_keeps_its_owned_driver_through_the_project_codec() {
    let source = NamedSource {
        name: "Docs".into(),
        path: "docs.json".into(),
        schema: ir::SchemaNode::group("Doc", Vec::new()),
        options: FormatOptions::default(),
        dynamic_path: Some(owned(
            SourceOwner::Named {
                name: "Catalog".into(),
            },
            &["Files"],
        )),
    };
    let wire = serde_json::to_value(&source).unwrap();
    assert_eq!(
        wire["dynamic_path"],
        json!({"node":5,"iteration":{"owner":{"kind":"named","name":"Catalog"},"path":["Files"]}})
    );
    let decoded: NamedSource = serde_json::from_value(wire).unwrap();
    assert_eq!(
        decoded
            .dynamic_path
            .as_ref()
            .and_then(DynamicSourcePath::owned_driver),
        source
            .dynamic_path
            .as_ref()
            .and_then(DynamicSourcePath::owned_driver)
    );
}
