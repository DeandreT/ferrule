use super::*;
use ir::SourceDocumentOrigin;
use mapping::ScopeConstruction;

const EXPECTED: &str = r#"{"Groups":[{"Member":[{"Value":"A"},{"Value":"A"},{"Value":"A"},{"Value":"A"},{"Value":"A"}]},{"Member":[{"Value":"B"},{"Value":"B"}]}]}"#;

pub(super) fn project() -> Project {
    let row = |name| SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]);
    Project {
        source: SchemaNode::group("Input", vec![row("Rows").repeating()]),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Groups", vec![row("Member").repeating()]).repeating()],
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
            nodes: [(
                1,
                Node::SourceField {
                    frame: Some(vec!["Rows".into()]),
                    path: vec!["Value".into()],
                },
            )]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Groups".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                group_by: Some(1),
                children: vec![Scope {
                    target_field: "Member".into(),
                    iteration: ScopeIteration::Source(vec![]),
                    construction: ScopeConstruction::CopyCurrentSource,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

#[test]
fn actual_grouped_mappings_keep_physical_rows_and_write_only_data() -> TestResult<()> {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let documents = [
        SourceDocumentOrigin::new("same.csv", Some("resolved/same.csv"))?,
        SourceDocumentOrigin::new("same.csv", Some("resolved/same.csv"))?,
    ];
    let mut rows = Vec::new();
    for document in &documents {
        for (index, value) in ["A", "B", "A"].into_iter().enumerate() {
            let fields = vec![(
                "Value".into(),
                Instance::Scalar(Value::String(value.into())),
            )];
            rows.push(Instance::Group(
                ir::InstanceGroup::from(fields).with_source_origin(document.row(index + 1)?),
            ));
        }
    }
    rows.push(Instance::Group(
        vec![("Value".into(), Instance::Scalar(Value::String("A".into())))].into(),
    ));
    let input = Instance::Group(vec![("Rows".into(), Instance::Repeated(rows))].into());
    let output = engine::run(&project, &input)?;
    assert_eq!(
        format_json::to_value(&project.target, &output)?,
        serde_json::from_str::<serde_json::Value>(EXPECTED)?
    );
    let Instance::Repeated(groups) = output.field("Groups").unwrap() else {
        panic!("groups")
    };
    for (group, expected) in groups.iter().zip([
        vec![Some((0, 1)), Some((0, 3)), Some((1, 1)), Some((1, 3)), None],
        vec![Some((0, 2)), Some((1, 2))],
    ]) {
        let Instance::Group(wrapper) = group else {
            panic!("group")
        };
        assert!(wrapper.source_origin().is_none());
        let Instance::Repeated(members) = group.field("Member").unwrap() else {
            panic!("members")
        };
        assert_eq!(members.len(), expected.len());
        for (member, expected) in members.iter().zip(expected) {
            let Instance::Group(member) = member else {
                panic!("member")
            };
            match expected {
                Some((owner, position)) => {
                    let origin = member.source_origin().unwrap();
                    assert!(origin.document().same_document(&documents[owner]));
                    assert_eq!(origin.position(), position);
                }
                None => assert!(member.source_origin().is_none()),
            }
        }
    }
    let directory = TempDir::new("row_provenance_mapping")?;
    let lowered = codegen::lower(&project)?;
    for language in ["rust", "csharp"] {
        let output = directory.0.join(language);
        let artifacts = if language == "rust" {
            codegen_rust::emit(
                &lowered,
                &codegen_rust::Options {
                    package_name: "row_provenance_fixture".into(),
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
                include_str!("fixtures/row_provenance_mapping.rs.txt"),
            )?;
            let mut command = Command::new("cargo");
            command.args(["run", "--offline", "--quiet"]);
            command
        } else {
            std::fs::create_dir(output.join("Harness"))?;
            std::fs::write(
                output.join("Harness/Program.cs"),
                include_str!("fixtures/row_provenance_mapping.cs.txt"),
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
        let result = host_policy::recorded_output(
            command.current_dir(&output),
            &output,
            "row-provenance-mapping",
        )?;
        assert!(
            result.status.success(),
            "{language}:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("row provenance mapping cases passed")
        );
    }
    Ok(())
}
