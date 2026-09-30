use super::*;

fn text_boundary_project() -> Project {
    let arbitrary = |name: &str| {
        string(name)
            .json_any()
            .expect("string encodes arbitrary JSON")
    };
    Project {
        source: SchemaNode::group("Source", vec![string("Text"), arbitrary("Any")]),
        target: SchemaNode::group(
            "Target",
            vec![
                string("Text"),
                arbitrary("Any"),
                arbitrary("Parsed"),
                int("Length"),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Any".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "length".into(),
                        args: vec![1],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Text".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Any".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Parsed".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Length".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    }
}

#[test]
fn generated_json_text_boundaries_match_interpreter_in_rust_and_csharp() -> TestResult<()> {
    let project = text_boundary_project();
    assert!(engine::validate(&project).is_empty());
    let valid_inputs = [
        serde_json::json!({"Text": "😀e\u{301}", "Any": {"valid": "😀", "items": [1, true, null]}})
            .to_string(),
        // These are valid graph strings containing invalid encoded JSON. The
        // arbitrary-JSON writer must retain the original string as its fallback.
        serde_json::json!({"Text": r#""\uD800""#, "Any": "valid"}).to_string(),
        serde_json::json!({"Text": r#"{"value":1e400}"#, "Any": 7}).to_string(),
        serde_json::json!({"Text": r#"{"value":"\uDFFF"}"#, "Any": []}).to_string(),
        r#"{"Text":"first","Text":"last","Any":0}"#.into(),
    ];
    let mut cases = Vec::new();
    for input in valid_inputs {
        let source = format_json::from_str(&input, &project.source)?;
        let result = engine::run(&project, &source)?;
        let expected: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &result)?)?;
        cases.push(serde_json::json!({"input": input, "expected": expected}));
    }
    for input in [
        r#"{"Text":"\uD800","Text":"valid","Any":0}"#,
        r#"{"Text":1e400,"Text":"valid","Any":0}"#,
        r#"{"Text":"ok","Any":{"value":"\uDFFF"}}"#,
        r#"{"Text":"ok","Any":1e400}"#,
        r#"{"Text":"ok","Any":-1e400}"#,
        r#"{"Text":"ok","Any":{"value":1e400,"value":1}}"#,
        r#"{"Text":"ok","Any":{"\uD800":1}}"#,
        r#"{"Text":"ok","Any":0,"Unexpected":true}"#,
    ] {
        assert!(
            format_json::from_str(input, &project.source).is_err(),
            "native boundary accepted invalid input: {input}"
        );
        cases.push(serde_json::json!({"input": input, "reject": true}));
    }
    for bytes in [
        b"{\"Text\":\"\xFF\",\"Any\":0}".as_slice(),
        b"{\"Text\":\"\xED\xA0\x80\",\"Any\":0}".as_slice(),
        b"{\"Text\":\"\xF0\x9F\",\"Any\":0}".as_slice(),
    ] {
        cases.push(serde_json::json!({"bytes": bytes, "reject": true}));
    }

    let directory = TempDir::new("json_text_boundaries")?;
    let project_path = directory.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let fixtures = serde_json::to_vec(&cases)?;
    let rust_output = directory.0.join("rust");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    let manifest_path = rust_output.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path)?;
    std::fs::write(
        manifest_path,
        manifest.replace(
            "\n[workspace]",
            "\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n\n[workspace]",
        ),
    )?;
    std::fs::write(rust_output.join("cases.json"), &fixtures)?;
    std::fs::write(
        rust_output.join("src/main.rs"),
        include_str!("fixtures/json_text_boundaries_rust.rs.txt"),
    )?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .env("RUSTFLAGS", "-Dwarnings")
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust text boundaries failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    std::fs::write(csharp_output.join("cases.json"), fixtures)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/json_text_boundaries_csharp.cs.txt"),
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
        "generated C# text boundaries failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
