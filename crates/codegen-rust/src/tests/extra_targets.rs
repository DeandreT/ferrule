use super::*;

#[test]
fn emits_ordered_named_outputs_with_unique_scope_names() {
    let mut program = program();
    let nested_schema = SchemaNode::group(
        "Audit",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Nested",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
        ],
    );
    program.extra_targets = vec![
        NamedTargetProgram {
            name: "first \"audit\"".into(),
            target: nested_schema,
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Name".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: vec![TargetScope {
                    target_field: "Nested".into(),
                    repeating: false,
                    iteration: None,
                    construction: TargetConstruction::Group,
                    bindings: vec![Binding {
                        target_field: "Name".into(),
                        expression: 1,
                        target_type: ScalarType::String,
                        repeating: false,
                    }],
                    children: Vec::new(),
                }],
            },
        },
        NamedTargetProgram {
            name: "second".into(),
            target: SchemaNode::group(
                "Summary",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Name".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: Vec::new(),
            },
        },
    ];

    let artifacts = emit(
        &program,
        &Options {
            package_name: "named-outputs".into(),
            runtime_dependency: RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .unwrap();

    assert!(source.contains("pub struct NamedOutput"));
    assert!(source.contains("pub struct ExecutionOutputs"));
    assert!(source.contains("Ok(execute_outputs(source)?.primary)"));
    assert!(source.contains("Ok(execute_outputs_with_context(source, execution)?.primary)"));
    assert!(source.contains("fn scope_extra_0(context: &ScopeContext<'_>)"));
    assert!(source.contains("fn scope_extra_0_0(context: &ScopeContext<'_>)"));
    assert!(source.contains("fn scope_extra_1(context: &ScopeContext<'_>)"));
    let first = source.find("name: \"first \\\"audit\\\"\"").unwrap();
    let second = source.find("name: \"second\"").unwrap();
    assert!(first < second);
    assert!(source.contains("instance: scope_extra_0(context)?"));
    assert!(source.contains("instance: scope_extra_1(context)?"));
}
