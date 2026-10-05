use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

const METHODS: [&str; 8] = [
    "text-set",
    "text-set-context",
    "bytes-set",
    "bytes-set-context",
    "text",
    "text-context",
    "bytes",
    "bytes-context",
];
const DOCUMENTS: [(&str, &str); 11] = [
    (
        "default.xml",
        include_str!("fixtures/rootview_named_inputs/documents/default.xml"),
    ),
    (
        "derived.xml",
        include_str!("fixtures/rootview_named_inputs/documents/derived.xml"),
    ),
    (
        "padded.xml",
        include_str!("fixtures/rootview_named_inputs/documents/padded.xml"),
    ),
    (
        "other.xml",
        include_str!("fixtures/rootview_named_inputs/documents/other.xml"),
    ),
    (
        "missing-extra.xml",
        include_str!("fixtures/rootview_named_inputs/documents/missing-extra.xml"),
    ),
    (
        "malformed.xml",
        include_str!("fixtures/rootview_named_inputs/documents/malformed.xml"),
    ),
    (
        "rates.xml",
        include_str!("fixtures/rootview_named_inputs/documents/rates.xml"),
    ),
    (
        "labels.xml",
        include_str!("fixtures/rootview_named_inputs/documents/labels.xml"),
    ),
    (
        "labels-lazy.xml",
        include_str!("fixtures/rootview_named_inputs/documents/labels-lazy.xml"),
    ),
    (
        "labels-context.xml",
        include_str!("fixtures/rootview_named_inputs/documents/labels-context.xml"),
    ),
    (
        "labels-malformed.xml",
        include_str!("fixtures/rootview_named_inputs/documents/labels-malformed.xml"),
    ),
];
struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_rootview_named_inputs_{language}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
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
                "Retained RootView named-input regression artifacts: {}",
                self.path.display()
            );
        }
    }
}
fn recorded(command: &mut Command, directory: &Path, label: &str) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{label}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = command.isolated_output()?;
    std::fs::write(
        directory.join(format!("{label}-stdout.txt")),
        &output.stdout,
    )?;
    std::fs::write(
        directory.join(format!("{label}-stderr.txt")),
        &output.stderr,
    )?;
    std::fs::write(
        directory.join(format!("{label}-status.json")),
        serde_json::to_vec(
            &json!({"success":output.status.success(),"code":output.status.code()}),
        )?,
    )?;
    Ok(output)
}
fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn physical(xml: &str, schema: &Json) -> Json {
    fn element(node: roxmltree::Node<'_, '_>, schema: &Json) -> Json {
        let mut attributes = node.attributes().map(|a| json!({"namespace":a.namespace().unwrap_or(""),"name":a.name(),"value":a.value()})).collect::<Vec<_>>();
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
                let field = schema["kind"]["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|f| f["name"] == child.tag_name().name() && f["attribute"] != true)
                    .expect("closed target child schema");
                element(child, field)
            })
            .collect::<Vec<_>>();
        let text = if group {
            assert!(
                text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')),
                "element-only group formatting"
            );
            String::new()
        } else {
            text
        };
        json!({"namespace":node.tag_name().namespace().unwrap_or(""),"name":node.tag_name().name(),"attributes":attributes,"text":text,"children":children})
    }
    element(
        roxmltree::Document::parse(xml).unwrap().root_element(),
        schema,
    )
}
fn field<'a>(instance: &'a Instance, name: &str) -> &'a Instance {
    let Instance::Group(fields) = instance else {
        panic!("complete target group")
    };
    &fields
        .iter()
        .find(|(key, _)| key == name)
        .expect("complete target field")
        .1
}
fn explicit_values(instance: &Instance, expected: &Json) {
    for name in ["Code", "Extra", "Message", "Timestamp"] {
        assert_eq!(
            field(instance, name),
            &Instance::Scalar(Value::String(expected[name].as_str().unwrap().into())),
            "exact significant scalar spaces: {name}"
        );
    }
    assert_eq!(
        field(instance, "Derived"),
        &Instance::Scalar(Value::Bool(expected["Derived"].as_bool().unwrap()))
    );
    assert_eq!(
        field(instance, "Revision"),
        &Instance::Scalar(Value::Int(expected["Revision"].as_i64().unwrap()))
    );
}
enum NativeOutcome {
    Input { owner: Json, detail: String },
    Mapping(engine::EngineError),
    Success(Instance),
}
fn native(
    project: &Project,
    case: &Json,
    context: bool,
    directory: &Path,
) -> TestResult<NativeOutcome> {
    let id = case["id"].as_str().unwrap();
    let label = if context { "context" } else { "plain" };
    let original = std::fs::read_to_string(
        directory
            .join("documents")
            .join(case["primary"].as_str().unwrap()),
    )?;
    let source = match format_xml::from_str_with_options(
        &original,
        &project.source,
        &format_xml::XmlReadOptions {
            allow_inactive_root_type_members: true,
            root_view_policy: true,
        },
    ) {
        Ok(source) => source,
        Err(error) => {
            std::fs::write(
                directory.join(format!("native-{id}-{label}-input-error.txt")),
                format!("{error:?}\n{error}\n"),
            )?;
            return Ok(NativeOutcome::Input {
                owner: json!({"kind":"Primary"}),
                detail: error.to_string(),
            });
        }
    };
    let mut snapshots = json!({"primary":snapshot::snapshot(&source),"named":[]});
    std::fs::write(
        directory.join(format!("native-{id}-{label}-inputs.json")),
        serde_json::to_vec_pretty(&snapshots)?,
    )?;
    let mut inputs = Vec::new();
    for (index, input) in project.extra_sources.iter().enumerate() {
        let supplied = case["named"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == input.name)
            .expect("native oracle applies only to complete names");
        let xml = std::fs::read_to_string(
            directory
                .join("documents")
                .join(supplied["file"].as_str().unwrap()),
        )?;
        let parsed = match format_xml::from_str_structured(&xml, &input.schema) {
            Ok(parsed) => parsed,
            Err(error) => {
                std::fs::write(
                    directory.join(format!("native-{id}-{label}-input-error.txt")),
                    format!("{error:?}\n{error}\n"),
                )?;
                return Ok(NativeOutcome::Input {
                    owner: json!({"kind":"Named","index":index,"name":input.name}),
                    detail: error.to_string(),
                });
            }
        };
        snapshots["named"]
            .as_array_mut()
            .unwrap()
            .push(json!({"index":index,"name":input.name,"instance":snapshot::snapshot(&parsed)}));
        std::fs::write(
            directory.join(format!("native-{id}-{label}-inputs.json")),
            serde_json::to_vec_pretty(&snapshots)?,
        )?;
        inputs.push((input.name.clone(), parsed));
    }
    let result = if context {
        let path = Path::new("mapping/library.json");
        let execution =
            engine::ExecutionContext::new(path).with_current_datetime("2026-10-05T00:00:00Z");
        engine::run_with_sources_and_context(project, &source, inputs, &execution)
    } else {
        engine::run_with_sources(project, &source, inputs)
    };
    match result {
        Ok(value) => {
            std::fs::write(
                directory.join(format!("native-{id}-{label}-target.json")),
                serde_json::to_vec_pretty(&snapshot::snapshot(&value))?,
            )?;
            Ok(NativeOutcome::Success(value))
        }
        Err(error) => {
            std::fs::write(
                directory.join(format!("native-{id}-{label}-mapping-error.txt")),
                format!("{error:?}\n{error}\n"),
            )?;
            Ok(NativeOutcome::Mapping(error))
        }
    }
}
fn expected<'a>(case: &'a Json, method: &str) -> &'a Json {
    let e = &case["expected"];
    if e.get("context").is_some() {
        &e[if method.ends_with("-context") {
            "context"
        } else {
            "no_context"
        }]
    } else {
        e
    }
}
fn runtime<'a>(error: &'a Json, name: &str) -> &'a Json {
    error["causes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.get("runtime"))
        .find(|r| r["error"] == name)
        .expect("exact typed runtime cause")
}
fn verify_error(row: &Json, expected: &Json, language: &str) {
    assert!(row["outputs"].is_null(), "no partial result");
    let error = &row["error"];
    assert_eq!(error["kind"], expected["kind"]);
    assert_eq!(error["input"], expected["owner"]);
    assert!(
        error["output"].is_null() && error["request"].is_null(),
        "exclusive input/global failure"
    );
    assert_eq!(error["boundary_identity"], true);
    assert!(error["bytes"].is_null() && error["limit"].is_null());
    if let Some(name) = expected["runtime"].as_str() {
        let typed = runtime(error, name);
        if name == "PrimaryRoot" {
            assert_eq!(
                typed[if language == "rust" { "node" } else { "Node" }],
                expected["node"]
            );
            if language == "rust" {
                let original = typed["primary_root"].as_str().unwrap();
                assert!(
                    original.contains("MissingRequiredField") && original.contains("[\"Extra\"]")
                );
            } else {
                assert_eq!(typed["primary_root"]["error"], "MissingRequiredField");
                assert_eq!(typed["primary_root"]["path"], expected["field"]);
            }
        } else if name == "MissingRuntimeValue" {
            assert_eq!(typed["runtime_value"], "CurrentDateTime");
        } else {
            assert_eq!(
                typed[if language == "rust" { "name" } else { "Detail" }],
                expected["name"]
            );
        }
    }
    if !row["primitive_input_boundary"].is_null() {
        assert_eq!(
            error["original_boundary"], row["primitive_input_boundary"],
            "own-language original parser boundary and every captured cause"
        );
    }
}
fn exercise(language: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language)?;
    let project_text =
        include_str!("../../../codegen/src/tests/fixtures/root_view_static_named_xml_inputs.json");
    let project: Project = serde_json::from_str(project_text)?;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    let policy = lowered
        .xml_boundary
        .as_ref()
        .expect("RootView with static Structured names");
    assert_eq!(
        policy.input.profile(),
        Some(codegen::XmlInputProfile::RootView)
    );
    assert_eq!(policy.extra_inputs.len(), 2);
    assert!(
        policy
            .extra_inputs
            .iter()
            .all(|input| input.input.profile() == Some(codegen::XmlInputProfile::Structured))
    );
    assert!(project.extra_targets.is_empty());
    assert_eq!(
        project
            .extra_sources
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "labels"]
    );
    std::fs::create_dir(directory.path.join("documents"))?;
    for (name, value) in DOCUMENTS {
        std::fs::write(directory.path.join("documents").join(name), value)?;
    }
    let cases_text = include_str!("fixtures/rootview_named_inputs/cases.json");
    let cases: Json = serde_json::from_str(cases_text)?;
    std::fs::write(directory.path.join("cases.json"), cases_text)?;
    let project_path = directory.path.join("project.json");
    std::fs::write(&project_path, project_text)?;
    let generated = directory.path.join("generated");
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut generation = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    generation
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&project_path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        generation.arg("--rust-runtime-path").arg(&runtime_path);
    }
    let generation = recorded(&mut generation, &directory.path, "generate")?;
    assert!(
        generation.status.success(),
        "CLI generation: {}",
        String::from_utf8_lossy(&generation.stderr)
    );
    let unchanged = artifact_files(&generated)?;
    let host = directory.path.join("host");
    std::fs::create_dir(&host)?;
    let records = directory.path.join("actual-records");
    let output = if language == "rust" {
        std::fs::create_dir(host.join("src"))?;
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/rootview_named_inputs_rust.rs.txt"),
        )?;
        let package = "ferrule-rootview-named-inputs-regression-host";
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={package:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime_path:?}}}\nserde_json=\"1\"\n"
            ),
        )?;
        let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
            Some(path) => {
                let path = PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    std::env::current_dir()?.join(path)
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
        let built = recorded(&mut build, &directory.path, "build")?;
        assert!(
            built.status.success(),
            "Rust host build: {}\n{}",
            String::from_utf8_lossy(&built.stdout),
            String::from_utf8_lossy(&built.stderr)
        );
        let binary = target
            .join(format!("debug/{package}"))
            .with_extension(std::env::consts::EXE_EXTENSION);
        let mut command = Command::new(binary);
        command
            .arg(&directory.path)
            .arg(&records)
            .current_dir(&host);
        recorded(&mut command, &directory.path, "host")?
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/rootview_named_inputs_csharp.cs.txt"),
        )?;
        std::fs::write(
            host.join("Host.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../generated/Ferrule.Generated.csproj"/></ItemGroup></Project>"#,
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
        let built = recorded(&mut build, &directory.path, "build")?;
        assert!(
            built.status.success(),
            "C# host build: {}\n{}",
            String::from_utf8_lossy(&built.stdout),
            String::from_utf8_lossy(&built.stderr)
        );
        let mut command = dotnet_command(&directory.path);
        command
            .arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(&directory.path)
            .arg(&records)
            .current_dir(&directory.path)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        recorded(&mut command, &directory.path, "host")?
    };
    assert_eq!(
        artifact_files(&generated)?,
        unchanged,
        "generated library remains unchanged"
    );
    assert!(
        output.status.success(),
        "public host: {}\n{}",
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
        96,
        "12 cases and every eight source-aware methods"
    );
    let mut seen = BTreeSet::new();
    let schema: Json = serde_json::to_value(&project.target)?;
    for case in cases["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mut native_plain = None;
        let mut native_context = None;
        let native_applicable = !id.ends_with("name-before-primary-parse");
        if native_applicable {
            native_plain = Some(native(&project, case, false, &directory.path)?);
            native_context = Some(native(&project, case, true, &directory.path)?);
        } else {
            std::fs::write(
                directory
                    .path
                    .join(format!("native-{id}-inapplicable.json")),
                serde_json::to_vec(
                    &json!({"reason":"complete generated raw-name admission precedes parsing; interpreter accepts parsed instances"}),
                )?,
            )?;
        }
        for method in METHODS {
            let row = rows
                .iter()
                .find(|row| row["case"] == id && row["method"] == method)
                .expect("complete exact row identity");
            assert!(seen.insert((id, method)));
            let original = std::fs::read(
                directory
                    .path
                    .join("documents")
                    .join(case["primary"].as_str().unwrap()),
            )?;
            assert_eq!(unhex(row["primary_hex"].as_str().unwrap()), original);
            let supplied = case["named"].as_array().unwrap();
            assert_eq!(row["named"].as_array().unwrap().len(), supplied.len());
            for (record, item) in row["named"].as_array().unwrap().iter().zip(supplied) {
                assert_eq!(record["name"], item["name"]);
                assert_eq!(
                    unhex(record["document_hex"].as_str().unwrap()),
                    std::fs::read(
                        directory
                            .path
                            .join("documents")
                            .join(item["file"].as_str().unwrap())
                    )?
                );
            }
            let expected = expected(case, method);
            assert_eq!(row["outcome"], expected["outcome"]);
            let native = if method.ends_with("-context") {
                native_context.as_ref()
            } else {
                native_plain.as_ref()
            };
            if row["outcome"] == "success" {
                assert!(row["error"].is_null());
                let NativeOutcome::Success(native) = native.expect("native applicable success")
                else {
                    panic!("native outcome")
                };
                explicit_values(native, expected);
                let native_xml = format_xml::to_string_with_options(
                    &project.target,
                    native,
                    &format_xml::XmlWriteOptions::default(),
                )?;
                std::fs::write(
                    directory.path.join(format!("native-{id}-{method}.xml")),
                    &native_xml,
                )?;
                let native_roundtrip =
                    format_xml::from_str_structured(&native_xml, &project.target)?;
                let outputs = row["outputs"].as_array().unwrap();
                assert_eq!(
                    outputs.len(),
                    1,
                    "single primary and empty named output set"
                );
                let result = &outputs[0];
                assert!(result["name"].is_null());
                let bytes = unhex(result["document_hex"].as_str().unwrap());
                assert_eq!(result["bytes"].as_u64().unwrap(), bytes.len() as u64);
                assert_eq!(
                    std::fs::read(records.join(result["file"].as_str().unwrap()))?,
                    bytes
                );
                let actual_xml = std::str::from_utf8(&bytes)?;
                let parsed = format_xml::from_str_structured(actual_xml, &project.target)?;
                explicit_values(&parsed, expected);
                assert_eq!(
                    snapshot::snapshot(&parsed),
                    snapshot::snapshot(&native_roundtrip),
                    "complete schema-parsed typed target"
                );
                assert_eq!(
                    physical(actual_xml, &schema),
                    physical(&native_xml, &schema),
                    "whole QName/attribute/ordered tree and exact scalar text"
                );
            } else {
                verify_error(row, expected, language);
                if let Some(native) = native {
                    match native {
                        NativeOutcome::Input { owner, detail } => {
                            assert_eq!(expected["kind"], "Input");
                            assert_eq!(&expected["owner"], owner);
                            assert!(!detail.is_empty());
                        }
                        NativeOutcome::Mapping(engine::EngineError::PrimaryRoot {
                            node,
                            source: ir::PrimaryRootError::MissingRequiredField { path },
                        }) => {
                            assert_eq!(expected["runtime"], "PrimaryRoot");
                            assert_eq!(*node, 1);
                            assert_eq!(path, &["Extra".to_owned()]);
                        }
                        NativeOutcome::Mapping(engine::EngineError::MissingRuntimeValue(value)) => {
                            assert_eq!(expected["runtime"], "MissingRuntimeValue");
                            assert_eq!(format!("{value:?}"), "CurrentDateTime");
                        }
                        other => panic!(
                            "unexpected native outcome: {}",
                            match other {
                                NativeOutcome::Success(_) => "success".to_owned(),
                                NativeOutcome::Input { detail, .. } => detail.clone(),
                                NativeOutcome::Mapping(error) => format!("{error:?}"),
                            }
                        ),
                    }
                }
            }
        }
    }
    assert_eq!(seen.len(), 96);
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_rust_rootview_with_static_named_xml_preserves_observed_and_lazy_context()
-> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_rootview_with_static_named_xml_preserves_observed_and_lazy_context()
-> TestResult<()> {
    exercise("csharp")
}
