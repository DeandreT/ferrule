use super::*;

fn encoded(text: &str, width: u8, be: bool) -> Vec<u8> {
    let mut bytes = match (width, be) {
        (2, false) => vec![0xff, 0xfe],
        (2, true) => vec![0xfe, 0xff],
        (4, false) => vec![0xff, 0xfe, 0, 0],
        (4, true) => vec![0, 0, 0xfe, 0xff],
        _ => unreachable!(),
    };
    if width == 2 {
        bytes.extend(
            text.encode_utf16()
                .flat_map(|x| if be { x.to_be_bytes() } else { x.to_le_bytes() }),
        );
    } else {
        bytes.extend(text.chars().flat_map(|x| {
            if be {
                (x as u32).to_be_bytes()
            } else {
                (x as u32).to_le_bytes()
            }
        }));
    }
    bytes
}

#[test]
fn complete_csv_documents_keep_decoding_explicit_in_both_backends() -> TestResult<()> {
    let project = Project {
        source: SchemaNode::group("Input", vec![string("Value")]),
        target: SchemaNode::group("Output", vec![string("Value")]),
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
            ..Default::default()
        },
        target_options: mapping::FormatOptions {
            tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                1,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]
            .into(),
        },
        root: Scope {
            iteration: ScopeIteration::Source(vec![]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        },
    };
    let program = codegen::lower(&project)?;
    let policy = codegen::CsvDocumentBoundaryPolicy::from_project(&project)?;
    let text = "Value\r\nSnow ☃🙂\r\n";
    let mut cases = vec![(text.as_bytes().to_vec(), "Value\nSnow ☃🙂\n", false)];
    for width in [2, 4] {
        for be in [false, true] {
            cases.push((encoded(text, width, be), "Value\nSnow ☃🙂\n", true));
        }
    }
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(text.as_bytes());
    cases.push((bytes, "Value\nSnow ☃🙂\n", false));
    let mut bytes = b"Value\nV".to_vec();
    bytes.extend_from_slice(&[0xe2, 0x82]);
    cases.push((bytes.clone(), "Value\nV\n", true));
    bytes.push(b'\n');
    cases.push((bytes, "Value\nV\u{fffd}\n", true));
    let mut bytes = encoded("Value\nV", 2, false);
    bytes.extend_from_slice(&[0, 0xd8]);
    cases.push((bytes.clone(), "Value\nV\n", true));
    bytes.extend_from_slice(&[b'\n', 0]);
    cases.push((bytes, "Value\nV\u{fffd}\n", true));
    let mut bytes = encoded("Value\nV", 4, true);
    bytes.extend_from_slice(&[0, 0x41]);
    cases.push((bytes, "Value\nV\n", true));
    let directory = TempDir::new("csv_byte_decoding")?;
    for language in ["rust", "csharp"] {
        let output = directory.0.join(language);
        let artifacts = if language == "rust" {
            codegen_rust::emit_with_csv_document_adapters(
                &program,
                &codegen_rust::Options {
                    package_name: "csv_decoding_fixture".into(),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../codegen-runtime")
                            .display()
                            .to_string(),
                    ),
                },
                &policy,
            )?
        } else {
            codegen_csharp::emit_with_csv_document_adapters(&program, &policy)?
        };
        for file in artifacts.files() {
            let path = output.join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, &file.contents)?;
        }
        for (i, (bytes, expected, strict_refuses)) in cases.iter().enumerate() {
            std::fs::write(output.join(format!("input{i}")), bytes)?;
            std::fs::write(output.join(format!("expected{i}")), expected)?;
            if *strict_refuses {
                std::fs::write(output.join(format!("strict-refuses{i}")), [])?;
            }
        }
        let mut command = if language == "rust" {
            std::fs::write(
                output.join("src/main.rs"),
                include_str!("fixtures/csv_byte_decoding_rust.rs.txt"),
            )?;
            let mut command = Command::new("cargo");
            command.args(["run", "--offline", "--quiet", "--"]);
            command
        } else {
            std::fs::create_dir(output.join("Harness"))?;
            std::fs::write(
                output.join("Harness/Program.cs"),
                include_str!("fixtures/csv_byte_decoding_csharp.cs.txt"),
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
                "--",
            ]);
            command
        };
        let result = host_policy::recorded_output(
            command.arg(cases.len().to_string()).current_dir(&output),
            &output,
            "csv-decoding",
        )?;
        assert!(
            result.status.success(),
            "{language}:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("CSV byte decoding cases passed"));
    }
    Ok(())
}
