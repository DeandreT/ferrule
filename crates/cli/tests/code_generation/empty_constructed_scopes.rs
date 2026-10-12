use super::*;

#[test]
fn empty_constructed_scopes_compile_with_warnings_denied() -> TestResult<()> {
    for (target, root, expected, label) in [
        (
            SchemaNode::group("Empty", vec![]),
            Scope::default(),
            serde_json::json!({}),
            "empty_root",
        ),
        (
            SchemaNode::group(
                "Report",
                vec![
                    SchemaNode::group("Single", vec![]),
                    SchemaNode::group("Many", vec![]).repeating(),
                ],
            ),
            Scope {
                children: vec![
                    Scope {
                        target_field: "Single".into(),
                        ..Default::default()
                    },
                    Scope {
                        target_field: "Many".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            serde_json::json!({"Single":{},"Many":[{}]}),
            "empty_child_scopes",
        ),
    ] {
        let project = Project {
            source: SchemaNode::group("Source", vec![string("Text")]),
            target,
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph::default(),
            root,
        };
        assert!(engine::validate(&project).is_empty());
        let cases = vec![
            serde_json::json!({"input":"{}","expected":expected}),
            serde_json::json!({"input":"{\"Text\":\"雪\"}","expected":expected}),
            serde_json::json!({"input":"{\"Text\":false}","reject":true}),
            serde_json::json!({"input":"{broken","reject":true}),
        ];
        json_text_boundaries::run_generated_boundary_cases(&project, &cases, label)?;
    }
    Ok(())
}
