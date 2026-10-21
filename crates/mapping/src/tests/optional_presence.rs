use super::*;

fn presence_scope() -> Scope {
    Scope {
        iteration: ScopeIteration::OptionalPresence { condition: 7 },
        bindings: vec![Binding {
            target_field: "Name".into(),
            node: 3,
        }],
        ..Scope::default()
    }
}

#[test]
fn optional_presence_round_trips_under_the_strict_sequence_key() {
    let value = serde_json::to_value(presence_scope()).unwrap();
    assert_eq!(
        value["sequence"],
        serde_json::json!({"kind": "optional_presence", "condition": 7})
    );
    assert_eq!(value["source"], serde_json::Value::Null);
    let decoded: Scope = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.presence(), Some(7));
    assert!(decoded.iterates());
    assert_eq!(decoded.sequence(), None);
    assert_eq!(decoded.source(), Ok(None));
}

/// Readers that predate optional presence decode `sequence` as a generated
/// sequence and therefore refuse the scope instead of emitting one group.
#[test]
fn readers_without_optional_presence_refuse_it() {
    let value = serde_json::to_value(presence_scope()).unwrap();
    let error = serde_json::from_value::<SequenceExpr>(value["sequence"].clone()).unwrap_err();
    assert!(error.to_string().contains("optional_presence"), "{error}");
}

#[test]
fn optional_presence_wire_is_strict_and_exclusive() {
    let value = serde_json::to_value(presence_scope()).unwrap();
    for (key, conflict) in [
        ("source", serde_json::json!(["Rows"])),
        ("segments", serde_json::json!([{"target_field": ""}])),
    ] {
        let mut conflicting = value.as_object().cloned().unwrap();
        conflicting.insert(key.into(), conflict);
        let error = serde_json::from_value::<Scope>(serde_json::Value::Object(conflicting))
            .unwrap_err()
            .to_string();
        assert!(error.contains("mutually exclusive"), "{key}: {error}");
    }
    for sequence in [
        serde_json::json!({"kind": "optional_presence"}),
        serde_json::json!({"kind": "optional_presence", "condition": 7, "item": 1}),
        serde_json::json!({"kind": "optional_presence", "condition": "7"}),
    ] {
        let mut malformed = value.as_object().cloned().unwrap();
        malformed.insert("sequence".into(), sequence.clone());
        assert!(
            serde_json::from_value::<Scope>(serde_json::Value::Object(malformed)).is_err(),
            "{sequence}"
        );
    }
}

#[test]
fn optional_presence_conditions_are_reachable_roots() {
    let mut project = Project {
        source: ir::SchemaNode::group("Source", Vec::new()),
        target: ir::SchemaNode::group("Target", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: [3, 7, 8]
                .into_iter()
                .map(|node| {
                    (
                        node,
                        Node::Const {
                            value: ir::Value::Bool(true),
                        },
                    )
                })
                .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Item".into(),
                ..presence_scope()
            }],
            ..Scope::default()
        },
        user_functions: Default::default(),
    };
    project.prune_unreachable_nodes();
    assert_eq!(
        project.graph.nodes.keys().copied().collect::<Vec<_>>(),
        [3, 7]
    );
}
