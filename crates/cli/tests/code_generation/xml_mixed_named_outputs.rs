use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const DATETIME: &str = "2026-01-01T00:00:00Z";
const MULTIPLE_LIST_CASES: [&str; 7] = [
    "ordered",
    "empty",
    "empty-first",
    "empty-second",
    "late-second-path",
    "second-member-writer",
    "second-context",
];

const CASES: [&str; 14] = [
    "ordered",
    "empty",
    "static-context",
    "member-context",
    "missing-context",
    "primary-writer",
    "static-writer",
    "member-writer",
    "late-path",
    "late-context",
    "two-writers",
    "count-exact",
    "count-plus",
    "count-before-primary",
];

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("xml_mixed_named_outputs_{language}"))?;
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
        if self.complete
            && std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                != Some(std::ffi::OsStr::new("1"))
        {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained mixed named XML test artifacts: {}",
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
fn source(name: &str) -> Instance {
    let count = match name {
        "empty" => 0,
        "count-exact" => 4094,
        "count-plus" | "count-before-primary" => 4095,
        _ => 2,
    };
    let rows = (0..count)
        .map(|index| {
            Instance::Group(
                vec![
                    (
                        "File".into(),
                        Instance::Scalar(Value::String(
                            if name == "late-path" && index == 1 {
                                ""
                            } else {
                                "  same.xml  "
                            }
                            .into(),
                        )),
                    ),
                    (
                        "Value".into(),
                        Instance::Scalar(Value::String(
                            if matches!(name, "member-writer" | "two-writers") && index == 1 {
                                "bad"
                            } else {
                                "雪 & 😀"
                            }
                            .into(),
                        )),
                    ),
                    (
                        "Amount".into(),
                        Instance::Scalar(Value::Float(if index == 0 { -0.0 } else { 2.5 })),
                    ),
                    (
                        "NeedContext".into(),
                        Instance::Scalar(Value::Bool(name == "member-context" && index == 1)),
                    ),
                ]
                .into(),
            )
        })
        .collect();
    Instance::Group(
        vec![
            (
                "PrimaryText".into(),
                Instance::Scalar(Value::String("雪 & 😀".into())),
            ),
            (
                "PrimaryBad".into(),
                Instance::Scalar(Value::Bool(matches!(
                    name,
                    "primary-writer" | "late-path" | "late-context" | "count-before-primary"
                ))),
            ),
            (
                "StaticBad".into(),
                Instance::Scalar(Value::Bool(matches!(name, "static-writer" | "two-writers"))),
            ),
            (
                "StaticContext".into(),
                Instance::Scalar(Value::Bool(matches!(
                    name,
                    "static-context" | "missing-context" | "late-context"
                ))),
            ),
            ("Row".into(), Instance::Repeated(rows)),
        ]
        .into(),
    )
}
fn multiple_list_source(name: &str) -> Instance {
    let primary = source(if name == "late-second-path" {
        "primary-writer"
    } else {
        "ordered"
    });
    let Instance::Group(fields) = &primary else {
        panic!("primary group")
    };
    let mut fields = fields
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    if matches!(name, "empty" | "empty-first") {
        fields.iter_mut().find(|(key, _)| key == "Row").unwrap().1 = Instance::Repeated(Vec::new());
    }
    let rows = if matches!(name, "empty" | "empty-second") {
        Vec::new()
    } else {
        (0..2)
            .map(|index| {
                Instance::Group(
                    vec![
                        (
                            "File".into(),
                            Instance::Scalar(Value::String(
                                if name == "late-second-path" && index == 1 {
                                    ""
                                } else if index == 0 {
                                    "  same.xml  "
                                } else {
                                    "別/next.xml"
                                }
                                .into(),
                            )),
                        ),
                        (
                            "Value".into(),
                            Instance::Scalar(Value::String(
                                if name == "second-member-writer" && index == 1 {
                                    "bad"
                                } else {
                                    "independent 雪"
                                }
                                .into(),
                            )),
                        ),
                        (
                            "Amount".into(),
                            Instance::Scalar(Value::Float(if index == 0 { -0.0 } else { 2.5 })),
                        ),
                        (
                            "NeedContext".into(),
                            Instance::Scalar(Value::Bool(name == "second-context" && index == 1)),
                        ),
                    ]
                    .into(),
                )
            })
            .collect()
    };
    fields.push(("SecondRow".into(), Instance::Repeated(rows)));
    Instance::Group(fields.into())
}

fn structured_value_content(instance: &Instance) -> Json {
    fn content(value: &mut Json) {
        match value {
            Json::Object(fields) => {
                if fields.get("kind") == Some(&json!("Group")) {
                    fields.remove("xml_type_origin");
                }
                for field in fields.values_mut() {
                    content(field);
                }
            }
            Json::Array(values) => {
                for value in values {
                    content(value);
                }
            }
            _ => {}
        }
    }
    let mut value = snapshot::snapshot(instance);
    content(&mut value);
    value
}
fn native_original_instance(instance: &Instance) -> Json {
    match instance {
        Instance::Scalar(_) => snapshot::snapshot(instance),
        Instance::Group(fields) => {
            let origin = match fields.xml_type_origin() {
                ir::XmlTypeOrigin::Unknown => json!({"kind":"Unknown"}),
                ir::XmlTypeOrigin::Absent => json!({"kind":"Absent"}),
                ir::XmlTypeOrigin::Explicit(identity) => {
                    json!({"kind":"Explicit","identity":identity})
                }
                ir::XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                } => {
                    json!({"kind":"ExplicitPadded","literal":literal,"resolved_identity":resolved_identity})
                }
            };
            json!({"kind":"Group","xml_type_origin":origin,"fields":fields.iter().map(|(name,value)|
                json!({"name":name,"instance":native_original_instance(value)})).collect::<Vec<_>>()})
        }
        Instance::Repeated(items) => {
            json!({"kind":"Repeated","items":items.iter().map(native_original_instance).collect::<Vec<_>>()})
        }
        Instance::MappedSequence(items) => {
            json!({"kind":"MappedSequence","items":items.iter().map(native_original_instance).collect::<Vec<_>>()})
        }
        Instance::DocumentSet(members) => {
            json!({"kind":"DocumentSet","members":members.iter().map(|member|
            json!({"path":member.path(),"source_path":member.source_path(),"instance":native_original_instance(member.value())})).collect::<Vec<_>>()})
        }
    }
}
fn physical(xml: &str) -> TestResult<Json> {
    fn element(node: roxmltree::Node<'_, '_>) -> Json {
        let mut attributes = node
            .attributes()
            .map(|attribute| {
                (
                    attribute.namespace().unwrap_or(""),
                    attribute.name(),
                    attribute.value(),
                )
            })
            .collect::<Vec<_>>();
        attributes.sort_unstable();
        json!({"name":node.tag_name().name(),"namespace":node.tag_name().namespace().unwrap_or(""),
            "attributes":attributes,"text":if node.children().any(|child| child.is_element()) { None } else { node.text() },
            "children":node.children().filter(|child| child.is_element()).map(element).collect::<Vec<_>>()})
    }
    Ok(element(roxmltree::Document::parse(xml)?.root_element()))
}
fn native_document(
    schema: &SchemaNode,
    value: &Instance,
    options: &mapping::FormatOptions,
) -> Result<String, format_xml::XmlFormatError> {
    format_xml::to_string_with_options(
        schema,
        value,
        &format_xml::XmlWriteOptions {
            schema_hints: options.xml_schema_hints.clone(),
            ..Default::default()
        },
    )
}
fn named_owner(index: usize, name: &str) -> Json {
    json!({"kind":"Named","declaration_index":index,"name":name})
}
fn member_owner(index: usize, name: &str, member: usize, path: &str) -> Json {
    json!({"kind":"Member","declaration_index":index,"name":name,"member_index":member,"path":path})
}
fn native_expected(
    project: &Project,
    source: &Instance,
    path: &Path,
    supplied: bool,
    api: usize,
    directory: &Path,
    stem: &str,
) -> TestResult<Json> {
    let execution = engine::ExecutionContext::new(path);
    let execution = if supplied {
        execution.with_current_datetime(DATETIME)
    } else {
        execution
    };
    let mapped = if api >= 2 {
        engine::run_outputs_with_sources_and_context(project, source, Vec::new(), &execution)
    } else {
        engine::run_outputs(project, source)
    };
    let mapped = match mapped {
        Ok(mapped) => mapped,
        Err(error) => {
            let mut causes = Vec::new();
            let mut cause = std::error::Error::source(&error);
            while let Some(original) = cause {
                causes
                    .push(json!({"debug":format!("{original:?}"),"display":original.to_string()}));
                cause = original.source();
            }
            std::fs::write(
                directory.join(format!("{stem}-native-error-original.json")),
                serde_json::to_vec(
                    &json!({"source":native_original_instance(source),"error_debug":format!("{error:?}"),
                    "error_display":error.to_string(),"source_chain":causes,"api":api,"context_supplied":supplied}),
                )?,
            )?;
            let cause = match &error {
                engine::EngineError::EmptyDynamicTargetPath { node } => {
                    json!({"kind":"EmptyDynamicTargetPath","node":node,"value":null})
                }
                engine::EngineError::MissingRuntimeValue(value) => {
                    json!({"kind":"MissingRuntimeValue","node":null,"value":format!("{value:?}")})
                }
                _ => json!({"kind":"UnexpectedEngineError","debug":format!("{error:?}")}),
            };
            let expected = json!({"outputs":null,"kind":"Mapping","owner":null,"cause":cause,"native_error":format!("{error:?}")});
            std::fs::write(
                directory.join(format!("{stem}-native.json")),
                serde_json::to_vec(&expected)?,
            )?;
            return Ok(expected);
        }
    };
    std::fs::write(
        directory.join(format!("{stem}-native-mapped-original.json")),
        serde_json::to_vec(
            &json!({"source":native_original_instance(source),"primary":native_original_instance(&mapped.primary),
            "extras":mapped.extras.iter().enumerate().map(|(index,output)|
                json!({"declaration_index":index,"name":output.name,"instance":native_original_instance(&output.instance)})).collect::<Vec<_>>(),
            "declared_names":project.extra_targets.iter().map(|target|&target.name).collect::<Vec<_>>(),"api":api,"context_supplied":supplied}),
        )?,
    )?;
    assert_eq!(mapped.extras.len(), project.extra_targets.len());
    let mut actual_count = 1;
    let mut typed = Vec::new();
    for (index, (target, output)) in project.extra_targets.iter().zip(&mapped.extras).enumerate() {
        assert_eq!(output.name, target.name);
        if matches!(target.root.iteration, mapping::ScopeIteration::None) {
            actual_count += 1;
            assert_eq!(
                output.instance.field("Exact"),
                Some(&Instance::Scalar(Value::Int(9_007_199_254_740_993)))
            );
            typed.push(json!({"kind":"SingleDocument","declaration_index":index,"name":output.name,"instance":snapshot::snapshot(&output.instance)}));
        } else {
            let Instance::DocumentSet(members) = &output.instance else {
                panic!("native list")
            };
            actual_count += members.len();
            if let Some(member) = members.first() {
                let Some(Instance::Scalar(Value::Float(value))) = member.value().field("Amount")
                else {
                    panic!("native float")
                };
                assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
            }
            typed.push(json!({"kind":"DocumentList","declaration_index":index,"name":output.name,
                "documents":members.iter().map(|member|json!({"path":member.path(),"instance":snapshot::snapshot(member.value())})).collect::<Vec<_>>()}));
        }
    }
    std::fs::write(
        directory.join(format!("{stem}-native-mapped.json")),
        serde_json::to_vec(
            &json!({"primary":snapshot::snapshot(&mapped.primary),"extras":typed,"actual_artifact_count":actual_count}),
        )?,
    )?;
    if actual_count > 4096 {
        return Ok(json!({"outputs":null,"kind":"Output","owner":null,
            "cause":{"kind":"Resource","resource":"xml_output_artifact_count","observed":actual_count,"limit":4096}}));
    }
    let primary = match native_document(&project.target, &mapped.primary, &project.target_options) {
        Ok(xml) => xml,
        Err(error) => {
            return Ok(
                json!({"outputs":null,"kind":"Output","owner":{"kind":"Primary"},"native_error":format!("{error:?}")}),
            );
        }
    };
    std::fs::write(
        directory.join(format!("{stem}-native-primary.xml")),
        &primary,
    )?;
    let mut extras = Vec::new();
    for (index, (target, output)) in project.extra_targets.iter().zip(&mapped.extras).enumerate() {
        if let Instance::DocumentSet(members) = &output.instance {
            let mut documents = Vec::new();
            for (member_index, member) in members.iter().enumerate() {
                let xml = match native_document(&target.schema, member.value(), &target.options) {
                    Ok(xml) => xml,
                    Err(error) => {
                        return Ok(
                            json!({"outputs":null,"kind":"Output","owner":member_owner(index,&target.name,member_index,member.path()),"native_error":format!("{error:?}")}),
                        );
                    }
                };
                std::fs::write(
                    directory.join(format!("{stem}-native-{index}-{member_index}.xml")),
                    &xml,
                )?;
                documents.push(json!({"path":member.path(),"physical":physical(&xml)?,"instance":snapshot::snapshot(member.value()),"value_content":structured_value_content(member.value())}));
            }
            extras.push(json!({"kind":"DocumentList","declaration_index":index,"name":target.name,"documents":documents}));
        } else {
            let xml = match native_document(&target.schema, &output.instance, &target.options) {
                Ok(xml) => xml,
                Err(error) => {
                    return Ok(
                        json!({"outputs":null,"kind":"Output","owner":named_owner(index,&target.name),"native_error":format!("{error:?}")}),
                    );
                }
            };
            std::fs::write(directory.join(format!("{stem}-native-{index}.xml")), &xml)?;
            extras.push(json!({"kind":"SingleDocument","declaration_index":index,"name":target.name,"physical":physical(&xml)?,"instance":snapshot::snapshot(&output.instance),"value_content":structured_value_content(&output.instance)}));
        }
    }
    Ok(
        json!({"outputs":{"primary":{"physical":physical(&primary)?,"instance":snapshot::snapshot(&mapped.primary),"value_content":structured_value_content(&mapped.primary)},"extras":extras},"kind":null,"owner":null}),
    )
}
fn compare_document(actual: &Json, expected: &Json, schema: &SchemaNode) -> TestResult<()> {
    // recorded() retained both complete host streams before this derived decode.
    let document = if actual["bytes"].is_null() {
        actual["document"]
            .as_str()
            .expect("complete original output text")
            .to_owned()
    } else {
        assert!(
            actual["document"].is_null(),
            "byte originals have no fabricated returned text"
        );
        let bytes = actual["bytes"]
            .as_array()
            .expect("complete original byte buffer")
            .iter()
            .map(|value| u8::try_from(value.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        String::from_utf8(bytes)?
    };
    assert_eq!(physical(&document)?, expected["physical"]);
    assert_eq!(
        structured_value_content(&format_xml::from_str(&document, schema)?),
        expected["value_content"]
    );
    Ok(())
}
fn compare_row(language: &str, row: &Json, expected: &Json, project: &Project) -> TestResult<()> {
    assert_eq!(row["kind"], expected["kind"]);
    assert_eq!(row["owner"], expected["owner"]);
    if expected["outputs"].is_null() {
        assert!(
            row["outputs"].is_null(),
            "failed call exposes no partial result"
        );
        assert_eq!(row["boundary_source_identity"], true);
        assert!(
            row["detail"]
                .as_str()
                .is_some_and(|detail| !detail.is_empty())
        );
        assert!(row["bytes"].is_null() && row["limit"].is_null());
        if let Some(cause) = expected.get("cause") {
            assert_eq!(&row["cause"], cause);
        } else if language == "rust" {
            // The Rust writer's XML character policy retains a source-free boundary error.
            assert_eq!(expected["kind"], "Output");
            assert!(
                expected["native_error"]
                    .as_str()
                    .is_some_and(|error| error.starts_with("InvalidXmlCharacter {"))
            );
            assert_eq!(row["detail"], "XML output must contain XML 1.0 characters");
            assert!(row["cause"].is_null() && row["cause_debug"].is_null());
            assert_eq!(row["cause_type"], "");
            assert_eq!(row["wrapper_source_debug"], row["boundary_debug"]);
        } else {
            assert!(
                row["cause_type"]
                    .as_str()
                    .is_some_and(|name| !name.is_empty())
            );
        }
        return Ok(());
    }
    assert!(row["cause"].is_null());
    compare_document(
        &row["outputs"]["primary"],
        &expected["outputs"]["primary"],
        &project.target,
    )?;
    let actual = row["outputs"]["extras"].as_array().unwrap();
    let wanted = expected["outputs"]["extras"].as_array().unwrap();
    assert_eq!(actual.len(), project.extra_targets.len());
    for (index, ((actual, wanted), target)) in actual
        .iter()
        .zip(wanted)
        .zip(&project.extra_targets)
        .enumerate()
    {
        assert_eq!(actual["kind"], wanted["kind"]);
        assert_eq!(actual["declaration_index"], index);
        assert_eq!(actual["name"], target.name);
        if actual["kind"] == "SingleDocument" {
            assert!(actual.get("path").is_none() && actual.get("member_index").is_none());
            compare_document(actual, wanted, &target.schema)?;
        } else {
            let documents = actual["documents"].as_array().unwrap();
            let wanted = wanted["documents"].as_array().unwrap();
            assert_eq!(documents.len(), wanted.len());
            for (document, wanted) in documents.iter().zip(wanted) {
                assert_eq!(document["path"], wanted["path"]);
                compare_document(document, wanted, &target.schema)?;
            }
        }
    }
    Ok(())
}
fn file_identity(path: &Path) -> io::Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = std::io::Read::read(&mut file, &mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((bytes, format!("{:x}", digest.finalize())))
}
fn selected_rust_executable(
    stdout: &[u8],
    host: &Path,
    package: &str,
) -> TestResult<(PathBuf, Json)> {
    let manifest = host.join("Cargo.toml").canonicalize()?;
    let mut selected = Vec::new();
    for line in std::str::from_utf8(stdout)?.lines() {
        let record: Json = serde_json::from_str(line)?;
        if record["reason"] == "compiler-artifact"
            && record["target"]["name"] == package
            && record["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            && record["manifest_path"].as_str().map(Path::new) == Some(manifest.as_path())
            && record["executable"].is_string()
        {
            selected.push(record);
        }
    }
    assert_eq!(selected.len(), 1, "one exact Cargo-selected host artifact");
    let record = selected.pop().unwrap();
    Ok((
        PathBuf::from(record["executable"].as_str().unwrap()),
        record,
    ))
}
fn run_host(
    language: &str,
    directory: &Path,
    generated: &Path,
    runtime: &Path,
    ir: &Path,
) -> TestResult<Output> {
    let host = directory.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    if language == "rust" {
        let package = "ferrule-xml-mixed-named-output-regression-host";
        let format_xml = runtime
            .parent()
            .unwrap()
            .join("format-xml")
            .canonicalize()?;
        std::fs::write(host.join("src/main.rs"), RUST_HOST)?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={package:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={ir:?}}}\nformat-xml={{path={format_xml:?}}}\nserde_json=\"1\"\n"
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
            None => directory.join("rust-target"),
        };
        let mut build = Command::new("cargo");
        build
            .args(["build", "--message-format=json", "--jobs", "1"])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings");
        let output = recorded(&mut build, directory, "build")?;
        assert!(
            output.status.success(),
            "Rust build: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let (selected, record) = selected_rust_executable(&output.stdout, &host, package)?;
        std::fs::write(
            directory.join("selected-rust-compiler-artifact.json"),
            serde_json::to_vec(&record)?,
        )?;
        let executable = if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let retained = directory
                .join("selected-rust-host")
                .with_extension(std::env::consts::EXE_EXTENSION);
            std::fs::copy(&selected, &retained)?;
            let original = file_identity(&selected)?;
            assert_eq!(
                original,
                file_identity(&retained)?,
                "complete selected ELF retention"
            );
            std::fs::write(
                directory.join("selected-rust-host-retention.json"),
                serde_json::to_vec(
                    &json!({"compiler_selected":selected,"retained":retained,"bytes":original.0,"sha256":original.1}),
                )?,
            )?;
            retained
        } else {
            selected
        };
        let mut run = Command::new(executable);
        run.arg(directory).current_dir(&host);
        Ok(recorded(&mut run, directory, "host")?)
    } else {
        std::fs::write(host.join("Program.cs"), CSHARP_HOST)?;
        std::fs::write(
            host.join("Host.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
        )?;
        let artifacts = directory.join("artifacts");
        let mut build = dotnet_command(directory);
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
            .current_dir(directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let output = recorded(&mut build, directory, "build")?;
        assert!(
            output.status.success(),
            "C# build: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let mut run = dotnet_command(directory);
        run.arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(directory)
            .current_dir(directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        Ok(recorded(&mut run, directory, "host")?)
    }
}
fn exercise(language: &str, multiple_lists: bool) -> TestResult<()> {
    let label = if multiple_lists {
        format!("{language}_multiple_lists")
    } else {
        language.to_owned()
    };
    let mut directory = RegressionDirectory::new(&label)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let ir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ir")
        .canonicalize()?;
    for reverse in [false, true] {
        let variant = directory.path.join(if reverse {
            "dynamic-first"
        } else {
            "static-first"
        });
        std::fs::create_dir(&variant)?;
        let mut project: Project = serde_json::from_str(if multiple_lists {
            include_str!(
                "../../../codegen/src/tests/fixtures/static_primary_mixed_multiple_named_xml_documents.json"
            )
        } else {
            include_str!(
                "../../../codegen/src/tests/fixtures/static_primary_mixed_named_xml_documents.json"
            )
        })?;
        if reverse {
            project.extra_targets.reverse();
        }
        assert!(engine::validate(&project).is_empty());
        assert_eq!(
            codegen::lower(&project)?.xml_output_mode()?,
            Some(codegen::XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs)
        );
        let path = variant.join("project.json");
        std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
        let mut cases = Vec::new();
        let mut expected = BTreeMap::new();
        let names: &[&str] = if multiple_lists {
            &MULTIPLE_LIST_CASES
        } else {
            &CASES
        };
        for &name in names {
            let constructed = if multiple_lists {
                multiple_list_source(name)
            } else {
                source(name)
            };
            std::fs::write(
                variant.join(format!("{name}-constructed-input-original.json")),
                serde_json::to_vec(&native_original_instance(&constructed))?,
            )?;
            let xml = format_xml::to_string(&project.source, &constructed)?;
            std::fs::write(variant.join(format!("{name}.xml")), &xml)?;
            let source = format_xml::from_str(&xml, &project.source)?;
            std::fs::write(
                variant.join(format!("{name}-complete-input-typed.json")),
                serde_json::to_vec(
                    &json!({"constructed":native_original_instance(&constructed),"parsed":native_original_instance(&source)}),
                )?,
            )?;
            assert_eq!(
                structured_value_content(&source),
                structured_value_content(&constructed)
            );
            let supplied = matches!(name, "static-context" | "member-context" | "second-context");
            cases.push(json!({"name":name,"context":supplied}));
            for api in 0..4 {
                let original = native_expected(
                    &project,
                    &source,
                    &path,
                    supplied,
                    api,
                    &variant,
                    &format!("{name}-{api}"),
                )?;
                std::fs::write(
                    variant.join(format!("{name}-{api}-native-result-original.json")),
                    serde_json::to_vec(&original)?,
                )?;
                expected.insert((name.to_owned(), api), original);
            }
        }
        std::fs::write(variant.join("cases.json"), serde_json::to_vec(&cases)?)?;
        std::fs::write(
            variant.join("native-expected.json"),
            serde_json::to_vec(
                &expected
                    .iter()
                    .map(|((case, api), row)| json!({"case":case,"api":api,"row":row}))
                    .collect::<Vec<_>>(),
            )?,
        )?;
        let generated = variant.join("generated");
        let mut generate = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        generate
            .args(["--diagnostics", "json", "generate", "--project"])
            .arg(&path)
            .args(["--language", language, "--out"])
            .arg(&generated);
        if language == "rust" {
            generate.arg("--rust-runtime-path").arg(&runtime);
        }
        let output = recorded(&mut generate, &variant, "generate")?;
        assert!(
            output.status.success(),
            "generation: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let unchanged = artifact_files(&generated)?;
        assert_eq!(unchanged.len(), if language == "rust" { 2 } else { 75 });
        let output = run_host(language, &variant, &generated, &runtime, &ir)?;
        assert_eq!(
            artifact_files(&generated)?,
            unchanged,
            "host build leaves complete generated tree exact"
        );
        assert!(
            output.status.success(),
            "public host: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let originals = std::str::from_utf8(&output.stdout)?
            .lines()
            .map(serde_json::from_str::<Json>)
            .collect::<Result<Vec<_>, _>>()?;
        let diagnostics = std::str::from_utf8(&output.stderr)?
            .lines()
            .map(serde_json::from_str::<Json>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(originals.len(), names.len() * 4);
        assert_eq!(diagnostics.len(), names.len() * 4);
        let mut identities = BTreeSet::new();
        for (original, diagnostic) in originals.iter().zip(diagnostics) {
            let mut reconstructed = diagnostic;
            assert_eq!(
                reconstructed.as_object_mut().unwrap().remove("diagnostic"),
                Some(json!("post-flush-complete-row"))
            );
            assert_eq!(
                &reconstructed, original,
                "separate post-flush diagnostic retains every original field/buffer"
            );
            let name = original["case"].as_str().unwrap().to_owned();
            let api = original["api"].as_u64().unwrap() as usize;
            assert!(identities.insert((name.clone(), api)));
            compare_row(language, original, &expected[&(name, api)], &project)?;
        }
        assert_eq!(identities, expected.keys().cloned().collect());
    }
    directory.complete = true;
    Ok(())
}
#[test]
fn rust_mixed_named_xml_public_apis_preserve_cardinality_owners_and_real_counts() -> TestResult<()>
{
    exercise("rust", false)
}
#[test]
fn csharp_mixed_named_xml_public_apis_preserve_cardinality_owners_and_real_counts() -> TestResult<()>
{
    exercise("csharp", false)
}

#[test]
fn rust_mixed_xml_multiple_lists_preserve_independent_policies_and_late_failures() -> TestResult<()>
{
    exercise("rust", true)
}
#[test]
fn csharp_mixed_xml_multiple_lists_preserve_independent_policies_and_late_failures()
-> TestResult<()> {
    exercise("csharp", true)
}

const RUST_HOST: &str = r#"use ferrule_generated_mapping::*;
use serde_json::{Value as Json,json};
use std::{error::Error,io::Write,path::Path};
fn document(text:Option<String>,bytes:Option<Vec<u8>>) -> Json { json!({"document":text,"bytes":bytes}) }
fn text(outputs:XmlMixedExecutionOutputs) -> Json {
    let extras=outputs.extras.into_iter().map(|extra|match extra {
        NamedXmlMixedOutput::SingleDocument{declaration_index,name,document:xml}=>{
            let mut value=document(Some(xml),None); let object=value.as_object_mut().unwrap();
            object.insert("kind".into(),json!("SingleDocument"));object.insert("declaration_index".into(),json!(declaration_index));object.insert("name".into(),json!(name));value },
        NamedXmlMixedOutput::DocumentList{declaration_index,name,documents}=>json!({"kind":"DocumentList","declaration_index":declaration_index,"name":name,"documents":documents.into_iter().map(|member|{
            let mut value=document(Some(member.document),None);value["path"]=json!(member.path);value }).collect::<Vec<_>>()})
    }).collect::<Vec<_>>();json!({"primary":document(Some(outputs.primary),None),"extras":extras})
}
fn bytes(outputs:XmlMixedBytesExecutionOutputs) -> Json {
    let extras=outputs.extras.into_iter().map(|extra|match extra {
        NamedXmlMixedBytesOutput::SingleDocument{declaration_index,name,document:xml}=>{
            let mut value=document(None,Some(xml));let object=value.as_object_mut().unwrap();
            object.insert("kind".into(),json!("SingleDocument"));object.insert("declaration_index".into(),json!(declaration_index));object.insert("name".into(),json!(name));value },
        NamedXmlMixedBytesOutput::DocumentList{declaration_index,name,documents}=>json!({"kind":"DocumentList","declaration_index":declaration_index,"name":name,"documents":documents.into_iter().map(|member|{
            let mut value=document(None,Some(member.document));value["path"]=json!(member.path);value }).collect::<Vec<_>>()})
    }).collect::<Vec<_>>();json!({"primary":document(None,Some(outputs.primary)),"extras":extras})
}
fn error(error:XmlMixedExecutionError)->Json {
    let original=error.boundary.as_ref();
    let wrapper_source=error.source();
    let identity=wrapper_source.and_then(|source|source.downcast_ref::<codegen_runtime::XmlBoundaryError>())
        .is_some_and(|source|std::ptr::eq(source,original));
    let owner=match &error.owner { None=>Json::Null,Some(XmlMixedOutputOwner::Primary)=>json!({"kind":"Primary"}),
        Some(XmlMixedOutputOwner::Named{declaration_index,name})=>json!({"kind":"Named","declaration_index":declaration_index,"name":name}),
        Some(XmlMixedOutputOwner::Member{declaration_index,name,member_index,path})=>json!({"kind":"Member","declaration_index":declaration_index,"name":name,"member_index":member_index,"path":path}) };
    let source=original.source();
    let (cause,cause_type)=if let Some(runtime)=source.and_then(|source|source.downcast_ref::<codegen_runtime::RuntimeError>()) {
        let cause=match runtime {codegen_runtime::RuntimeError::EmptyDynamicTargetPath{node}=>json!({"kind":"EmptyDynamicTargetPath","node":node,"value":null}),
            codegen_runtime::RuntimeError::MissingRuntimeValue{value}=>json!({"kind":"MissingRuntimeValue","node":null,"value":format!("{value:?}")}),
            _=>json!({"kind":"UnexpectedRuntimeError","debug":format!("{runtime:?}")})};(cause,"RuntimeError")
    } else if let Some(resource)=source.and_then(|source|source.downcast_ref::<codegen_runtime::XmlOutputSetResourceError>()) {
        (json!({"kind":"Resource","resource":resource.resource,"observed":resource.observed_count,"limit":resource.limit}),"XmlOutputSetResourceError")
    } else if source.and_then(|source|source.downcast_ref::<format_xml::XmlFormatError>()).is_some() {
        (Json::Null,"format_xml::XmlFormatError")
    } else { (Json::Null,"") };
    json!({"outputs":null,"owner":owner,"kind":format!("{:?}",original.kind),"detail":original.detail,"bytes":original.bytes,"limit":original.limit,
        "cause":cause,"cause_type":cause_type,"cause_debug":source.map(|source|format!("{source:?}")),"boundary_debug":format!("{original:?}"),"wrapper_debug":format!("{error:?}"),
        "wrapper_source_debug":wrapper_source.map(|source|format!("{source:?}")),"boundary_source_identity":identity})
}
fn main()->Result<(),Box<dyn Error>> {
    let root=std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let cases:Json=serde_json::from_slice(&std::fs::read(root.join("cases.json"))?)?;
    for case in cases.as_array().unwrap() {
        let name=case["name"].as_str().unwrap();let xml=std::fs::read_to_string(root.join(format!("{name}.xml")))?;
        let path=root.join("project.json");let execution=codegen_runtime::ExecutionContext::new(Path::new(&path));
        let execution=if case["context"]==true {execution.with_current_datetime("2026-01-01T00:00:00Z")}else{execution};
        for api in 0..4 {
            let result=match api {0=>execute_xml_mixed_outputs(&xml).map(text),1=>execute_xml_bytes_mixed_outputs(xml.as_bytes()).map(bytes),
                2=>execute_xml_mixed_outputs_with_context(&xml,&execution).map(text),_=>execute_xml_bytes_mixed_outputs_with_context(xml.as_bytes(),&execution).map(bytes)};
            let mut row=match result {Ok(outputs)=>json!({"outputs":outputs,"owner":null,"kind":null,"cause":null}),Err(value)=>error(value)};
            row["case"]=json!(name);row["api"]=json!(api);println!("{row}");std::io::stdout().flush()?;
            row["diagnostic"]=json!("post-flush-complete-row");eprintln!("{row}");std::io::stderr().flush()?;
        }
    }
    Ok(())
}
"#;

const CSHARP_HOST: &str = r#"using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;
static class Program
{
    static readonly UTF8Encoding Utf8=new(false,true);
    static JsonObject Document(string? text,byte[]? bytes=null)=>new(){["document"]=text,["bytes"]=bytes is null?null:new JsonArray(bytes.Select(value=>(JsonNode?)JsonValue.Create((int)value)).ToArray())};
    static JsonObject Text(XmlMixedExecutionOutputs outputs)
    {
        var extras=new JsonArray();
        foreach(var extra in outputs.Extras)
        {
            if(extra is NamedXmlMixedOutput.SingleDocument single)
            {var value=Document(single.Document);value["kind"]="SingleDocument";value["declaration_index"]=single.DeclarationIndex;value["name"]=single.Name;extras.Add(value);}
            else if(extra is NamedXmlMixedOutput.DocumentList list)
            {var documents=new JsonArray();foreach(var member in list.Documents){var value=Document(member.Document);value["path"]=member.Path;documents.Add(value);}extras.Add(new JsonObject{["kind"]="DocumentList",["declaration_index"]=list.DeclarationIndex,["name"]=list.Name,["documents"]=documents});}
            else extras.Add(new JsonObject{["kind"]="UnexpectedResult",["type"]=extra?.GetType().FullName,["debug"]=extra?.ToString()});
        }
        return new(){["primary"]=Document(outputs.Primary),["extras"]=extras};
    }
    static JsonObject Bytes(XmlMixedBytesExecutionOutputs outputs)
    {
        var extras=new JsonArray();
        foreach(var extra in outputs.Extras)
        {
            if(extra is NamedXmlMixedBytesOutput.SingleDocument single)
            {var value=Document(null,single.Document);value["kind"]="SingleDocument";value["declaration_index"]=single.DeclarationIndex;value["name"]=single.Name;extras.Add(value);}
            else if(extra is NamedXmlMixedBytesOutput.DocumentList list)
            {var documents=new JsonArray();foreach(var member in list.Documents){var value=Document(null,member.Document);value["path"]=member.Path;documents.Add(value);}extras.Add(new JsonObject{["kind"]="DocumentList",["declaration_index"]=list.DeclarationIndex,["name"]=list.Name,["documents"]=documents});}
            else extras.Add(new JsonObject{["kind"]="UnexpectedResult",["type"]=extra?.GetType().FullName,["debug"]=extra?.ToString()});
        }
        return new(){["primary"]=Document(null,outputs.Primary),["extras"]=extras};
    }
    static JsonObject Error(XmlMixedExecutionException error)
    {
        var identity=ReferenceEquals(error.Boundary,error.InnerException);
        JsonObject? owner=error.Owner switch{
            null=>null,XmlMixedOutputOwner.Primary=>new(){["kind"]="Primary"},
            XmlMixedOutputOwner.Named named=>new(){["kind"]="Named",["declaration_index"]=named.DeclarationIndex,["name"]=named.Name},
            XmlMixedOutputOwner.Member member=>new(){["kind"]="Member",["declaration_index"]=member.DeclarationIndex,["name"]=member.Name,["member_index"]=member.MemberIndex,["path"]=member.Path},
            XmlMixedOutputOwner unexpected=>new(){["kind"]="UnexpectedOwner",["type"]=unexpected.GetType().FullName,["debug"]=unexpected.ToString()}};
        JsonObject? cause=null;
        JsonObject? runtimePayload=null;
        if(error.Boundary.InnerException is FerruleRuntimeException runtime)
        {
            cause=new(){["kind"]=runtime.Error.ToString(),["node"]=runtime.Node is uint node?JsonValue.Create(node):null,["value"]=runtime.RuntimeValue?.ToString()};
            runtimePayload=new(){["error"]=runtime.Error.ToString(),["node"]=runtime.Node,["function"]=runtime.Function,
                ["expected_arity"]=runtime.ExpectedArity,["actual_arity"]=runtime.ActualArity,["found_kind"]=runtime.FoundKind?.ToString(),
                ["aggregate_operation"]=runtime.AggregateOperation?.ToString(),["detail"]=runtime.Detail,
                ["requested_items_decimal"]=runtime.RequestedItems?.ToString(),["maximum_items_decimal"]=runtime.MaximumItems?.ToString(),
                ["maximum_depth"]=runtime.MaximumDepth,["runtime_value"]=runtime.RuntimeValue?.ToString(),["failure_rule"]=runtime.FailureRule,
                ["mapping_failure_message"]=runtime.MappingFailureMessage,["join"]=runtime.Join,["user_function"]=runtime.UserFunction,
                ["function_parameter"]=runtime.FunctionParameter,["expected_scalar_type"]=runtime.ExpectedScalarType?.ToString(),
                ["runtime_parameter"]=runtime.RuntimeParameter,["source_field"]=runtime.SourceField,["found_instance"]=runtime.FoundInstance,
                ["primary_root"]=runtime.PrimaryRoot is FerrulePrimaryRootFailure primary?new JsonObject{["error"]=primary.Error.ToString(),
                    ["path"]=new JsonArray(primary.Path.Select(part=>(JsonNode?)JsonValue.Create(part)).ToArray()),["found"]=primary.Found}:null};
        }
        else if(error.Boundary.InnerException is FerruleXmlOutputSetResourceException resource)
            cause=new(){["kind"]="Resource",["resource"]=resource.Resource,["observed"]=resource.ObservedCount,["limit"]=resource.Limit};
        return new(){["outputs"]=null,["owner"]=owner,["kind"]=error.Boundary.Kind.ToString(),["detail"]=error.Boundary.Detail,
            ["bytes"]=error.Boundary.Bytes,["limit"]=error.Boundary.Limit,["cause"]=cause,["cause_type"]=error.Boundary.InnerException?.GetType().FullName,
            ["cause_debug"]=error.Boundary.InnerException?.ToString(),["runtime_payload"]=runtimePayload,["boundary_debug"]=error.Boundary.ToString(),
            ["wrapper_debug"]=error.ToString(),["wrapper_inner_type"]=error.InnerException?.GetType().FullName,
            ["wrapper_inner_debug"]=error.InnerException?.ToString(),["boundary_source_identity"]=identity};
    }
    static void Main(string[] args)
    {
        var root=args.Single();var cases=JsonNode.Parse(File.ReadAllText(Path.Combine(root,"cases.json")))!.AsArray();
        foreach(var item in cases)
        {
            var value=item!.AsObject();var name=value["name"]!.GetValue<string>();var xml=File.ReadAllText(Path.Combine(root,name+".xml"));
            var path=Path.Combine(root,"project.json");var execution=value["context"]!.GetValue<bool>()?new FerruleExecutionContext(path,path,"2026-01-01T00:00:00Z"):new FerruleExecutionContext(path);
            for(var api=0;api<4;api++)
            {
                JsonObject row;
                try {var outputs=api switch{0=>Text(GeneratedMapping.ExecuteXmlMixedOutputs(xml)),1=>Bytes(GeneratedMapping.ExecuteXmlBytesMixedOutputs(Utf8.GetBytes(xml))),
                    2=>Text(GeneratedMapping.ExecuteXmlMixedOutputs(xml,execution)),_=>Bytes(GeneratedMapping.ExecuteXmlBytesMixedOutputs(Utf8.GetBytes(xml),execution))};
                    row=new(){["outputs"]=outputs,["owner"]=null,["kind"]=null,["cause"]=null};}
                catch(XmlMixedExecutionException error){row=Error(error);}
                row["case"]=name;row["api"]=api;Console.Out.WriteLine(row.ToJsonString());Console.Out.Flush();
                row["diagnostic"]="post-flush-complete-row";Console.Error.WriteLine(row.ToJsonString());Console.Error.Flush();
            }
        }
    }
}
"#;

#[path = "xml_mixed_named_outputs/late_mapping_count.rs"]
mod late_mapping_count;
