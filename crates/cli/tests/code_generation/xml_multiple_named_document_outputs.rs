use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
const CASES: [(&str, &str); 10] = [
    (
        "ordered",
        include_str!("fixtures/xml_multiple_named_document_outputs/ordered.xml"),
    ),
    (
        "empty-alpha",
        include_str!("fixtures/xml_multiple_named_document_outputs/empty-alpha.xml"),
    ),
    (
        "empty-beta",
        include_str!("fixtures/xml_multiple_named_document_outputs/empty-beta.xml"),
    ),
    (
        "empty-both",
        include_str!("fixtures/xml_multiple_named_document_outputs/empty-both.xml"),
    ),
    (
        "selected",
        include_str!("fixtures/xml_multiple_named_document_outputs/selected.xml"),
    ),
    (
        "late-beta-output",
        include_str!("fixtures/xml_multiple_named_document_outputs/late-beta-output.xml"),
    ),
    (
        "primary-output",
        include_str!("fixtures/xml_multiple_named_document_outputs/primary-output.xml"),
    ),
    (
        "alpha-output",
        include_str!("fixtures/xml_multiple_named_document_outputs/alpha-output.xml"),
    ),
    (
        "mapping-primary",
        include_str!("fixtures/xml_multiple_named_document_outputs/mapping-primary.xml"),
    ),
    (
        "mapping-alpha",
        include_str!("fixtures/xml_multiple_named_document_outputs/mapping-alpha.xml"),
    ),
];
struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str, reverse: bool) -> io::Result<Self> {
        let d = TempDir::new(&format!("xml_multiple_named_outputs_{language}_{reverse}"))?;
        let path = d.0.clone();
        std::mem::forget(d);
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
                "Retained plural XML output test artifacts: {}",
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
    Ok(serde_json::from_str(include_str!(
        "fixtures/xml_multiple_named_document_outputs/project.json"
    ))?)
}
fn typed_extras(outputs: &engine::ExecutionOutputs) -> Json {
    json!(outputs.extras.iter().map(|e|{let Instance::DocumentSet(docs)=&e.instance else{panic!("complete named set")};json!({"name":e.name,"members":docs.iter().map(|d|json!({"path":d.path(),"instance":snapshot::snapshot(d.value())})).collect::<Vec<_>>()})}).collect::<Vec<_>>())
}
fn success(case: &str) -> bool {
    matches!(
        case,
        "ordered" | "empty-alpha" | "empty-beta" | "empty-both" | "selected"
    )
}
fn exercise(language: &str, reverse: bool) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language, reverse)?;
    let mut project = project()?;
    if reverse {
        project.extra_targets.swap(0, 1);
    }
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    assert_eq!(
        lowered.xml_output_mode()?,
        Some(codegen::XmlOutputMode::StaticPrimaryDynamicNamedDocuments)
    );
    assert_eq!(lowered.extra_targets.len(), 2);
    assert!(lowered.extra_sources.is_empty());
    for (case, document) in CASES {
        std::fs::write(directory.path.join(format!("{case}.xml")), document)?;
    }
    for (name, schema) in [("source", &project.source), ("primary", &project.target)] {
        std::fs::write(
            directory.path.join(format!("{name}-schema.txt")),
            codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?,
        )?;
    }
    for (index, target) in project.extra_targets.iter().enumerate() {
        std::fs::write(
            directory.path.join(format!("named-{index}-schema.txt")),
            codegen::serialize_embedded_schema(
                &target.schema,
                codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
            )?,
        )?;
        std::fs::write(
            directory.path.join(format!("named-{index}-hints.json")),
            serde_json::to_vec(
                target
                    .options
                    .xml_schema_hints
                    .as_ref()
                    .expect("literal own schema hint"),
            )?,
        )?;
    }
    let path = directory.path.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let mut expected = BTreeMap::new();
    for (case, document) in CASES {
        let source = format_xml::from_str(document, &project.source)?;
        let execution = engine::ExecutionContext::new(&path);
        let result =
            engine::run_outputs_with_sources_and_context(&project, &source, Vec::new(), &execution);
        if case.starts_with("mapping-") {
            assert!(matches!(
                result,
                Err(engine::EngineError::EmptyDynamicTargetPath { node: 10 })
            ));
            expected.insert(case,json!({"source":snapshot::snapshot(&source),"primary":null,"extras":null,"mapping_failure":true}));
            continue;
        }
        let mapped = result?;
        assert_eq!(mapped.extras.len(), 2);
        for (declaration, extra) in mapped.extras.iter().enumerate() {
            assert_eq!(extra.name, project.extra_targets[declaration].name);
            let Instance::DocumentSet(docs) = &extra.instance else {
                panic!("named set")
            };
            let paths = docs.iter().map(|d| d.path()).collect::<Vec<_>>();
            let alpha = extra.name == "z-audit";
            match case {
                "ordered" => {
                    assert_eq!(
                        paths,
                        if alpha {
                            vec!["same.xml", "same.xml", "résult/雪😀.xml"]
                        } else {
                            vec!["same.xml", "/opaque.xml", "../opaque.xml"]
                        }
                    );
                    if alpha {
                        assert_eq!(
                            snapshot::snapshot(docs[0].value())["fields"][0]["instance"]["bits_hex"],
                            "4340000000000000"
                        );
                        assert_eq!(
                            snapshot::snapshot(docs[1].value())["fields"][0]["instance"]["bits_hex"],
                            "8000000000000000"
                        );
                    } else {
                        assert_eq!(
                            docs[0].value().field("Amount"),
                            Some(&Instance::Scalar(Value::Int(9_007_199_254_740_993)))
                        );
                    }
                }
                "empty-alpha" => assert_eq!(docs.len(), usize::from(!alpha)),
                "empty-beta" => assert_eq!(docs.len(), usize::from(alpha)),
                "empty-both" => assert!(docs.is_empty()),
                "selected" => assert_eq!(
                    paths,
                    if alpha {
                        vec!["middle-alpha.xml", "last-alpha.xml"]
                    } else {
                        vec!["middle-beta.xml", "last-beta.xml"]
                    }
                ),
                "late-beta-output" => {
                    if !alpha {
                        assert_eq!(paths, ["middle-beta.xml", "bad-beta.xml"]);
                        assert_eq!(
                            docs[1].value().field("Value"),
                            Some(&Instance::Scalar(Value::String("\u{1}".into())))
                        );
                    }
                }
                "alpha-output" => {
                    if alpha {
                        assert_eq!(paths, ["good-alpha.xml", "bad-alpha.xml"]);
                        assert_eq!(
                            docs[1].value().field("Value"),
                            Some(&Instance::Scalar(Value::String("\u{1}".into())))
                        );
                    }
                }
                "primary-output" => assert_eq!(
                    mapped.primary.field("Marker"),
                    Some(&Instance::Scalar(Value::String("\u{1}".into())))
                ),
                _ => panic!("authored case"),
            }
            if success(case) {
                for (index, member) in docs.iter().enumerate() {
                    let options = format_xml::XmlWriteOptions {
                        schema_hints: project.extra_targets[declaration]
                            .options
                            .xml_schema_hints
                            .clone(),
                        ..Default::default()
                    };
                    let xml = format_xml::to_string_with_options(
                        &project.extra_targets[declaration].schema,
                        member.value(),
                        &options,
                    )?;
                    assert_eq!(
                        format_xml::from_str(&xml, &project.extra_targets[declaration].schema)?,
                        *member.value()
                    );
                    std::fs::write(
                        directory
                            .path
                            .join(format!("native-{case}-{declaration}-{index}.xml")),
                        xml,
                    )?;
                }
            }
        }
        if success(case) {
            let xml = format_xml::to_string(&project.target, &mapped.primary)?;
            assert_eq!(format_xml::from_str(&xml, &project.target)?, mapped.primary);
            std::fs::write(
                directory.path.join(format!("native-{case}-primary.xml")),
                xml,
            )?;
        }
        expected.insert(case,json!({"source":snapshot::snapshot(&source),"primary":snapshot::snapshot(&mapped.primary),"extras":typed_extras(&mapped),"mapping_failure":false}));
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
        let package = "ferrule-xml-multiple-named-document-output-regression-host";
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/xml_multiple_named_document_outputs_rust.rs.txt"),
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
            include_str!("fixtures/xml_multiple_named_document_outputs_csharp.cs.txt"),
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
        50,
        "ten complete typed diagnostics and forty public calls"
    );
    let mut identities = BTreeSet::new();
    for row in &rows {
        let case = row["case"].as_str().unwrap();
        let api = row["api"].as_i64().unwrap();
        assert!(identities.insert((case.to_owned(), api)));
        let native = &expected[case];
        if api == -1 {
            for field in ["source", "primary", "extras", "mapping_failure"] {
                assert_eq!(
                    row[field], native[field],
                    "complete native/generated typed {field}"
                );
            }
            continue;
        }
        assert!((0..4).contains(&api));
        if success(case) {
            assert_eq!(row["kind"], "ok");
            assert_eq!(row["primary"]["native_physical_match"], true);
            assert_eq!(
                snapshot::snapshot(&format_xml::from_str(
                    row["primary"]["document"].as_str().unwrap(),
                    &project.target
                )?),
                native["primary"]
            );
            let extras = row["extras"].as_array().unwrap();
            let wanted = native["extras"].as_array().unwrap();
            assert_eq!(extras.len(), 2);
            for (declaration, (extra, expected)) in extras.iter().zip(wanted).enumerate() {
                assert_eq!(extra["name"], expected["name"]);
                let docs = extra["documents"].as_array().unwrap();
                let members = expected["members"].as_array().unwrap();
                assert_eq!(docs.len(), members.len());
                let alpha = extra["name"] == "z-audit";
                let uri = if alpha { "urn:audit" } else { "urn:receipt" };
                let root = if alpha { "Audit" } else { "Receipt" };
                let hint = if alpha {
                    "urn:audit audit.xsd"
                } else {
                    "urn:receipt receipt.xsd"
                };
                for (doc, member) in docs.iter().zip(members) {
                    assert_eq!(doc["path"], member["path"]);
                    assert_eq!(
                        snapshot::snapshot(&format_xml::from_str(
                            doc["document"].as_str().unwrap(),
                            &project.extra_targets[declaration].schema
                        )?),
                        member["instance"]
                    );
                    assert_eq!(doc["native_physical_match"], true);
                    let physical = &doc["physical"];
                    assert_eq!(physical["name"], root);
                    assert_eq!(physical["namespace"], uri);
                    assert_eq!(
                        physical["attributes"],
                        json!([{"name":"schemaLocation","namespace":"http://www.w3.org/2001/XMLSchema-instance","value":hint}])
                    );
                    let children = physical["children"].as_array().unwrap();
                    assert_eq!(children.len(), 2);
                    for (child, name) in children.iter().zip(["Amount", "Value"]) {
                        assert_eq!(child["name"], name);
                        assert_eq!(child["namespace"], uri);
                        assert_eq!(child["attributes"], json!([]));
                        assert_eq!(child["children"], json!([]));
                    }
                    assert_eq!(
                        children[1]["text"],
                        member["instance"]["fields"][1]["instance"]["value"]
                    );
                }
            }
        } else {
            assert_eq!(
                row["wrapper"],
                if language == "rust" {
                    "XmlDocumentOutputsExecutionError"
                } else {
                    "FerruleXmlDocumentOutputsExecutionException"
                }
            );
            assert_eq!(row["boundary_identity"], true);
            assert_eq!(row["original_boundary_match"], true);
            assert_eq!(row["no_partial_result"], true);
            assert!(row["boundary"]["bytes"].is_null() && row["boundary"]["limit"].is_null());
            if case.starts_with("mapping-") {
                assert_eq!(row["kind"], "Mapping");
                assert!(row["owner"].is_null());
                assert_eq!(row["runtime_error"], "EmptyDynamicTargetPath");
                assert_eq!(row["node"], 10);
                assert!(!row["boundary"]["causes"].as_array().unwrap().is_empty());
            } else {
                assert_eq!(row["kind"], "Output");
                assert_eq!(row["boundary"], row["original_boundary"]);
                let owner = if case == "primary-output" {
                    json!({"kind":"Primary"})
                } else {
                    let name = if case == "late-beta-output" {
                        "a-receipt"
                    } else {
                        "z-audit"
                    };
                    let declaration = project
                        .extra_targets
                        .iter()
                        .position(|t| t.name == name)
                        .unwrap();
                    let path = if case == "late-beta-output" {
                        "bad-beta.xml"
                    } else {
                        "bad-alpha.xml"
                    };
                    json!({"kind":"NamedMember","declaration_index":declaration,"name":name,"index":1,"path":path})
                };
                assert_eq!(row["owner"], owner);
                if language == "rust" {
                    assert_eq!(
                        row["boundary"]["detail"],
                        "XML output must contain XML 1.0 characters"
                    );
                    assert_eq!(row["boundary"]["causes"], json!([]));
                } else {
                    assert!(!row["boundary"]["causes"].as_array().unwrap().is_empty());
                }
            }
        }
    }
    let required = CASES
        .iter()
        .flat_map(|(case, _)| (-1..4).map(move |api| (case.to_string(), api)))
        .collect::<BTreeSet<_>>();
    assert_eq!(identities, required);
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_rust_plural_named_xml_outputs_preserve_declared_schemas_order_and_original_causes()
-> TestResult<()> {
    for reverse in [false, true] {
        exercise("rust", reverse)?;
    }
    Ok(())
}
#[test]
fn generated_csharp_plural_named_xml_outputs_preserve_declared_schemas_order_and_original_causes()
-> TestResult<()> {
    for reverse in [false, true] {
        exercise("csharp", reverse)?;
    }
    Ok(())
}
