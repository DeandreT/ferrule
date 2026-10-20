use crate::*;
use serde_json::json;

fn rule(iteration: FailureIteration) -> FailureRule {
    FailureRule {
        iteration,
        selection: FailureSelection::WhenTrue { predicate: 7 },
        message: None,
    }
}

fn owned(owner: SourceOwner, path: &[&str]) -> FailureIteration {
    FailureIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|segment| (*segment).into()).collect(),
        },
    }
}

/// The pre-#471 reader's exact tagged representation.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(dead_code)]
enum PreviousFailureIteration {
    Source { collection: Vec<String> },
    Sequence { sequence: SequenceExpr },
}

#[test]
fn owned_iteration_round_trips_with_exact_owner_and_path() {
    for (iteration, wire) in [
        (
            owned(SourceOwner::Primary, &["Rows"]),
            json!({"kind":"owned_source","source":{"owner":{"kind":"primary"},"path":["Rows"]}}),
        ),
        (
            owned(
                SourceOwner::Named {
                    name: " Lookup ".into(),
                },
                &["Rows", "Lines"],
            ),
            json!({"kind":"owned_source","source":{"owner":{"kind":"named","name":" Lookup "},
                "path":["Rows","Lines"]}}),
        ),
        (
            owned(SourceOwner::Primary, &[]),
            json!({"kind":"owned_source","source":{"owner":{"kind":"primary"},"path":[]}}),
        ),
    ] {
        assert_eq!(serde_json::to_value(&iteration).unwrap(), wire);
        let decoded: FailureIteration = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, iteration);
        assert!(decoded.owned_source().is_some());
    }
}

#[test]
fn legacy_failure_wire_and_tolerance_are_unchanged() {
    let legacy = FailureIteration::Source {
        collection: vec!["Rows".into()],
    };
    assert_eq!(
        serde_json::to_value(&legacy).unwrap(),
        json!({"kind":"source","collection":["Rows"]})
    );
    assert!(legacy.owned_source().is_none());
    // Legacy readers ignored unknown keys; an owned-looking key is not an owner.
    let decoded: FailureIteration = serde_json::from_value(json!({"kind":"source",
        "collection":["Rows"],"source":{"owner":{"kind":"primary"},"path":["Rows"]}}))
    .unwrap();
    assert_eq!(decoded, legacy);
    let sequence: FailureIteration = serde_json::from_value(json!({"kind":"sequence",
        "sequence":{"kind":"generate","to":3,"item":4},"extra":true}))
    .unwrap();
    assert!(matches!(sequence, FailureIteration::Sequence { .. }));
    let saved = serde_json::to_value(rule(legacy)).unwrap();
    assert_eq!(
        saved,
        json!({"iteration":{"kind":"source","collection":["Rows"]},
            "selection":{"kind":"when_true","predicate":7}})
    );
}

#[test]
fn owned_failure_iteration_is_strict() {
    for wire in [
        json!({"kind":"owned_source"}),
        json!({"kind":"owned_source","source":["Rows"]}),
        json!({"kind":"owned_source","source":{"owner":{"kind":"primary"}}}),
        json!({"kind":"owned_source","source":{"path":["Rows"]}}),
        json!({"kind":"owned_source","source":{"owner":{"kind":"named","name":""},"path":["Rows"]}}),
        json!({"kind":"owned_source","source":{"owner":{"kind":"primary","name":"x"},"path":[]}}),
        json!({"kind":"owned_source","source":{"owner":{"kind":"primary"},"path":[],"frame":[]}}),
        json!({"kind":"owned_source","source":{"owner":{"kind":"primary"},"path":["Rows"]},
            "collection":["Rows"]}),
        json!({"kind":"owned_source","source":{"owner":{"kind":"primary"},"path":["Rows"]},
            "extra":true}),
        json!({"kind":"owned_source","source":null}),
    ] {
        assert!(
            serde_json::from_value::<FailureIteration>(wire.clone()).is_err(),
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
fn previous_readers_refuse_owned_failure_iteration() {
    let wire = serde_json::to_value(owned(SourceOwner::Primary, &["Rows"])).unwrap();
    let error = serde_json::from_value::<PreviousFailureIteration>(wire).unwrap_err();
    assert!(error.to_string().contains("owned_source"), "{error}");
    let legacy = serde_json::to_value(FailureIteration::Source {
        collection: vec!["Rows".into()],
    })
    .unwrap();
    assert!(serde_json::from_value::<PreviousFailureIteration>(legacy).is_ok());
}
