use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("xml_input_document_sets_{language}"))?;
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
                "Retained named-input dynamic XML document test artifacts: {}",
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
        "fixtures/xml_input_document_sets/project.json"
    ))?)
}
fn cases() -> TestResult<Vec<Json>> {
    Ok(serde_json::from_str(include_str!(
        "fixtures/xml_input_document_sets/cases.json"
    ))?)
}
fn typed_members(value: &Instance) -> Json {
    let Instance::DocumentSet(members) = value else {
        panic!("complete primary document set")
    };
    json!(
        members
            .iter()
            .map(|m| json!({"path":m.path(),"instance":snapshot::snapshot(m.value())}))
            .collect::<Vec<_>>()
    )
}
fn field<'a>(group: &'a Json, name: &str) -> &'a Json {
    &group["fields"]
        .as_array()
        .expect("whole ordered Group")
        .iter()
        .find(|f| f["name"] == name)
        .expect("exact declared field")["instance"]
}
fn physical(xml: &str) -> Json {
    fn element(node: roxmltree::Node<'_, '_>) -> Json {
        let mut attrs = node
            .attributes()
            .map(|a| (a.namespace().unwrap_or(""), a.name(), a.value()))
            .collect::<Vec<_>>();
        attrs.sort();
        let children = node
            .children()
            .filter(|n| n.is_element())
            .map(element)
            .collect::<Vec<_>>();
        let raw = node
            .children()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .collect::<String>();
        let text = if children.is_empty() {
            Some(raw)
        } else {
            assert!(raw.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')));
            None
        };
        json!({"name":node.tag_name().name(),"namespace":node.tag_name().namespace().unwrap_or(""),"text":text,
            "attributes":attrs.iter().map(|(namespace,name,value)|json!({"name":name,"namespace":namespace,"value":value})).collect::<Vec<_>>(),"children":children})
    }
    element(
        roxmltree::Document::parse(xml)
            .expect("complete XML")
            .root_element(),
    )
}
fn prepare(
    directory: &Path,
    project: &Project,
    tests: &[Json],
) -> TestResult<BTreeMap<String, Json>> {
    const INPUTS: &[(&str, &[u8])] = &[
        (
            "ordered.xml",
            include_bytes!("fixtures/xml_input_document_sets/ordered.xml"),
        ),
        (
            "empty.xml",
            include_bytes!("fixtures/xml_input_document_sets/empty.xml"),
        ),
        (
            "selected.xml",
            include_bytes!("fixtures/xml_input_document_sets/selected.xml"),
        ),
        (
            "late-output.xml",
            include_bytes!("fixtures/xml_input_document_sets/late-output.xml"),
        ),
        (
            "mapping-first.xml",
            include_bytes!("fixtures/xml_input_document_sets/mapping-first.xml"),
        ),
        (
            "primary-malformed.xml",
            include_bytes!("fixtures/xml_input_document_sets/primary-malformed.xml"),
        ),
        (
            "rates.xml",
            include_bytes!("fixtures/xml_input_document_sets/rates.xml"),
        ),
        (
            "rates-negative-zero.xml",
            include_bytes!("fixtures/xml_input_document_sets/rates-negative-zero.xml"),
        ),
        (
            "rates-malformed.xml",
            include_bytes!("fixtures/xml_input_document_sets/rates-malformed.xml"),
        ),
        (
            "count.xml",
            include_bytes!("fixtures/xml_input_document_sets/count.xml"),
        ),
        (
            "count-disabled.xml",
            include_bytes!("fixtures/xml_input_document_sets/count-disabled.xml"),
        ),
        (
            "count-malformed.xml",
            include_bytes!("fixtures/xml_input_document_sets/count-malformed.xml"),
        ),
        (
            "invalid-utf8.bin",
            include_bytes!("fixtures/xml_input_document_sets/invalid-utf8.bin"),
        ),
    ];
    for (name, bytes) in INPUTS {
        std::fs::write(directory.join(name), bytes)?;
    }
    let path = directory.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(project)?)?;
    std::fs::write(
        directory.join("cases.json"),
        serde_json::to_vec_pretty(tests)?,
    )?;
    let hints = serde_json::to_string(
        project
            .target_options
            .xml_schema_hints
            .as_ref()
            .expect("own target hint"),
    )?;
    let descriptors = json!({"source":codegen::serialize_embedded_schema(&project.source,codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?,
        "target":codegen::serialize_embedded_schema(&project.target,codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?,"hints":hints,
        "inputs":project.extra_sources.iter().map(|s|Ok(json!({"name":s.name,"schema":codegen::serialize_embedded_schema(&s.schema,codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?}))).collect::<TestResult<Vec<_>>>()?});
    std::fs::write(
        directory.join("descriptors.json"),
        serde_json::to_vec_pretty(&descriptors)?,
    )?;
    let mut expected = BTreeMap::new();
    for test in tests.iter().filter(|t| t["semantic"] == true) {
        let name = test["case"].as_str().expect("case");
        let input = std::fs::read_to_string(
            directory.join(test["primary"].as_str().expect("primary file")),
        )?;
        let source = format_xml::from_str(&input, &project.source)?;
        let mut declared = Vec::new();
        for schema in &project.extra_sources {
            let supplied = test["inputs"]
                .as_array()
                .expect("complete supplied pairs")
                .iter()
                .find(|p| p["name"] == schema.name)
                .expect("complete native named pair");
            let xml = std::fs::read_to_string(
                directory.join(supplied["file"].as_str().expect("named file")),
            )?;
            declared.push((
                schema.name.clone(),
                format_xml::from_str(&xml, &schema.schema)?,
            ));
        }
        let input_records = declared
            .iter()
            .map(|(name, value)| json!({"name":name,"instance":snapshot::snapshot(value)}))
            .collect::<Vec<_>>();
        assert_eq!(field(&input_records[0]["instance"], "Rate")["tag"], "Float");
        assert_eq!(
            field(&input_records[0]["instance"], "Rate")["bits_hex"],
            if name == "ordered" {
                "8000000000000000"
            } else {
                "4340000000000000"
            }
        );
        assert_eq!(field(&input_records[1]["instance"], "Count")["tag"], "Int");
        assert_eq!(
            field(&input_records[1]["instance"], "Count")["value"],
            json!(9007199254740993_i64)
        );
        declared.reverse();
        let execution = engine::ExecutionContext::new(&path);
        let mapped =
            engine::run_outputs_with_sources_and_context(project, &source, declared, &execution);
        if name == "mapping-first" {
            assert!(matches!(
                mapped,
                Err(engine::EngineError::EmptyDynamicTargetPath { node: 0 })
            ));
            expected.insert(name.to_owned(),json!({"source":snapshot::snapshot(&source),"inputs":input_records,"members":null,"mapping_failure":true}));
            continue;
        }
        let mapped = mapped?;
        assert!(mapped.extras.is_empty());
        let Instance::DocumentSet(members) = &mapped.primary else {
            panic!("native list")
        };
        let paths = members.iter().map(|m| m.path()).collect::<Vec<_>>();
        match name {
            "ordered" => assert_eq!(
                paths,
                [
                    "same.xml",
                    "/opaque.xml",
                    "same.xml",
                    "../opaque.xml",
                    "résult/雪😀.xml"
                ]
            ),
            "empty" | "filtered" => assert!(members.is_empty()),
            "selected" => assert_eq!(paths, ["middle.xml", "last.xml"]),
            "late-output" => {
                assert_eq!(paths, ["ok.xml", "bad.xml"]);
                assert_eq!(
                    members[1].value().field("Value"),
                    Some(&Instance::Scalar(Value::String("\u{1}".into())))
                );
            }
            _ => panic!("closed native case"),
        }
        if test["kind"] == "ok" {
            for (index, member) in members.iter().enumerate() {
                let xml = format_xml::to_string_with_options(
                    &project.target,
                    member.value(),
                    &format_xml::XmlWriteOptions {
                        declaration: true,
                        indent: true,
                        default_namespace: None,
                        schema_hints: project.target_options.xml_schema_hints.clone(),
                    },
                )?;
                assert_eq!(
                    format_xml::from_str(&xml, &project.target)?,
                    *member.value()
                );
                std::fs::write(directory.join(format!("native-{name}-{index}.xml")), xml)?;
            }
        }
        expected.insert(name.to_owned(),json!({"source":snapshot::snapshot(&source),"inputs":input_records,"members":typed_members(&mapped.primary),"mapping_failure":false}));
    }
    Ok(expected)
}
fn exercise(language: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language)?;
    let project = project()?;
    let tests = cases()?;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    assert_eq!(
        lowered.xml_output_mode()?,
        Some(codegen::XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments)
    );
    assert!(lowered.extra_targets.is_empty());
    assert_eq!(
        project
            .extra_sources
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["z-rates", "a-count"]
    );
    let expected = prepare(&directory.path, &project, &tests)?;
    let path = directory.path.join("project.json");
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
    assert_eq!(unchanged.len(), if language == "rust" { 2 } else { 74 });
    let api = std::fs::read_to_string(generated.join(if language == "rust" {
        "src/lib.rs"
    } else {
        "GeneratedMapping.cs"
    }))?;
    if language == "rust" {
        for name in [
            "execute_xml_documents_with_sources",
            "execute_xml_documents_with_sources_and_context",
            "execute_xml_bytes_documents_with_sources",
            "execute_xml_bytes_documents_with_sources_and_context",
        ] {
            assert_eq!(
                api.matches(&format!("pub fn {name}(")).count(),
                1,
                "only one public signature"
            );
        }
        for name in [
            "execute_xml",
            "execute_xml_documents",
            "execute_xml_outputs",
            "execute_xml_document_outputs",
        ] {
            assert!(!api.contains(&format!("pub fn {name}(")));
        }
    } else {
        assert_eq!(api.matches(" ExecuteXmlDocumentsWithSources(").count(), 2);
        assert_eq!(
            api.matches(" ExecuteXmlBytesDocumentsWithSources(").count(),
            2
        );
        for name in [
            "ExecuteXml",
            "ExecuteXmlDocuments",
            "ExecuteXmlOutputs",
            "ExecuteXmlDocumentOutputs",
        ] {
            assert!(!api.contains(&format!(" {name}(")));
        }
        assert!(
            generated
                .join("Runtime/FerruleXml.InputDocumentSet.cs")
                .is_file()
        );
        assert!(
            generated
                .join("Runtime/FerruleXml.InputDocumentOutputs.cs")
                .is_file()
        );
    }

    let host = directory.path.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    let output = if language == "rust" {
        let package = "ferrule-xml-named-input-document-regression-host";
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/xml_input_document_sets_rust.rs.txt"),
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
            include_str!("fixtures/xml_input_document_sets_csharp.cs.txt"),
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
        "all generated source untouched by caller build and run"
    );
    assert!(
        output.status.success(),
        "named-input document-list host: {}\n{}",
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
        if language == "rust" { 72 } else { 76 },
        "all public rows and complete typed diagnostics"
    );
    let mut identities = BTreeSet::new();
    for row in &rows {
        let name = row["case"].as_str().expect("case");
        let api = row["api"].as_i64().expect("API");
        assert!(identities.insert((name.to_owned(), api)), "unique key");
        if name == "csharp-null-document-before-decode" {
            assert_eq!(language, "csharp");
            assert!((0..4).contains(&api));
            assert_eq!(row["kind"], "host-shape");
            assert_eq!(row["exception"], "System.ArgumentNullException");
            assert!(row["param_name"].as_str().is_some_and(|v| !v.is_empty()));
            assert_eq!(row["no_partial_result"], true);
            continue;
        }
        let test = tests
            .iter()
            .find(|t| t["case"] == name)
            .expect("closed fixture case");
        if api == -1 {
            assert_eq!(test["semantic"], true);
            let native = &expected[name];
            for key in ["source", "inputs", "members", "mapping_failure"] {
                assert_eq!(
                    row[key], native[key],
                    "complete native typed source/named inputs/output documents"
                );
            }
            continue;
        }
        assert!(test["apis"].as_array().expect("APIs").contains(&json!(api)));
        assert_eq!(row["kind"], test["kind"]);
        if test["kind"] == "ok" {
            let actual = row["documents"]
                .as_array()
                .expect("complete public returned list");
            let native = expected[name]["members"]
                .as_array()
                .expect("native members");
            assert_eq!(actual.len(), native.len());
            for (index, (document, member)) in actual.iter().zip(native).enumerate() {
                assert_eq!(document["path"], member["path"]);
                let xml = document["document"].as_str().expect("whole owned XML");
                assert_eq!(
                    snapshot::snapshot(&format_xml::from_str(xml, &project.target)?),
                    member["instance"],
                    "typed exact integer, negative-zero float, nil, quotes and Unicode"
                );
                assert_eq!(
                    document["physical"],
                    physical(&std::fs::read_to_string(
                        directory.path.join(format!("native-{name}-{index}.xml"))
                    )?),
                    "complete native physical names/attributes/scalar text/order"
                );
                assert_eq!(document["native_physical_match"], true);
            }
        } else {
            assert_eq!(row["owner"], test["owner"]);
            assert_eq!(row["boundary_identity"], true);
            assert_eq!(row["original_boundary_match"], true);
            assert_eq!(row["no_partial_result"], true);
            assert_eq!(row["boundary"], row["original_boundary"]);
            assert!(row.get("documents").is_none(), "no partially returned list");
            if test["primitive"]["phase"] == "names" {
                assert_eq!(
                    row["boundary"]["runtime"]["error"],
                    test["primitive"]["error"]
                );
                assert_eq!(
                    row["boundary"]["runtime"]["name"],
                    test["primitive"]["name"]
                );
            }
            if name == "mapping-first" {
                assert_eq!(
                    row["boundary"]["runtime"]["error"],
                    "EmptyDynamicTargetPath"
                );
                assert_eq!(row["boundary"]["runtime"]["node"], 0);
            }
            if name == "late-output" {
                assert_eq!(
                    row["boundary"]["detail"],
                    "XML output must contain XML 1.0 characters"
                );
                assert_eq!(
                    (
                        row["boundary"]["bytes"].clone(),
                        row["boundary"]["limit"].clone()
                    ),
                    (Json::Null, Json::Null)
                );
                if language == "rust" {
                    assert_eq!(row["boundary"]["causes"], json!([]));
                } else {
                    assert!(
                        !row["boundary"]["causes"]
                            .as_array()
                            .expect("original cause chain")
                            .is_empty()
                    );
                }
            }
        }
    }
    let mut required = tests
        .iter()
        .flat_map(|test| {
            test["apis"]
                .as_array()
                .expect("APIs")
                .iter()
                .map(move |api| {
                    (
                        test["case"].as_str().expect("case").to_owned(),
                        api.as_i64().expect("API"),
                    )
                })
        })
        .collect::<BTreeSet<_>>();
    for test in tests.iter().filter(|test| test["semantic"] == true) {
        required.insert((test["case"].as_str().expect("case").to_owned(), -1));
    }
    if language == "csharp" {
        for api in 0..4 {
            required.insert(("csharp-null-document-before-decode".into(), api));
        }
    }
    assert_eq!(identities, required);
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_rust_static_named_inputs_dynamic_xml_document_lists_preserve_original_owners_and_outputs()
-> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_static_named_inputs_dynamic_xml_document_lists_preserve_original_owners_and_outputs()
-> TestResult<()> {
    exercise("csharp")
}
