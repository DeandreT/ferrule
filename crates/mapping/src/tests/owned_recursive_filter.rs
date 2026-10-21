use crate::*;
use serde_json::json;

fn plan() -> RecursiveFilterPlan {
    RecursiveFilterPlan::new("Dir".into(), "file".into(), 7).unwrap()
}

fn owned(owner: SourceOwner, path: &[&str]) -> Scope {
    Scope {
        target_field: "Dir".into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: path.iter().map(|segment| (*segment).into()).collect(),
            },
            output_path: None,
        },
        construction: ScopeConstruction::RecursiveFilter { plan: plan() },
        ..Scope::default()
    }
}

#[test]
fn owned_recursive_filter_keeps_one_selector_owner_and_relative_plan() {
    for (owner, wire_owner) in [
        (SourceOwner::Primary, json!({"kind":"primary"})),
        (
            SourceOwner::Named {
                name: " Lookup ".into(),
            },
            json!({"kind":"named","name":" Lookup "}),
        ),
    ] {
        let scope = owned(owner.clone(), &["Dir"]);
        let value = serde_json::to_value(&scope).unwrap();
        assert_eq!(
            value["source"],
            json!({"owner": wire_owner, "path": ["Dir"]})
        );
        assert_eq!(
            value["construction"],
            json!({"recursive_filter":{"plan":{"children":"Dir","items":"file","predicate":7}}})
        );
        // Exactly one owner is serialized: the scope selector's.
        assert_eq!(value.to_string().matches("\"owner\"").count(), 1);
        let decoded: Scope = serde_json::from_value(value.clone()).unwrap();
        let (source, plan) = decoded.owned_recursive_filter().unwrap();
        assert_eq!(source.owner, owner);
        assert_eq!(source.path, vec!["Dir".to_owned()]);
        assert_eq!((plan.children(), plan.items()), ("Dir", "file"));
        assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
    }
}

#[test]
fn legacy_recursive_filter_wire_and_defaults_are_unchanged() {
    let legacy = Scope {
        target_field: "Dir".into(),
        iteration: ScopeIteration::Source(vec!["Dir".into()]),
        construction: ScopeConstruction::RecursiveFilter { plan: plan() },
        ..Scope::default()
    };
    assert!(legacy.owned_recursive_filter().is_none());
    let value = serde_json::to_value(&legacy).unwrap();
    assert_eq!(value["source"], json!(["Dir"]));
    assert!(!value.to_string().contains("owner"));
    let decoded: Scope = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.construction, legacy.construction);
    // Other unknown plan keys keep their legacy, ignored meaning.
    let tolerant: RecursiveFilterPlan = serde_json::from_value(
        json!({"children":"Dir","items":"file","predicate":7,"legacy_unknown":true}),
    )
    .unwrap();
    assert_eq!(tolerant, plan());
    // A plan without an owned selector never becomes owned.
    let unowned = Scope {
        construction: ScopeConstruction::RecursiveFilter { plan: plan() },
        ..Scope::default()
    };
    assert!(unowned.owned_recursive_filter().is_none());
}

#[test]
fn plan_refuses_a_second_owner_instead_of_ignoring_it() {
    for owner in [
        json!({"kind":"named","name":"Lookup"}),
        json!({"kind":"primary"}),
        json!(null),
    ] {
        let error = serde_json::from_value::<RecursiveFilterPlan>(
            json!({"children":"Dir","items":"file","predicate":7,"owner":owner}),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("scope's owned source selector"),
            "{error}"
        );
    }
    let scope = json!({
        "source": {"owner":{"kind":"primary"},"path":["Dir"]},
        "construction": {"recursive_filter":{"plan":{"children":"Dir","items":"file",
            "predicate":7,"owner":{"kind":"named","name":"Lookup"}}}}
    });
    assert!(serde_json::from_value::<Scope>(scope).is_err());
    // The plan's existing structural refusals still apply.
    assert!(
        serde_json::from_value::<RecursiveFilterPlan>(
            json!({"children":"Dir","items":"Dir","predicate":7})
        )
        .is_err()
    );
}
