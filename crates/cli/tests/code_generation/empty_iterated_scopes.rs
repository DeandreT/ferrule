use super::*;
use mapping::SequenceWindow;

#[test]
fn empty_iterated_scopes_compile_with_warnings_denied() -> TestResult<()> {
    for mode in ["source", "filtered", "windowed"] {
        let controlled = mode != "source";
        let root_array = mode == "windowed";
        let item = SchemaNode::group("Rows", vec![]).repeating();
        let empty = Scope {
            target_field: if root_array {
                String::new()
            } else {
                "Rows".into()
            },
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            filter: (mode == "filtered").then_some(0),
            windows: if root_array {
                vec![SequenceWindow::SkipFirst { count: 0 }]
            } else {
                vec![]
            },
            ..Default::default()
        };
        let project = Project {
            source: SchemaNode::group(
                "Source",
                vec![SchemaNode::group("Rows", vec![string("Label")]).repeating()],
            ),
            target: if root_array {
                item
            } else {
                SchemaNode::group("Report", vec![item])
            },
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: if controlled {
                    BTreeMap::from([(
                        0,
                        Node::Const {
                            value: if root_array {
                                Value::Int(1)
                            } else {
                                Value::Bool(true)
                            },
                        },
                    )])
                } else {
                    BTreeMap::new()
                },
            },
            root: if root_array {
                empty
            } else {
                Scope {
                    children: vec![empty],
                    ..Default::default()
                }
            },
        };
        assert!(engine::validate(&project).is_empty());
        let output = |count| {
            let values: Vec<_> = (0..count).map(|_| serde_json::json!({})).collect();
            if root_array {
                serde_json::json!(values)
            } else {
                serde_json::json!({"Rows":values})
            }
        };
        let cases = vec![
            serde_json::json!({"input":"{}","expected":output(0)}),
            serde_json::json!({"input":"{\"Rows\":[]}","expected":output(0)}),
            serde_json::json!({"input":"{\"Rows\":[{\"Label\":\"A\"},{\"Label\":\"雪\"},{}]}","expected":output(if root_array {2} else {3})}),
            serde_json::json!({"input":"{\"Rows\":[{}]}","expected":output(if root_array {0} else {1})}),
            serde_json::json!({"input":"{\"Rows\":[{\"Label\":false}]}","reject":true}),
            serde_json::json!({"input":"{\"Rows\":false}","reject":true}),
            serde_json::json!({"input":"{broken","reject":true}),
        ];
        json_text_boundaries::run_generated_boundary_cases(
            &project,
            &cases,
            &format!("empty_iterated_{mode}"),
        )?;
    }
    Ok(())
}
