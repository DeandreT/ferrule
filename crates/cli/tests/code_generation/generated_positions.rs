use super::*;
use mapping::{SequenceExpr, SequenceWindow};

fn field(path: &str) -> Node {
    Node::SourceField {
        path: vec![path.into()],
        frame: None,
    }
}
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
fn constant(value: i64) -> Node {
    Node::Const {
        value: Value::Int(value),
    }
}
fn binding(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn generated(item: u32, from: u32, to: u32) -> ScopeIteration {
    ScopeIteration::Sequence(SequenceExpr::Generate {
        item,
        from: Some(from),
        to,
    })
}
fn project() -> Project {
    let nested =
        SchemaNode::group("Nested", vec![int("Outer"), int("Inner"), int("Position")]).repeating();
    let rows = SchemaNode::group(
        "Rows",
        vec![
            int("Value"),
            int("Original"),
            int("Position"),
            bool_("Exists"),
            int("Sum"),
            nested,
            SchemaNode::group("Config", vec![int("Outer")]).repeating(),
        ],
    )
    .repeating();
    let windowed =
        SchemaNode::group("Windowed", vec![int("Original"), int("Position")]).repeating();
    let function = FunctionId::new(1);
    let mut project = Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Parents",
                    vec![int("From"), int("To"), SchemaNode::group("Config", vec![])],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Parents", vec![rows, windowed]).repeating()],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    project.graph.nodes.extend([
        (1, field("From")),
        (2, field("To")),
        (3, constant(1)),
        (4, constant(2)),
        (100, item()),
        (110, Node::GeneratedPosition { item: 100 }),
        (111, Node::Position { collection: vec![] }),
        (
            112,
            Node::Call {
                function: "greater_than".into(),
                args: vec![100, 1],
            },
        ),
        (
            113,
            Node::UserFunctionCall {
                function,
                args: vec![100, 110],
            },
        ),
        (200, item()),
        (210, Node::GeneratedPosition { item: 200 }),
        (300, item()),
        (310, Node::GeneratedPosition { item: 300 }),
        (400, item()),
        (411, Node::GeneratedPosition { item: 400 }),
        (
            412,
            Node::Call {
                function: "greater_than".into(),
                args: vec![411, 3],
            },
        ),
        (
            410,
            Node::SequenceExists {
                sequence: SequenceExpr::Generate {
                    item: 400,
                    from: Some(3),
                    to: 4,
                },
                predicate: 412,
            },
        ),
        (500, item()),
        (501, Node::GeneratedPosition { item: 500 }),
        (
            502,
            Node::Call {
                function: "add".into(),
                args: vec![110, 501],
            },
        ),
        (
            510,
            Node::SequenceAggregate {
                function: mapping::AggregateOp::Sum,
                sequence: SequenceExpr::Generate {
                    item: 500,
                    from: Some(3),
                    to: 4,
                },
                predicate: None,
                expression: Some(502),
                arg: None,
            },
        ),
        (
            312,
            Node::Call {
                function: "greater_than".into(),
                args: vec![300, 1],
            },
        ),
    ]);
    let left = mapping::FunctionParameterId::new(1);
    let right = mapping::FunctionParameterId::new(2);
    project.user_functions.insert(
        function,
        UserFunction {
            library: "positions".into(),
            name: "Encode".into(),
            description: None,
            parameters: vec![
                mapping::FunctionParameter {
                    id: left,
                    name: "item".into(),
                    ty: ScalarType::Int,
                },
                mapping::FunctionParameter {
                    id: right,
                    name: "original".into(),
                    ty: ScalarType::Int,
                },
            ],
            output_name: "Result".into(),
            output_type: ScalarType::Int,
            body: Graph {
                nodes: [
                    (1, Node::FunctionParameter { parameter: left }),
                    (2, Node::FunctionParameter { parameter: right }),
                    (3, constant(10)),
                    (
                        4,
                        Node::Call {
                            function: "multiply".into(),
                            args: vec![1, 3],
                        },
                    ),
                    (
                        5,
                        Node::Call {
                            function: "add".into(),
                            args: vec![4, 2],
                        },
                    ),
                ]
                .into(),
            },
            output: 5,
        },
    );
    project.root.children.push(Scope {
        target_field: "Parents".into(),
        iteration: ScopeIteration::Source(vec!["Parents".into()]),
        children: vec![
            Scope {
                target_field: "Rows".into(),
                iteration: generated(100, 1, 2),
                filter: Some(112),
                sort_by: Some(100),
                sort_descending: true,
                bindings: vec![
                    binding("Value", 113),
                    binding("Original", 110),
                    binding("Position", 111),
                    binding("Exists", 410),
                    binding("Sum", 510),
                ],
                children: vec![
                    Scope {
                        target_field: "Nested".into(),
                        iteration: generated(200, 3, 4),
                        bindings: vec![
                            binding("Outer", 110),
                            binding("Inner", 210),
                            binding("Position", 111),
                        ],
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "Config".into(),
                        iteration: ScopeIteration::Source(vec!["Config".into()]),
                        group_by: Some(3),
                        bindings: vec![binding("Outer", 110)],
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            },
            Scope {
                target_field: "Windowed".into(),
                iteration: generated(300, 1, 2),
                filter: Some(312),
                sort_by: Some(300),
                sort_descending: true,
                windows: vec![SequenceWindow::SkipFirst { count: 3 }],
                bindings: vec![binding("Original", 310), binding("Position", 111)],
                ..Scope::default()
            },
        ],
        ..Scope::default()
    });
    project
}

const INPUT: &str = r#"{"Parents":[{"From":3,"To":5,"Config":{}},{"From":8,"To":10,"Config":{}},{"From":5,"To":3,"Config":{}}]}"#;
fn expected_json() -> String {
    let row = |value, original, position, sum| {
        serde_json::json!({
            "Value": value, "Original": original, "Position": position, "Exists": true, "Sum": sum,
            "Nested": [
                {"Outer": original, "Inner": 1, "Position": 1},
                {"Outer": original, "Inner": 2, "Position": 2},
            ],
            "Config": [{"Outer": original}],
        })
    };
    serde_json::json!({"Parents": [
        {"Rows": [row(53,3,1,9), row(42,2,2,7)], "Windowed": [{"Original":2,"Position":1}]},
        {"Rows": [row(103,3,1,9), row(92,2,2,7)], "Windowed": [{"Original":2,"Position":1}]},
        {"Rows":[],"Windowed":[]},
    ]})
    .to_string()
}

#[test]
fn generated_positions_survive_sort_filter_windows_nested_owners_and_repeated_parents()
-> TestResult<()> {
    let project = project();
    let input = format_json::from_str(INPUT, &project.source)?;
    let expected_json = expected_json();
    let expected = format_json::from_str(&expected_json, &project.target)?;
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    assert_eq!(engine::run(&project, &input)?, expected);
    let directory = TempDir::new("generated_positions")?;
    let path = directory.0.join("project.json");
    std::fs::write(&path, serde_json::to_vec(&project)?)?;
    std::fs::write(directory.0.join("input.json"), INPUT)?;
    std::fs::write(directory.0.join("expected.json"), expected_json)?;
    let rust_output = directory.0.join("rust");
    generate_project(
        &path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        r#"
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    for _ in 0..2 {
        let actual = ferrule_generated_mapping::execute_json(include_str!("../../input.json"))?;
        assert_eq!(compact(&actual), compact(include_str!("../../expected.json")));
    }
    Ok(())
}
"#,
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .env("CARGO_BUILD_BUILD_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let csharp_output = directory.0.join("csharp");
    generate_project(&path, &csharp_output, GenerateTarget::CSharp)?;
    std::fs::create_dir(csharp_output.join("Harness"))?;
    std::fs::write(
        csharp_output.join("Harness/Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        csharp_output.join("Harness/Program.cs"),
        r#"
using System.Text.Json;
using Ferrule.Generated;
var input = File.ReadAllText("../input.json");
var expected = JsonDocument.Parse(File.ReadAllText("../expected.json")).RootElement;
for (var i=0; i<2; i++) {
    var actual = JsonDocument.Parse(GeneratedMapping.ExecuteJson(input)).RootElement;
    if (!JsonElement.DeepEquals(expected, actual)) throw new Exception(actual.ToString());
}
"#,
    )?;
    let csharp = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}

#[test]
fn generated_positions_refuse_missing_sibling_parent_bound_and_group_origins() {
    for kind in ["missing", "sibling", "parent-bound", "group"] {
        let mut project = project();
        match kind {
            "missing" => {
                project
                    .graph
                    .nodes
                    .insert(110, Node::GeneratedPosition { item: 999 });
            }
            "sibling" => {
                project
                    .graph
                    .nodes
                    .insert(110, Node::GeneratedPosition { item: 300 });
            }
            "parent-bound" => {
                project.root.children[0].children[0].windows =
                    vec![SequenceWindow::First { count: 110 }];
            }
            "group" => {
                project.root.children[0].children[0].group_into_blocks = Some(4);
            }
            _ => unreachable!(),
        }
        assert!(!engine::validate(&project).is_empty(), "{kind}");
        assert!(codegen::lower(&project).is_err(), "{kind}");
    }
    let project = project();
    let mut program = codegen::lower(&project).unwrap();
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == 110)
        .unwrap()
        .expression = codegen::Expression::GeneratedPosition { item: 300 };
    assert!(matches!(
        codegen::validate_program(&program),
        Err(codegen::ProgramValidationError::GeneratedPositionOutOfContext { .. })
    ));
}
