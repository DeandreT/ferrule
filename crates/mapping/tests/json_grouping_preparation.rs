use mapping::{DocumentProject, JsonGroupingPreparation, JsonSourceShape, PrimarySourcePlan};

fn shape() -> JsonSourceShape {
    serde_json::from_str(r#"{"kind":"object","properties":[{"name":"rows","shape":{"kind":"array","item":{"kind":"object","properties":[{"name":"key","shape":{"kind":"string"}},{"name":"__group_key","shape":{"kind":"string"}},{"name":"__group_member","shape":{"kind":"string"}}]}}}]}"#).unwrap()
}
fn plan() -> JsonGroupingPreparation {
    JsonGroupingPreparation {
        collection: vec!["Root".into(), "rows".into(), "Items".into()],
        keys: vec!["key".into()],
    }
}

#[test]
fn preparation_keeps_raw_shape_separate_and_preserves_reserved_member_fields() {
    let shape = shape();
    let plan = plan();
    let schema = plan.document_set_schema(&shape).unwrap();
    let candidate = schema
        .child("Documents")
        .unwrap()
        .child("Root")
        .unwrap()
        .child("rows")
        .unwrap()
        .child("Items")
        .unwrap();
    let member = candidate.child(JsonGroupingPreparation::MEMBER).unwrap();
    assert!(!member.repeating);
    assert!(member.child(JsonGroupingPreparation::KEY).is_some());
    assert!(member.child(JsonGroupingPreparation::MEMBER).is_some());
    let mut project = mapping::Project {
        source: schema,
        target: ir::SchemaNode::group("out", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Default::default(),
    };
    project.graph.nodes.insert(
        1,
        mapping::Node::Const {
            value: ir::Value::String("input.json".into()),
        },
    );
    let document = DocumentProject::new(project, PrimarySourcePlan { filename: 1 })
        .with_json_grouping_preparation(shape.clone(), plan.clone())
        .unwrap();
    let encoded = serde_json::to_value(&document).unwrap();
    let restored: DocumentProject = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored.json_source_projection(), Some(&shape));
    assert_eq!(restored.json_grouping_preparation(), Some(&plan));
    assert!(restored.with_json_source_projection(shape).is_err());
}

#[test]
fn invalid_preparation_paths_and_non_string_keys_are_not_admitted() {
    let raw = shape();
    for candidate in [
        JsonGroupingPreparation {
            collection: vec![],
            ..plan()
        },
        JsonGroupingPreparation {
            collection: vec!["Documents".into(), "Root".into()],
            ..plan()
        },
        JsonGroupingPreparation {
            collection: vec!["Root".into()],
            ..plan()
        },
        JsonGroupingPreparation {
            keys: vec!["missing".into()],
            ..plan()
        },
        JsonGroupingPreparation {
            keys: vec!["key".into(), "Value".into()],
            ..plan()
        },
        JsonGroupingPreparation {
            keys: vec!["x".repeat(65537)],
            ..plan()
        },
    ] {
        assert!(candidate.validate(&raw).is_err());
    }
    let numeric: JsonSourceShape = serde_json::from_str(
        &serde_json::to_string(&raw)
            .unwrap()
            .replace("\"string\"", "\"number\""),
    )
    .unwrap();
    assert!(plan().validate(&numeric).is_err());
}
