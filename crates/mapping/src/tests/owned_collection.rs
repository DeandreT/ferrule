use crate::*;
use ir::{SchemaNode, Value};
use std::collections::BTreeMap;

fn owned(owner: SourceOwner, path: &[&str], output_path: Option<NodeId>) -> Scope {
    Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: path.iter().map(|part| (*part).into()).collect(),
            },
            output_path,
        },
        ..Scope::default()
    }
}
fn primary() -> Scope {
    owned(SourceOwner::Primary, &[], None)
}
fn project(root: Scope) -> Project {
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root,
    }
}

#[test]
fn absent_null_empty_legacy_and_owned_root_remain_distinct() {
    for text in ["{}", r#"{"source":null}"#] {
        let scope: Scope = serde_json::from_str(text).unwrap();
        assert!(matches!(scope.iteration, ScopeIteration::None));
        assert_eq!(
            serde_json::to_value(&scope).unwrap()["source"],
            serde_json::Value::Null
        );
    }
    let legacy: Scope = serde_json::from_str(r#"{"source":[]}"#).unwrap();
    assert!(matches!(legacy.iteration, ScopeIteration::Source(ref path) if path.is_empty()));
    assert_eq!(
        serde_json::to_value(&legacy).unwrap()["source"],
        serde_json::json!([])
    );
    let explicit = primary();
    let value = serde_json::to_value(&explicit).unwrap();
    assert_eq!(
        value["source"],
        serde_json::json!({"owner":{"kind":"primary"},"path":[]})
    );
    let decoded: Scope = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.source_selector(), explicit.source_selector());
    assert_eq!(decoded.source(), Err(OwnedSourceAccess));
}

#[test]
fn old_path_only_scope_reader_refuses_new_owned_wire_object() {
    #[derive(serde::Deserialize)]
    struct OldScope {
        #[serde(default)]
        source: Option<Vec<String>>,
    }
    for scope in [
        primary(),
        owned(
            SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
            &["Rows"],
            Some(7),
        ),
    ] {
        let text = serde_json::to_string(&scope).unwrap();
        assert!(serde_json::from_str::<OldScope>(&text).is_err(), "{text}");
    }
    for text in ["{}", r#"{"source":null}"#, r#"{"source":[]}"#] {
        let legacy: OldScope = serde_json::from_str(text).unwrap();
        assert_eq!(legacy.source.is_some(), text.contains("[]"));
    }
}

