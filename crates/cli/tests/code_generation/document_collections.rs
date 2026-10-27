use super::*;

const MODEL: &str =
    include_str!("../../../codegen/src/tests/fixtures/nested_document_collection.json");
const CASES: &str = include_str!("document_collections/cases.json");

fn expected(case: &serde_json::Value) -> Instance {
    Instance::DocumentSet(
        case["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|document| {
                ir::DocumentMember::new(
                    document["path"].as_str().unwrap(),
                    Instance::Group(
                        vec![(
                            "Person".into(),
                            Instance::Repeated(
                                document["names"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|name| {
                                        Instance::Group(
                                            vec![(
                                                "Name".into(),
                                                Instance::Scalar(Value::String(
                                                    name.as_str().unwrap().into(),
                                                )),
                                            )]
                                            .into(),
                                        )
                                    })
                                    .collect(),
                            ),
                        )]
                        .into(),
                    ),
                )
                .unwrap()
            })
            .collect(),
    )
}

#[test]
fn nested_document_collections_match_native_and_generated_public_calls() -> TestResult<()> {
    let project: Project = serde_json::from_str(MODEL)?;
    let cases: serde_json::Value = serde_json::from_str(CASES)?;
    exercise(&project, &cases)?;
    let mut nested = project.clone();
    nested.root = Scope {
        construction: mapping::ScopeConstruction::CollectDocuments,
        children: vec![project.root.clone(), project.root.clone()],
        ..Scope::default()
    };
    let mut repeated_cases = cases;
    for case in repeated_cases.as_array_mut().unwrap() {
        if let Some(documents) = case
            .get_mut("documents")
            .and_then(serde_json::Value::as_array_mut)
        {
            documents.extend(documents.clone());
        }
    }
    exercise(&nested, &repeated_cases)
}

fn exercise(project: &Project, cases: &serde_json::Value) -> TestResult<()> {
    assert!(engine::validate(project).is_empty());
    for case in cases.as_array().unwrap() {
        let source = format_xml::from_str(case["input"].as_str().unwrap(), &project.source)?;
        let result = engine::run(project, &source);
        if case.get("error").is_some() {
            assert_eq!(
                result,
                Err(engine::EngineError::MappingException {
                    node: 12,
                    message: Some("late document failure".into()),
                })
            );
        } else {
            assert_eq!(result?, expected(case));
        }
    }
    let directory = TempDir::new("document_collections")?;
    let root = &directory.0;
    let path = root.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(project)?)?;
    std::fs::write(root.join("cases.json"), serde_json::to_vec(cases)?)?;
    std::fs::write(
        root.join("source-schema.json"),
        serde_json::to_vec(&project.source)?,
    )?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let generated = root.join("rust");
    generate_project(
        &path,
        &generated,
        GenerateTarget::Rust {
            runtime_path: runtime.clone(),
        },
    )?;
    let host = root.join("rust-host");
    std::fs::create_dir_all(host.join("src"))?;
    std::fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname=\"document-collections-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={}}}\ncodegen-runtime={{path={}}}\nserde_json=\"1\"\nroxmltree=\"0.21\"\n",
            serde_json::to_string(&generated.to_string_lossy())?,
            serde_json::to_string(&runtime.to_string_lossy())?
        ),
    )?;
    std::fs::write(
        host.join("src/main.rs"),
        include_str!("document_collections/Host.rs.txt"),
    )?;
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("cargo-target"));
    let rust = Command::new("cargo")
        .args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
        .arg(root)
        .current_dir(&host)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    std::fs::write(root.join("rust-stdout.txt"), &rust.stdout)?;
    std::fs::write(root.join("rust-stderr.txt"), &rust.stderr)?;
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );

    let generated = root.join("csharp");
    generate_project(&path, &generated, GenerateTarget::CSharp)?;
    let host = root.join("csharp-host");
    std::fs::create_dir_all(&host)?;
    std::fs::write(
        host.join("Host.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><EnableTargetingPackDownload>false</EnableTargetingPackDownload><EnableRuntimePackDownload>false</EnableRuntimePackDownload></PropertyGroup><ItemGroup><ProjectReference Include="../csharp/Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(
        host.join("Program.cs"),
        include_str!("document_collections/Host.cs.txt"),
    )?;
    let csharp = dotnet_command(root)
        .args([
            "run",
            "--project",
            "csharp-host/Host.csproj",
            "--configuration",
            "Release",
            "-p:EnableTargetingPackDownload=false",
            "-p:EnableRuntimePackDownload=false",
            "--",
        ])
        .arg(root)
        .current_dir(root)
        .isolated_output()?;
    std::fs::write(root.join("csharp-stdout.txt"), &csharp.stdout)?;
    std::fs::write(root.join("csharp-stderr.txt"), &csharp.stderr)?;
    assert!(
        csharp.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
