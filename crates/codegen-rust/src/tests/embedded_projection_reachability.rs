use std::collections::BTreeMap;

use mapping::{
    Binding as MappingBinding, DelimitedDialect, DelimitedRecordField, FlexCommand, FlexLineEnding,
    FlexTextLayout, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, Project,
    Scope, UserFunction,
};

use super::*;

fn layout(fields: &[(&str, ScalarType)]) -> String {
    let layout = FlexTextLayout::new(
        "Root",
        FlexCommand::DelimitedRecords {
            name: "Row".into(),
            dialect: DelimitedDialect::new(',', "\n", '"', '"').unwrap(),
            fields: fields
                .iter()
                .map(|(name, ty)| DelimitedRecordField::new(*name, *ty).unwrap())
                .collect(),
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap();
    serde_json::to_string(&layout).unwrap()
}

fn nested_graph() -> Graph {
    Graph {
        nodes: BTreeMap::from([
            (
                1,
                Node::SourceField {
                    path: vec!["Raw".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String(layout(&[
                        ("Name", ScalarType::String),
                        ("Payload", ScalarType::String),
                    ])),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String(r#"["Row","Payload"]"#.into()),
                },
            ),
            (
                4,
                Node::Call {
                    function: "flextext_parse_field".into(),
                    args: vec![1, 2, 3],
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::String(layout(&[("Number", ScalarType::Int)])),
                },
            ),
            (
                6,
                Node::Const {
                    value: Value::String(r#"["Row","Number"]"#.into()),
                },
            ),
            (
                7,
                Node::Call {
                    function: "flextext_parse_field".into(),
                    args: vec![4, 5, 6],
                },
            ),
        ]),
    }
}

fn project() -> Project {
    let function = FunctionId::new(1);
    let parameter = FunctionParameterId::new(1);
    let mut body = nested_graph();
    body.nodes.insert(1, Node::FunctionParameter { parameter });
    let mut graph = nested_graph();
    graph.nodes.insert(
        8,
        Node::UserFunctionCall {
            function,
            args: vec![1],
        },
    );
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Raw", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Out", ScalarType::Int),
                SchemaNode::scalar("ViaFunction", ScalarType::Int),
                SchemaNode::scalar("Path", ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::from([(
            function,
            UserFunction {
                library: "tests".into(),
                name: "parse_nested_record".into(),
                description: None,
                parameters: vec![FunctionParameter {
                    id: parameter,
                    name: "raw".into(),
                    ty: ScalarType::String,
                }],
                output_name: "number".into(),
                output_type: ScalarType::Int,
                body,
                output: 7,
            },
        )]),
        graph,
        root: Scope {
            bindings: vec![
                MappingBinding {
                    target_field: "Out".into(),
                    node: 7,
                },
                MappingBinding {
                    target_field: "ViaFunction".into(),
                    node: 8,
                },
                MappingBinding {
                    target_field: "Path".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_nested_projections_and_udfs_compile_without_dead_metadata() {
    let project = project();
    let program = codegen::lower(&project).unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let directory = TempDir::new("nested_projection_reachability");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "nested-projection-reachability".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    write_artifacts(directory.path(), &artifacts);
    fs::write(directory.path().join("src/main.rs"), r##"use codegen_runtime::{Value, field, group, scalar};
fn main() {
    for (raw, number) in [("label,7", 7), ("other,-12", -12)] {
        let source = group([field("Raw", scalar(Value::String(raw.into())))]);
        assert_eq!(nested_projection_reachability::execute(&source).unwrap(), group([
            field("Out", scalar(Value::Int(number))),
            field("ViaFunction", scalar(Value::Int(number))),
            field("Path", scalar(Value::String(r#"["Row","Payload"]"#.into()))),
        ]));
    }
    let json = r#"{"Raw":"label,7"}"#;
    let expected = r#"{
  "Out": 7,
  "ViaFunction": 7,
  "Path": "[\"Row\",\"Payload\"]"
}
"#;
    assert_eq!(nested_projection_reachability::execute_json(json).unwrap(), expected);
    assert_eq!(nested_projection_reachability::execute_json_bytes(json.as_bytes()).unwrap(), expected.as_bytes());
}
"##).unwrap();
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(directory.path())
        .env("RUSTFLAGS", "-D warnings")
        .generated_host_output(directory.path())
        .unwrap();
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
