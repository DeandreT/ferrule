use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

const CASES: [(&str, &str); 4] = [
    (
        "ordered",
        include_str!("fixtures/xml_dynamic_outputs/ordered.xml"),
    ),
    (
        "empty",
        include_str!("fixtures/xml_dynamic_outputs/empty.xml"),
    ),
    (
        "late-output",
        include_str!("fixtures/xml_dynamic_outputs/late-output.xml"),
    ),
    (
        "mapping-first",
        include_str!("fixtures/xml_dynamic_outputs/mapping-first.xml"),
    ),
];

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("xml_dynamic_outputs_{language}"))?;
        let path = directory.0.clone();
        std::mem::forget(directory);
        Ok(Self {
            path,
            complete: false,
        })
    }
}
impl Drop for RegressionDirectory {
    fn drop(&mut self) {
        if self.complete {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained dynamic XML output test artifacts: {}",
                self.path.display()
            );
        }
    }
}
fn recorded(command: &mut Command, directory: &Path, name: &str) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = command.isolated_output()?;
    std::fs::write(directory.join(format!("{name}-stdout.txt")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.txt")), &output.stderr)?;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec(
            &json!({"success":output.status.success(),"code":output.status.code()}),
        )?,
    )?;
    Ok(output)
}
fn project() -> TestResult<Project> {
    let mut project: Project = serde_json::from_str(include_str!(
        "../../../codegen/src/tests/fixtures/dynamic_primary_xml_documents.json"
    ))?;
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("bad".into()),
        },
    );
    project.graph.nodes.insert(
        3,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 2],
        },
    );
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("\u{1}".into()),
        },
    );
    project.graph.nodes.insert(
        5,
        Node::If {
            condition: 3,
            then: 4,
            else_: 1,
        },
    );
    project.root.bindings[0].node = 5;
    Ok(project)
}
fn typed_members(value: &Instance) -> Json {
    let Instance::DocumentSet(members) = value else {
        panic!("complete primary document set")
    };
    json!(
        members
            .iter()
            .map(
                |member| json!({"path":member.path(),"instance":snapshot::snapshot(member.value())})
            )
            .collect::<Vec<_>>()
    )
}
fn expected_physical(value: &str) -> Json {
    json!({"name":"Result","namespace":"","text":null,"attributes":[],"children":[
        {"name":"Value","namespace":"","text":value,"attributes":[],"children":[]}
    ]})
}
fn exercise(language: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language)?;
    let project = project()?;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    assert_eq!(
        lowered.xml_output_mode()?,
        Some(codegen::XmlOutputMode::DynamicPrimaryDocuments)
    );
    assert!(lowered.extra_sources.is_empty() && lowered.extra_targets.is_empty());
    for (name, document) in CASES {
        std::fs::write(directory.path.join(format!("{name}.xml")), document)?;
    }
    for (name, schema) in [("source", &project.source), ("target", &project.target)] {
        std::fs::write(
            directory.path.join(format!("{name}-schema.txt")),
            codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?,
        )?;
    }
    let path = directory.path.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let mut expected = BTreeMap::new();
    for (name, document) in CASES {
        let source = format_xml::from_str(document, &project.source)?;
        let execution = engine::ExecutionContext::new(&path);
        let mapped =
            engine::run_outputs_with_sources_and_context(&project, &source, Vec::new(), &execution);
        if name == "mapping-first" {
            assert!(matches!(
                mapped,
                Err(engine::EngineError::EmptyDynamicTargetPath { node: 0 })
            ));
            expected.insert(
                name,
                json!({"source":snapshot::snapshot(&source),"members":null,"mapping_failure":true}),
            );
            continue;
        }
        let mapped = mapped?;
        assert!(mapped.extras.is_empty());
        let Instance::DocumentSet(members) = &mapped.primary else {
            panic!("native document set")
        };
        if name == "ordered" {
            let paths = members
                .iter()
                .map(|member| member.path())
                .collect::<Vec<_>>();
            assert_eq!(
                paths,
                [
                    "same.xml",
                    "same.xml",
                    "/opaque.xml",
                    "../opaque.xml",
                    "résult/雪.xml"
                ]
            );
        }
        if name == "empty" {
            assert!(members.is_empty());
        }
        if name == "late-output" {
            assert_eq!(members.len(), 2);
            assert_eq!(members[1].path(), "bad.xml");
            assert_eq!(
                members[1].value().field("Value"),
                Some(&Instance::Scalar(Value::String("\u{1}".into())))
            );
        }
        if matches!(name, "ordered" | "empty") {
            for (index, member) in members.iter().enumerate() {
                let xml = format_xml::to_string(&project.target, member.value())?;
                assert_eq!(
                    format_xml::from_str(&xml, &project.target)?,
                    *member.value()
                );
                std::fs::write(
                    directory.path.join(format!("native-{name}-{index}.xml")),
                    xml,
                )?;
            }
        }
        expected.insert(name, json!({"source":snapshot::snapshot(&source),"members":typed_members(&mapped.primary),"mapping_failure":false}));
    }
    let generated = directory.path.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let ir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ir")
        .canonicalize()?;
    let mut generate = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    generate
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        generate.arg("--rust-runtime-path").arg(&runtime);
    }
    let result = recorded(&mut generate, &directory.path, "generate")?;
    assert!(
        result.status.success(),
        "CLI generation: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let unchanged = artifact_files(&generated)?;
    let host = directory.path.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    let output = if language == "rust" {
        let package = "ferrule-xml-dynamic-output-regression-host";
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/xml_dynamic_outputs_rust.rs.txt"),
        )?;
        std::fs::write(
            host.join("src/structured_xml_snapshot.rs.txt"),
            include_str!("fixtures/structured_xml_snapshot.rs.txt"),
        )?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={package:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={ir:?}}}\nserde_json=\"1\"\nroxmltree=\"0.21\"\n"
            ),
        )?;
        let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
            Some(target) => {
                let target = PathBuf::from(target);
                if target.is_absolute() {
                    target
                } else {
                    std::env::current_dir()?.join(target)
                }
            }
            None => directory.path.join("rust-target"),
        };
        let mut build = Command::new("cargo");
        build
            .args(["build", "--quiet", "--jobs", "1"])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings");
        let result = recorded(&mut build, &directory.path, "build")?;
        assert!(
            result.status.success(),
            "Rust build: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let binary = target
            .join(format!("debug/{package}"))
            .with_extension(std::env::consts::EXE_EXTENSION);
        let mut run = Command::new(binary);
        run.arg(&directory.path).current_dir(&host);
        recorded(&mut run, &directory.path, "host")?
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/xml_dynamic_outputs_csharp.cs.txt"),
        )?;
        std::fs::write(
            host.join("Host.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
        )?;
        let artifacts = directory.path.join("artifacts");
        let mut build = dotnet_command(&directory.path);
        build
            .args([
                "build",
                "--nologo",
                "--verbosity",
                "quiet",
                "--configuration",
                "Release",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .args(["-m:1", "/p:UseSharedCompilation=false", "host/Host.csproj"])
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let result = recorded(&mut build, &directory.path, "build")?;
        assert!(
            result.status.success(),
            "C# build: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let mut run = dotnet_command(&directory.path);
        run.arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(&directory.path)
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        recorded(&mut run, &directory.path, "host")?
    };
    assert_eq!(
        artifact_files(&generated)?,
        unchanged,
        "untouched generated library"
    );
    assert!(
        output.status.success(),
        "public document-list host: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let rows = std::str::from_utf8(&output.stdout)?
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        rows.len(),
        20,
        "four complete typed diagnostics and sixteen public calls"
    );
    let mut identities = BTreeSet::new();
    for row in &rows {
        let name = row["case"].as_str().expect("exact case name");
        let api = row["api"].as_i64().expect("exact API index");
        assert!(
            identities.insert((name.to_owned(), api)),
            "unique case/API row"
        );
        let native = &expected[name];
        if api == -1 {
            assert_eq!(row["source"], native["source"], "complete parsed source");
            assert_eq!(
                row["members"], native["members"],
                "complete typed mapping with paths and origins"
            );
            assert_eq!(row["mapping_failure"], native["mapping_failure"]);
            continue;
        }
        assert!((0..4).contains(&api));
        match name {
            "ordered" | "empty" => {
                assert_eq!(row["kind"], "ok");
                let actual = row["documents"].as_array().expect("complete returned list");
                let members = native["members"].as_array().expect("native list");
                assert_eq!(actual.len(), members.len());
                for (document, member) in actual.iter().zip(members) {
                    assert_eq!(document["path"], member["path"]);
                    let xml = document["document"]
                        .as_str()
                        .expect("complete returned XML");
                    assert_eq!(
                        snapshot::snapshot(&format_xml::from_str(xml, &project.target)?),
                        member["instance"]
                    );
                    let value = member["instance"]["fields"][0]["instance"]["value"]
                        .as_str()
                        .expect("String leaf");
                    assert_eq!(
                        document["physical"],
                        expected_physical(value),
                        "all physical names, attributes, order and significant scalar text"
                    );
                    assert_eq!(
                        document["native_physical_match"], true,
                        "independent native serializer comparison"
                    );
                }
            }
            "late-output" => {
                assert_eq!(row["kind"], "Output");
                assert_eq!(
                    row["member"],
                    json!({"target":"Primary","index":1,"path":"bad.xml"})
                );
                assert_eq!(row["boundary_identity"], true);
                assert_eq!(row["original_boundary_match"], true);
                assert_eq!(row["no_partial_result"], true);
                assert_eq!(
                    (
                        row["boundary"]["bytes"].clone(),
                        row["boundary"]["limit"].clone()
                    ),
                    (Json::Null, Json::Null)
                );
                if language == "rust" {
                    assert_eq!(
                        row["boundary"]["detail"],
                        "XML output must contain XML 1.0 characters"
                    );
                    assert_eq!(row["boundary"]["causes"], json!([]));
                } else {
                    assert!(
                        !row["boundary"]["causes"]
                            .as_array()
                            .expect("original C# chain")
                            .is_empty()
                    );
                }
            }
            "mapping-first" => {
                assert_eq!(row["kind"], "Mapping");
                assert_eq!(row["member"], Json::Null);
                assert_eq!(row["boundary_identity"], true);
                assert_eq!(row["original_boundary_match"], true);
                assert_eq!(row["runtime_error"], "EmptyDynamicTargetPath");
                assert_eq!(row["node"], 0);
                assert_eq!(row["no_partial_result"], true);
            }
            _ => panic!("unknown case"),
        }
    }
    let required = CASES
        .iter()
        .flat_map(|(name, _)| (-1..4).map(move |api| (name.to_string(), api)))
        .collect::<BTreeSet<_>>();
    assert_eq!(identities, required);
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_dynamic_xml_document_lists_preserve_paths_outputs_and_original_failures()
-> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_dynamic_xml_document_lists_preserve_paths_outputs_and_original_failures()
-> TestResult<()> {
    exercise("csharp")
}
