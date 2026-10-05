use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

const METHODS: [&str; 4] = ["text", "text-context", "bytes", "bytes-context"];
const CASES: [&str; 3] = ["base", "context", "host-failure"];
const DOCUMENTS: [(&str, &str); 6] = [
    (
        "primary.xml",
        include_str!("fixtures/xml_dynamic_input_document_sets/primary.xml"),
    ),
    (
        "context.xml",
        include_str!("fixtures/xml_dynamic_input_document_sets/context.xml"),
    ),
    (
        "rates.xml",
        include_str!("fixtures/xml_dynamic_input_document_sets/rates.xml"),
    ),
    (
        "labels.xml",
        include_str!("fixtures/xml_dynamic_input_document_sets/labels.xml"),
    ),
    (
        "a.xml",
        include_str!("fixtures/xml_dynamic_input_document_sets/a.xml"),
    ),
    (
        "b.xml",
        include_str!("fixtures/xml_dynamic_input_document_sets/b.xml"),
    ),
];

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        static NEXT_DIRECTORY_ID: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_dynamic_input_document_sets_{language}_{}_{}",
            std::process::id(),
            NEXT_DIRECTORY_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
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
                "Retained dynamic XML input document-list artifacts: {}",
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
fn physical(xml: &str, schema: &Json) -> Json {
    fn element(node: roxmltree::Node<'_, '_>, schema: &Json) -> Json {
        let mut attributes = node.attributes().map(|a| json!({"name":a.name(),"namespace":a.namespace().unwrap_or(""),"value":a.value()})).collect::<Vec<_>>();
        attributes.sort_by_key(|a| {
            (
                a["namespace"].as_str().unwrap().to_owned(),
                a["name"].as_str().unwrap().to_owned(),
            )
        });
        let text = node
            .children()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .collect::<String>();
        let group = schema["kind"]["kind"] == "group";
        let children = node
            .children()
            .filter(|n| n.is_element())
            .map(|child| {
                let fields = schema["kind"]["children"]
                    .as_array()
                    .expect("closed Group child schema");
                let field = fields
                    .iter()
                    .find(|f| f["name"] == child.tag_name().name() && f["attribute"] != true)
                    .expect("exact child schema");
                element(child, field)
            })
            .collect::<Vec<_>>();
        let text = if group {
            assert!(
                text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')),
                "element-only Group formatting"
            );
            String::new()
        } else {
            text
        };
        json!({"name":node.tag_name().name(),"namespace":node.tag_name().namespace().unwrap_or(""),"attributes":attributes,"text":text,"children":children})
    }
    element(
        roxmltree::Document::parse(xml).unwrap().root_element(),
        schema,
    )
}

