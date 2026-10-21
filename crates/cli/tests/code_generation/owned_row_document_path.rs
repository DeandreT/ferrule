use super::*;
use ir::SourceDocumentOrigin;
use mapping::{NamedSource, OwnedRowDocumentPath, OwnedSourceRef, SourceOwner};

const EXPECTED: &str = r#"{"Primary":"p.root.csv","Named":"n.root.csv","Rows":[{"PrimaryRow":"p.1.csv","NamedRoot":"n.root.csv","Pairs":[{"PrimaryRow":"p.1.csv","NamedRow":"n.1.csv"},{"PrimaryRow":"p.1.csv","NamedRow":"n.2.csv"}]},{"PrimaryRow":"p.2.csv","NamedRoot":"n.root.csv","Pairs":[{"PrimaryRow":"p.2.csv","NamedRow":"n.1.csv"},{"PrimaryRow":"p.2.csv","NamedRow":"n.2.csv"}]}]}"#;

fn owned_path(owner: SourceOwner, collection: Option<Vec<String>>) -> Node {
    Node::OwnedRowDocumentPath(OwnedRowDocumentPath { owner, collection })
}
fn iteration(owner: SourceOwner) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: vec!["Rows".into()],
        },
        output_path: None,
    }
}
fn bind(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn project() -> Project {
    let endpoint =
        || SchemaNode::group("Input", vec![SchemaNode::group("Rows", vec![]).repeating()]);
    let s = |name| SchemaNode::scalar(name, ScalarType::String);
    Project {
        source: endpoint(),
        target: SchemaNode::group(
            "Output",
            vec![
                s("Primary"),
                s("Named"),
                SchemaNode::group(
                    "Rows",
                    vec![
                        s("PrimaryRow"),
                        s("NamedRoot"),
                        SchemaNode::group("Pairs", vec![s("PrimaryRow"), s("NamedRow")])
                            .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Ref".into(),
            path: "unused.json".into(),
            schema: endpoint(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (1, owned_path(SourceOwner::Primary, Some(vec![]))),
                (
                    2,
                    owned_path(SourceOwner::Named { name: "Ref".into() }, Some(vec![])),
                ),
                (3, owned_path(SourceOwner::Primary, None)),
                (
                    4,
                    owned_path(SourceOwner::Named { name: "Ref".into() }, None),
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![bind("Primary", 1), bind("Named", 2)],
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: iteration(SourceOwner::Primary),
                bindings: vec![bind("PrimaryRow", 3), bind("NamedRoot", 2)],
                children: vec![Scope {
                    target_field: "Pairs".into(),
                    iteration: iteration(SourceOwner::Named { name: "Ref".into() }),
                    bindings: vec![bind("PrimaryRow", 3), bind("NamedRow", 4)],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}
fn mark(instance: Instance, path: &str, enabled: bool) -> Instance {
    let Instance::Group(group) = instance else {
        panic!("group")
    };
    Instance::Group(if enabled {
        group.with_source_origin(
            SourceDocumentOrigin::new(path, None)
                .unwrap()
                .row(1)
                .unwrap(),
        )
    } else {
        group
    })
}
fn endpoint(prefix: &str, missing: Option<usize>) -> Instance {
    let rows = (1..=2)
        .map(|index| {
            mark(
                Instance::Group(vec![].into()),
                &format!("{prefix}.{index}.csv"),
                missing != Some(index),
            )
        })
        .collect();
    mark(
        Instance::Group(vec![("Rows".into(), Instance::Repeated(rows))].into()),
        &format!("{prefix}.root.csv"),
        missing != Some(0),
    )
}

#[test]
fn owned_row_paths_match_interpreter_and_generated_backends() -> TestResult<()> {
    let project = project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let wire = mapping::project_file::encode_pretty(&project)?;
    assert!(wire.contains("owned_row_document_path"));
    let decoded = mapping::project_file::decode_str(&wire)?;
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, wire);
    let result = engine::run_with_sources(
        &project,
        &endpoint("p", None),
        vec![("Ref".into(), endpoint("n", None))],
    )?;
    assert_eq!(
        format_json::to_value(&project.target, &result)?,
        serde_json::from_str::<serde_json::Value>(EXPECTED)?
    );
    for (primary_missing, named_missing, node, owner, collection) in [
        (Some(0), None, 1, SourceOwner::Primary, Some(vec![])),
        (Some(2), None, 3, SourceOwner::Primary, None),
        (
            None,
            Some(2),
            4,
            SourceOwner::Named { name: "Ref".into() },
            None,
        ),
    ] {
        let error = engine::run_with_sources(
            &project,
            &endpoint("p", primary_missing),
            vec![("Ref".into(), endpoint("n", named_missing))],
        )
        .unwrap_err();
        assert_eq!(
            error,
            engine::EngineError::OwnedSourceAccess {
                node: Some(node),
                scope: None,
                source: engine::OwnedSourceError::MissingRowDocumentPath { owner, collection }
            }
        );
    }
    let lowered = codegen::lower(&project)?;
    let directory = TempDir::new("owned_row_paths")?;
    for language in ["rust", "csharp"] {
        let output = directory.0.join(language);
        let artifacts = if language == "rust" {
            codegen_rust::emit(
                &lowered,
                &codegen_rust::Options {
                    package_name: "owned_row_paths_fixture".into(),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../codegen-runtime")
                            .display()
                            .to_string(),
                    ),
                },
            )?
        } else {
            codegen_csharp::emit(&lowered)?
        };
        for file in artifacts.files() {
            let path = output.join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, &file.contents)?;
        }
        std::fs::write(
            output.join("target.schema"),
            codegen_schema::encode(&project.target, 1024 * 1024)?,
        )?;
        std::fs::write(output.join("expected.json"), EXPECTED)?;
        let mut command = if language == "rust" {
            std::fs::write(
                output.join("src/main.rs"),
                include_str!("fixtures/owned_row_paths.rs.txt"),
            )?;
            let mut command = Command::new("cargo");
            command.args(["run", "--offline", "--quiet"]);
            command
        } else {
            std::fs::create_dir(output.join("Harness"))?;
            std::fs::write(
                output.join("Harness/Program.cs"),
                include_str!("fixtures/owned_row_paths.cs.txt"),
            )?;
            std::fs::write(
                output.join("Harness/Harness.csproj"),
                r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
            )?;
            let mut command = dotnet_command(&output);
            command.args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
                "-p:UseSharedCompilation=false",
            ]);
            command
        };
        let result =
            host_policy::recorded_output(command.current_dir(&output), &output, "owned-row-paths")?;
        assert!(
            result.status.success(),
            "{language}:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stdout)
                .contains("owned row document path cases passed")
        );
    }
    Ok(())
}
