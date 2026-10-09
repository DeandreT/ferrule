use std::path::Path;

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Project, Scope, ScopeIteration};

use crate::{MfdError, preflight_export};

fn project() -> Project {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "identity-all")
        .unwrap();
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: serde_json::from_value(case["user_functions"].clone()).unwrap(),
        graph: serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(
                    serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
                ),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 11,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn filter_map_preflight_refuses_before_artifact_preparation() {
    let project = project();
    let actual = preflight_export(&project, Path::new("unpublished.mfd"));
    eprintln!(
        "input={}\nactual={actual:#?}",
        serde_json::to_string(&project).unwrap()
    );
    assert!(matches!(
        actual,
        Err(MfdError::UnsupportedSequenceComposition { item: 11 })
    ));
}