struct NativeLoader<'a> {
    project: &'a Project,
    directory: &'a Path,
    fail: bool,
    calls: RefCell<Vec<Json>>,
}
impl engine::DynamicSourceLoader for NativeLoader<'_> {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        let index = self.calls.borrow().len();
        self.calls
            .borrow_mut()
            .push(json!({"source":source,"path":path,"loaded":null,"host_error":null}));
        let failure = if source != "catalog" || !matches!(path, "a.xml" | "b.xml") {
            Some("unexpected authored source/path")
        } else if self.fail && path == "b.xml" {
            Some("original second catalog failure")
        } else {
            None
        };
        if let Some(message) = failure {
            self.calls.borrow_mut()[index]["host_error"] = json!(message);
            return Err(message.into());
        }
        let document = std::fs::read_to_string(self.directory.join(path)).map_err(|error| {
            self.calls.borrow_mut()[index]["host_error"] = json!(error.to_string());
            error.to_string()
        })?;
        let instance = format_xml::from_str(&document, &self.project.extra_sources[1].schema)
            .map_err(|error| error.to_string())?;
        self.calls.borrow_mut()[index]["loaded"] = snapshot::snapshot(&instance);
        Ok(Arc::new(instance))
    }
}
fn native(project: &Project, directory: &Path, case: &str, context: bool) -> TestResult<Json> {
    let primary = format_xml::from_str(
        &std::fs::read_to_string(directory.join(if case == "context" {
            "context.xml"
        } else {
            "primary.xml"
        }))?,
        &project.source,
    )?;
    let labels = format_xml::from_str(
        &std::fs::read_to_string(directory.join("labels.xml"))?,
        &project.extra_sources[2].schema,
    )?;
    let rates = format_xml::from_str(
        &std::fs::read_to_string(directory.join("rates.xml"))?,
        &project.extra_sources[0].schema,
    )?;
    let inputs = json!({"primary":snapshot::snapshot(&primary),"statics":[
        {"name":"labels","declaration_index":2,"instance":snapshot::snapshot(&labels)},
        {"name":"rates","declaration_index":0,"instance":snapshot::snapshot(&rates)}]});
    let loader = NativeLoader {
        project,
        directory,
        fail: case == "host-failure",
        calls: RefCell::new(Vec::new()),
    };
    let mut execution = engine::ExecutionContext::with_main_mapping_file_path(
        Path::new("mapping/library.json"),
        Path::new("mapping/main.json"),
    );
    if context {
        execution = execution.with_current_datetime("2026-10-05T00:00:00Z");
    }
    execution = execution.with_dynamic_source_loader(&loader);
    let result = engine::run_outputs_with_sources_and_context(
        project,
        &primary,
        vec![("labels".into(), labels), ("rates".into(), rates)],
        &execution,
    );
    let mut row = json!({"case":case,"context":context,"inputs":inputs,"calls":loader.calls.borrow().clone(),"outputs":null,"error":null});
    match result {
        Ok(mapped) => {
            assert!(mapped.extras.is_empty());
            let Instance::DocumentSet(members) = mapped.primary else {
                panic!("complete native DocumentSet")
            };
            let target = serde_json::to_value(&project.target)?;
            let mut documents = Vec::new();
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
                std::fs::write(
                    directory.join(format!("native-{case}-{context}-{index}.xml")),
                    &xml,
                )?;
                let parsed = format_xml::from_str(&xml, &project.target)?;
                assert_eq!(
                    snapshot::snapshot(&parsed),
                    snapshot::snapshot(member.value()),
                    "complete native XML roundtrip"
                );
                documents.push(json!({"path":member.path(),"instance":snapshot::snapshot(member.value()),"physical":physical(&xml,&target)}));
            }
            row["outcome"] = json!("success");
            row["outputs"] = json!(documents);
        }
        Err(original) => {
            let fields = match &original {
                engine::EngineError::DynamicSourceLoad {
                    source_name,
                    path,
                    message,
                } => {
                    json!({"variant":"DynamicSourceLoad","source":source_name,"path":path,"message":message})
                }
                engine::EngineError::MissingRuntimeValue(value) => {
                    json!({"variant":"MissingRuntimeValue","value":format!("{value:?}")})
                }
                _ => panic!("unexpected native error: {original:?}"),
            };
            row["outcome"] = json!("error");
            row["error"] = json!({"detail":original.to_string(),"debug":format!("{original:?}"),"runtime":fields});
        }
    }
    std::fs::write(
        directory.join(format!("native-{case}-{context}.json")),
        serde_json::to_vec_pretty(&row)?,
    )?;
    Ok(row)
}
fn field<'a>(instance: &'a Json, name: &str) -> &'a Json {
    &instance["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .unwrap()["instance"]
}
fn check_native(row: &Json) {
    let case = row["case"].as_str().unwrap();
    let context = row["context"].as_bool().unwrap();
    let calls = row["calls"].as_array().unwrap();
    let paths = calls
        .iter()
        .map(|call| call["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    let succeeds = case == "base" || (case == "context" && context);
    if succeeds {
        assert_eq!(
            paths,
            ["a.xml", "b.xml", "a.xml", "b.xml", "a.xml", "b.xml"]
        );
        let outputs = row["outputs"].as_array().unwrap();
        assert_eq!(
            outputs
                .iter()
                .map(|out| out["path"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["same.xml", "same.xml", "../雪😀.xml"]
        );
        for (index, output) in outputs.iter().enumerate() {
            let instance = &output["instance"];
            assert_eq!(
                field(instance, "Id"),
                &json!({"kind":"Scalar","tag":"Int","value":([30,10,20][index])})
            );
            let marker = if case == "context" && index == 1 {
                "2026-10-05T00:00:00Z"
            } else {
                "  完了 & 😀  "
            };
            assert_eq!(
                field(instance, "Marker"),
                &json!({"kind":"Scalar","tag":"String","value":marker})
            );
            let lines = field(instance, "Line")["items"].as_array().unwrap();
            assert_eq!(lines.len(), 2);
            assert_eq!(field(&lines[0], "DriverId")["value"], 101);
            assert_eq!(field(&lines[1], "DriverId")["value"], 102);
            assert_eq!(field(&lines[0], "Amount")["bits_hex"], "8000000000000000");
            assert_eq!(field(&lines[0], "Adjustment")["tag"], "XmlNil");
            assert_eq!(field(&lines[0], "Text")["value"], "  雪 😀  ");
            assert_eq!(field(&lines[1], "Amount")["bits_hex"], "4340000000000000");
            assert_eq!(output["physical"]["namespace"], "urn:document");
            assert!(output["physical"]["attributes"].as_array().unwrap().iter().any(|attr|attr == &json!({"name":"schemaLocation","namespace":"http://www.w3.org/2001/XMLSchema-instance","value":"urn:document documents.xsd"})));
        }
    } else {
        assert_eq!(paths, ["a.xml", "b.xml"]);
        assert_eq!(row["outputs"], Json::Null);
        let variant = if case == "host-failure" {
            "DynamicSourceLoad"
        } else {
            "MissingRuntimeValue"
        };
        assert_eq!(row["error"]["runtime"]["variant"], variant);
        if case == "host-failure" {
            assert_eq!(calls[1]["host_error"], "original second catalog failure");
        }
    }
}

fn exercise(language: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language)?;
    let project: Project = serde_json::from_str(include_str!(
        "fixtures/xml_dynamic_input_document_sets/project.json"
    ))?;
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        codegen::lower(&project)?.xml_output_mode()?,
        Some(codegen::XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments)
    );
    assert_eq!(
        project
            .extra_sources
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog", "labels"]
    );
    assert!(project.extra_targets.is_empty());
    for (name, xml) in DOCUMENTS {
        std::fs::write(directory.path.join(name), xml)?;
    }
    for (name, schema) in [
        ("source", &project.source),
        ("rates", &project.extra_sources[0].schema),
        ("catalog", &project.extra_sources[1].schema),
        ("labels", &project.extra_sources[2].schema),
        ("target", &project.target),
    ] {
        std::fs::write(
            directory.path.join(format!("{name}-schema.json")),
            codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?,
        )?;
    }
    let mut expected = BTreeMap::new();
    for case in CASES {
        for context in [false, true] {
            let row = native(&project, &directory.path, case, context)?;
            check_native(&row);
            expected.insert((case, context), row);
        }
    }
    let project_path = directory.path.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
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
        .arg(&project_path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        generate.arg("--rust-runtime-path").arg(&runtime);
    }
    let result = recorded(&mut generate, &directory.path, "generate")?;
    assert!(
        result.status.success(),
        "generation: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original_tree = artifact_files(&generated)?;
    let host = directory.path.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    let output = if language == "rust" {
        let package = "ferrule-xml-dynamic-input-document-list-regression-host";
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/xml_dynamic_input_document_sets_rust.rs.txt"),
        )?;
        std::fs::write(
            host.join("src/snapshot.rs"),
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
        let mut run = Command::new(
            target
                .join(format!("debug/{package}"))
                .with_extension(std::env::consts::EXE_EXTENSION),
        );
        run.arg(&directory.path).current_dir(&host);
        recorded(&mut run, &directory.path, "host")?
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/xml_dynamic_input_document_sets_csharp.cs.txt"),
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
        original_tree,
        "complete generated tree unchanged"
    );
    assert!(
        output.status.success(),
        "recording host: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let rows = std::str::from_utf8(&output.stdout)?
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    let diagnostics = std::str::from_utf8(&output.stderr)?
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(rows.len(), 12);
    assert_eq!(diagnostics.len(), 6);
    let mut diagnostic_keys = BTreeSet::new();
    let mut parsed = BTreeMap::new();
    for row in diagnostics {
        let key = (
            row["case"].as_str().unwrap().to_owned(),
            row["method"].as_str().unwrap().to_owned(),
        );
        assert!(diagnostic_keys.insert(key.clone()));
        assert!(row.get("diagnostic_error").is_none());
        if language == "csharp" {
            assert_eq!(row["readonly_list"], true);
        }
        parsed.insert(key, row);
    }
    let mut identities = BTreeSet::new();
    for row in &rows {
        let case = row["case"].as_str().unwrap();
        let method = row["method"].as_str().unwrap();
        assert!(CASES.contains(&case) && METHODS.contains(&method));
        assert!(identities.insert((case, method)));
        let native = &expected[&(case, method.ends_with("context"))];
        assert_eq!(
            row["inputs"], native["inputs"],
            "every parsed source and original declaration identity"
        );
        assert_eq!(
            row["calls"], native["calls"],
            "every attempted callback and complete loaded snapshot"
        );
        assert_eq!(row["outcome"], native["outcome"]);
        if native["outcome"] == "success" {
            assert_eq!(row["error"], Json::Null);
            assert_eq!(row["distinct_output_buffers"], true);
            if language == "csharp" {
                assert_eq!(
                    row["public_byte_buffers_checked"],
                    method.starts_with("bytes")
                );
            }
            let outputs = row["outputs"].as_array().unwrap();
            let expected_outputs = native["outputs"].as_array().unwrap();
            assert_eq!(outputs.len(), expected_outputs.len());
            let diagnostic = &parsed[&(case.to_owned(), method.to_owned())];
            assert_eq!(
                diagnostic["outputs"].as_array().unwrap().len(),
                outputs.len()
            );
            for ((output, typed), expected_output) in outputs
                .iter()
                .zip(diagnostic["outputs"].as_array().unwrap())
                .zip(expected_outputs)
            {
                let xml = output["xml"].as_str().unwrap();
                assert_eq!(output["bytes"].as_u64().unwrap(), xml.len() as u64);
                assert_eq!(output["path"], expected_output["path"]);
                assert_eq!(typed["path"], expected_output["path"]);
                assert_eq!(
                    snapshot::snapshot(&format_xml::from_str(xml, &project.target)?),
                    expected_output["instance"],
                    "whole returned target typed values"
                );
                assert_eq!(typed["instance"], expected_output["instance"]);
                assert_eq!(
                    typed["physical"], expected_output["physical"],
                    "all expanded names/attributes/order/nil/significant scalar spaces"
                );
            }
        } else {
            assert_eq!(row["outputs"], Json::Null, "no returned partial set");
            let error = &row["error"];
            assert_eq!(error["kind"], "Mapping");
            assert_eq!(error["owner"], Json::Null);
            assert_eq!(error["request"], Json::Null);
            assert_eq!(error["boundary_identity"], true);
            assert_eq!(error["bytes"], Json::Null);
            assert_eq!(error["limit"], Json::Null);
            assert_eq!(error["runtime"], native["error"]["runtime"]);
            assert!(!error["causes"].as_array().unwrap().is_empty());
            if language == "csharp" && case == "host-failure" {
                assert_eq!(
                    error["original_host_identity"], true,
                    "exact original host exception remains in chain"
                );
            }
        }
    }
    assert_eq!(
        identities,
        CASES
            .into_iter()
            .flat_map(|case| METHODS.into_iter().map(move |method| (case, method)))
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(
        diagnostic_keys,
        rows.iter()
            .filter(|row| row["outcome"] == "success")
            .map(|row| (
                row["case"].as_str().unwrap().to_owned(),
                row["method"].as_str().unwrap().to_owned()
            ))
            .collect::<BTreeSet<_>>()
    );
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_dynamic_xml_document_loader_preserves_members_context_and_original_host_refusal()
-> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_dynamic_xml_document_loader_preserves_members_context_and_original_host_refusal()
-> TestResult<()> {
    exercise("csharp")
}
