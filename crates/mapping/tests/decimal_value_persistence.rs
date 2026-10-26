use ir::{Decimal96, ScalarType, SchemaNode, Value};
use mapping::{Graph, Node, Project, Scope, project_file};

fn project() -> Project {
    let value = Value::decimal(Decimal96::parse_exact("9007199254740993.123456789012").unwrap());
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::Const {
                        value: value.clone(),
                    },
                ),
                (
                    2,
                    Node::ValueMap {
                        input: 1,
                        input_type: None,
                        table: vec![
                            (value.clone(), Value::Float(0.1)),
                            (Value::Int(1), value.clone()),
                        ],
                        default: Some(value),
                    },
                ),
            ]
            .into(),
        },
        root: Scope::default(),
    }
}

#[test]
fn decimal_project_literals_round_trip_without_binary64() {
    let original = project();
    let wire = project_file::encode_pretty(&original).unwrap();
    assert!(wire.contains(r#""$decimal96": "9007199254740993.123456789012""#));
    let restored = project_file::decode_str(&wire).unwrap();
    assert_eq!(
        serde_json::to_string(&restored).unwrap(),
        serde_json::to_string(&original).unwrap()
    );
    assert_eq!(project_file::encode_pretty(&restored).unwrap(), wire);
}

#[test]
fn decimal_project_metadata_is_strict() {
    let wire = project_file::encode_pretty(&project()).unwrap();
    for invalid in ["NaN", "1e0", "1.0", "79228162514264337593543950336"] {
        assert!(
            project_file::decode_str(&wire.replace("9007199254740993.123456789012", invalid))
                .is_err()
        );
    }
    let duplicate = wire.replace(
        r#""$decimal96": "9007199254740993.123456789012""#,
        r#""$decimal96": "1", "$decimal96": "2""#,
    );
    assert!(project_file::decode_str(&duplicate).is_err());
}