#[test]
fn owned_wire_is_strict_without_changing_legacy_scope_unknown_fields() {
    for source in [
        r#"{"owner":{"kind":"primary"},"path":[],"extra":1}"#,
        r#"{"owner":{"kind":"primary","name":"ignored"},"path":[]}"#,
        r#"{"owner":{"kind":"named","name":""},"path":[]}"#,
        r#"{"owner":{"kind":"primary"},"path":[],"path":["duplicate"]}"#,
        r#"{"owner":{"kind":"primary"}}"#,
        r#"{"owner":{"kind":"primary"},"path":null}"#,
    ] {
        assert!(serde_json::from_str::<Scope>(&format!("{{\"source\":{source}}}")).is_err());
    }
    let scope: Scope = serde_json::from_str(r#"{"source":[],"legacy_unknown":true}"#).unwrap();
    assert_eq!(scope.source().unwrap(), Some([].as_slice()));
}

#[test]
fn source_modes_and_output_path_keep_the_existing_exclusivity_contract() {
    let mut modes = vec![
        Scope {
            iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                from: None,
                to: 1,
                item: 2,
            }),
            ..Scope::default()
        },
        Scope {
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(Scope::default(), vec![])),
            ..Scope::default()
        },
    ];
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    modes.push(Scope {
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(1),
            plan,
        },
        ..Scope::default()
    });
    let selector = serde_json::to_value(primary()).unwrap()["source"].clone();
    for mode in modes {
        let mut value = serde_json::to_value(mode).unwrap();
        value["source"] = selector.clone();
        assert!(serde_json::from_value::<Scope>(value).is_err());
    }
    assert!(serde_json::from_str::<Scope>(r#"{"output_path":7}"#).is_err());
    assert!(serde_json::from_str::<Scope>(r#"{"segments":[]}"#).is_err());
    let scope = owned(SourceOwner::Primary, &[], Some(7));
    let decoded: Scope = serde_json::from_value(serde_json::to_value(&scope).unwrap()).unwrap();
    assert_eq!(decoded.output_path(), Some(7));
    assert_eq!(decoded.source_selector(), scope.source_selector());
}

#[test]
fn path_edits_and_output_toggles_preserve_exact_owner_and_selector() {
    let owner = SourceOwner::Named {
        name: " Ref 雪 ".into(),
    };
    let mut scope = owned(owner.clone(), &["Rows"], Some(7));
    scope.set_source(Some(vec![]));
    assert_eq!(
        scope.source_selector(),
        Some(SourceSelectorRef::Owned(&OwnedSourceRef {
            owner: owner.clone(),
            path: vec![]
        }))
    );
    assert_eq!(scope.output_path(), Some(7));
    assert_eq!(scope.source_mut().unwrap_err(), OwnedSourceAccess);
    assert!(scope.set_output_path(None));
    assert_eq!(scope.output_path(), None);
    assert!(scope.set_output_path(Some(11)));
    assert_eq!(scope.output_path(), Some(11));
    scope.set_sequence(None);
    assert!(matches!(
        scope.iteration,
        ScopeIteration::OwnedSource { .. }
    ));
    if let Some(SourceSelectorMut::Owned(reference)) = scope.source_selector_mut() {
        reference.path.push("Items".into());
    } else {
        panic!("owner-aware mutable view missing");
    }
    assert_eq!(
        scope.source_selector(),
        Some(SourceSelectorRef::Owned(&OwnedSourceRef {
            owner,
            path: vec!["Items".into()]
        }))
    );
    scope.set_source_selector(Some(SourceSelector::Legacy(vec!["Legacy".into()])));
    assert_eq!(scope.source().unwrap(), Some(["Legacy".into()].as_slice()));
    assert_eq!(scope.output_path(), Some(11));
    scope.set_source_selector(None);
    assert!(matches!(scope.iteration, ScopeIteration::None));
    assert_eq!(scope.output_path(), None);
}

#[test]
fn nested_dynamic_and_segment_scopes_keep_independent_selector_identities() {
    let child = owned(
        SourceOwner::Named {
            name: "Child".into(),
        },
        &[],
        None,
    );
    let dynamic = owned(SourceOwner::Primary, &["Rows"], None);
    let segment = owned(
        SourceOwner::Named {
            name: "Segment".into(),
        },
        &["Rows", "Rows"],
        None,
    );
    let mut root = Scope {
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(primary(), vec![segment])),
        ..Scope::default()
    };
    root.children.push(child);
    root.dynamic_children.push(DynamicChild {
        key: 1,
        scope: dynamic,
    });
    let text = serde_json::to_string(&root).unwrap();
    let decoded: Scope = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), text);
    let segments: Vec<_> = decoded.concatenated().unwrap().iter().collect();
    assert_ne!(segments[0].source_selector(), segments[1].source_selector());
    assert_ne!(
        decoded.children[0].source_selector(),
        decoded.dynamic_children[0].scope.source_selector()
    );
}

#[test]
fn owned_document_output_expression_remains_a_graph_root_after_pruning() {
    let mut project = project(owned(SourceOwner::Primary, &[], Some(7)));
    project.graph.nodes = BTreeMap::from([
        (
            7,
            Node::Call {
                function: "concat".into(),
                args: vec![8],
            },
        ),
        (
            8,
            Node::Const {
                value: Value::String("document".into()),
            },
        ),
        (
            99,
            Node::Const {
                value: Value::String("orphan".into()),
            },
        ),
    ]);
    let selector = serde_json::to_value(&project.root).unwrap()["source"].clone();
    project.prune_unreachable_nodes();
    assert_eq!(
        project.graph.nodes.keys().copied().collect::<Vec<_>>(),
        [7, 8]
    );
    assert_eq!(project.root.output_path(), Some(7));
    assert_eq!(
        serde_json::to_value(&project.root).unwrap()["source"],
        selector
    );
}
