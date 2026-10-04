use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

use super::structured_xml_snapshot as snapshot;

const CURRENT: &str = "2026-10-04T00:00:00Z";
const NOMINAL: &str = include_str!("fixtures/xml_output_sets/nominal.xml");
const VARIANTS: [&str; 7] = [
    "base",
    "ordered-policies",
    "reversed-policies",
    "named-output",
    "context",
    "single",
    "phase-order",
];
const PUBLIC_APIS: [&str; 8] = [
    "xml-outputs",
    "xml-outputs-context",
    "bytes-outputs",
    "bytes-outputs-context",
    "xml",
    "xml-context",
    "bytes",
    "bytes-context",
];
type Inputs = Vec<(&'static str, String)>;

// Keep source, generated libraries, host commands and raw results on every failure.
struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("xml_output_sets_{language}"))?;
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
                "Retained XML output-set test artifacts: {}",
                self.path.display()
            );
        }
    }
}

fn mapping(variant: &str) -> TestResult<Project> {
    let mut value: Json =
        serde_json::from_str(include_str!("fixtures/xml_output_sets/project.json"))?;
    match variant {
        "base" => {}
        "single" => value["extra_targets"] = json!([]),
        "named-output" => {
            value["extra_targets"][0]["schema"]["kind"]["children"][2]["kind"]["ty"] = json!("int")
        }
        "phase-order" => value["target"]["kind"]["children"][1]["kind"]["ty"] = json!("int"),
        "context" => {
            value["graph"]["nodes"]["10"] =
                json!({"kind":"runtime_value","value":"current_date_time"});
            value["extra_targets"][0]["schema"]["kind"]["children"].as_array_mut().expect("Audit fields")
                .push(json!({"name":"RunTime","repeating":false,"xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"string"}}));
            value["extra_targets"][0]["root"]["bindings"]
                .as_array_mut()
                .expect("Audit bindings")
                .push(json!({"target_field":"RunTime","node":10}));
        }
        "ordered-policies" | "reversed-policies" => {
            value["target_options"]["xml_schema_hints"] =
                json!({"no_namespace_location":"invoice.xsd"});
            let audit = &mut value["extra_targets"][0];
            audit["schema"]["xml_namespace"] = json!({"kind":"qualified","uri":"urn:audit"});
            for field in audit["schema"]["kind"]["children"]
                .as_array_mut()
                .expect("Audit fields")
            {
                field["xml_namespace"] = json!({"kind":"qualified","uri":"urn:audit"});
            }
            audit["options"]["xml_schema_hints"] =
                json!({"locations":[{"namespace":"urn:audit","location":"audit.xsd"}]});
            value["graph"]["nodes"]["11"] = json!({"kind":"const","value":"receipt"});
            value["extra_targets"].as_array_mut().expect("targets").push(json!({
                "name":"Receipt", "path":"receipt.xml",
                "schema":{"name":"Receipt","repeating":false,"xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[
                    {"name":"Label","repeating":false,"xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"string"}},
                    {"name":"Count","repeating":false,"xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}
                ]}},
                "options":{"xml_document":true,"xml_schema_hints":{"no_namespace_location":"receipt.xsd"}},
                "root":{"target_field":"Receipt","bindings":[{"target_field":"Label","node":11},{"target_field":"Count","node":4}],"children":[]}
            }));
            if variant == "reversed-policies" {
                value["extra_targets"]
                    .as_array_mut()
                    .expect("targets")
                    .reverse();
            }
        }
        other => panic!("unknown variant {other}"),
    }
    Ok(serde_json::from_value(value)?)
}

fn inputs(variant: &str) -> Inputs {
    match variant {
        "base" => vec![
            ("nominal", NOMINAL.into()),
            (
                "empty",
                "<Batch BatchId=\"B-17\"><AuditFactor>1</AuditFactor></Batch>".into(),
            ),
            (
                "unicode",
                NOMINAL
                    .replace("B-17", "B😀&amp;17")
                    .replace("A-1", "A😀&amp;1"),
            ),
            (
                "bad-number",
                NOMINAL.replace("<Amount>2.5</Amount>", "<Amount>two</Amount>"),
            ),
            (
                "missing-factor",
                NOMINAL.replace("<AuditFactor>1</AuditFactor>", ""),
            ),
            ("invalid-utf8", String::new()),
        ],
        "phase-order" => vec![
            ("nominal", NOMINAL.into()),
            (
                "missing-factor",
                NOMINAL.replace("<AuditFactor>1</AuditFactor>", ""),
            ),
        ],
        _ => vec![("nominal", NOMINAL.into())],
    }
}

fn expected_kind(variant: &str, case: &str, with_context: bool) -> &'static str {
    if case == "invalid-utf8" {
        "Utf8"
    } else if case == "bad-number" {
        "Input"
    } else if case == "missing-factor" || (variant == "context" && !with_context) {
        "Mapping"
    } else if matches!(variant, "named-output" | "phase-order") {
        "Output"
    } else {
        "ok"
    }
}

fn attribute(name: &str, namespace: &str, value: &str) -> Json {
    json!({"name":name,"namespace":namespace,"value":value})
}
fn element(
    name: &str,
    namespace: &str,
    text: Option<&str>,
    attributes: Vec<Json>,
    children: Vec<Json>,
) -> Json {
    json!({"name":name,"namespace":namespace,"text":text,"attributes":attributes,"children":children})
}
fn physical(variant: &str, case: &str, name: Option<&str>) -> Json {
    let policies = variant.ends_with("policies");
    let empty = case == "empty";
    let id = if case == "unicode" {
        "B😀&17"
    } else {
        "B-17"
    };
    let total = if empty { "0" } else { "10.25" };
    let count = if empty { "0" } else { "2" };
    let xsi = "http://www.w3.org/2001/XMLSchema-instance";
    match name {
        None => {
            let mut attributes = vec![attribute("BatchId", "", id)];
            if policies {
                attributes.push(attribute("noNamespaceSchemaLocation", xsi, "invoice.xsd"));
            }
            let mut children = vec![element("Total", "", Some(total), vec![], vec![])];
            if !empty {
                for (code, quantity, amount, enabled) in [
                    (
                        if case == "unicode" { "A😀&1" } else { "A-1" },
                        "4",
                        "2.5",
                        "true",
                    ),
                    ("Z-9", "-2", "7.75", "false"),
                ] {
                    children.push(element(
                        "Line",
                        "",
                        None,
                        vec![attribute("Code", "", code)],
                        vec![
                            element("Quantity", "", Some(quantity), vec![], vec![]),
                            element("Amount", "", Some(amount), vec![], vec![]),
                            element("Enabled", "", Some(enabled), vec![], vec![]),
                        ],
                    ));
                }
            }
            element("Invoice", "", None, attributes, children)
        }
        Some("Audit") => {
            let namespace = if policies { "urn:audit" } else { "" };
            let attributes = if policies {
                vec![attribute("schemaLocation", xsi, "urn:audit audit.xsd")]
            } else {
                vec![]
            };
            let mut children = vec![
                element("BatchId", namespace, Some(id), vec![], vec![]),
                element("ItemCount", namespace, Some(count), vec![], vec![]),
                element("Total", namespace, Some(total), vec![], vec![]),
                element("Accepted", namespace, Some("true"), vec![], vec![]),
            ];
            if variant == "context" {
                children.push(element("RunTime", "", Some(CURRENT), vec![], vec![]));
            }
            element("AuditReport", namespace, None, attributes, children)
        }
        Some("Receipt") => element(
            "Receipt",
            "",
            None,
            vec![attribute("noNamespaceSchemaLocation", xsi, "receipt.xsd")],
            vec![
                element("Label", "", Some("receipt"), vec![], vec![]),
                element("Count", "", Some(count), vec![], vec![]),
            ],
        ),
        other => panic!("unexpected target {other:?}"),
    }
}

fn field<'a>(value: &'a Json, name: &str) -> &'a Json {
    &value["fields"]
        .as_array()
        .expect("complete group fields")
        .iter()
        .find(|field| field["name"] == name)
        .expect("declared field")["instance"]
}
fn assert_source_values(value: &Json, case: &str) {
    assert_eq!(value["kind"], "Group");
    assert_eq!(
        field(value, "AuditFactor"),
        &if case == "missing-factor" {
            json!({"kind":"Scalar","tag":"Null"})
        } else {
            json!({"kind":"Scalar","tag":"Float","bits_hex":"3ff0000000000000"})
        }
    );
    let items = field(value, "Item")["items"]
        .as_array()
        .expect("all typed items");
    assert_eq!(items.len(), if case == "empty" { 0 } else { 2 });
    for (index, item) in items.iter().enumerate() {
        assert_eq!(
            field(item, "Quantity"),
            &json!({"kind":"Scalar","tag":"Int","value":if index==0 {4} else {-2}})
        );
        assert_eq!(
            field(item, "Amount"),
            &json!({"kind":"Scalar","tag":"Float","bits_hex":if index==0 {"4004000000000000"} else {"401f000000000000"}})
        );
        assert_eq!(
            field(item, "Enabled"),
            &json!({"kind":"Scalar","tag":"Bool","value":index==0})
        );
    }
}
fn assert_mapped_values(result: &Json, case: &str) {
    let total = json!({"kind":"Scalar","tag":"Float","bits_hex":if case=="empty" {"0000000000000000"} else {"4024800000000000"}});
    assert_eq!(field(&result["primary"], "Total"), &total);
    for target in result["extras"].as_array().expect("all mapped extras") {
        if target["name"] == "Audit" {
            assert_eq!(
                field(&target["instance"], "ItemCount"),
                &json!({"kind":"Scalar","tag":"Int","value":if case=="empty" {0} else {2}})
            );
            assert_eq!(field(&target["instance"], "Total"), &total);
        }
    }
}

fn assert_rows(
    output: &Output,
    variant: &str,
    project: &Project,
    project_path: &Path,
    inputs: &[(&str, String)],
) -> TestResult<()> {
    assert!(
        output.status.success(),
        "host stdout:{}\nstderr:{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "unexpected host stderr");
    let rows = String::from_utf8(output.stdout.clone())?
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    let context = engine::ExecutionContext::new(project_path).with_current_datetime(CURRENT);
    for row in &rows {
        let case = row["case"].as_str().expect("case");
        let api = row["api"].as_str().expect("API");
        assert!(
            seen.insert((case.to_owned(), api.to_owned())),
            "duplicate {case}/{api}"
        );
        let xml = &inputs
            .iter()
            .find(|(name, _)| *name == case)
            .expect("known case")
            .1;
        let with_context = api.ends_with("context");
        let result = &row["result"];
        if PUBLIC_APIS.contains(&api) {
            let kind = expected_kind(variant, case, with_context);
            assert_eq!(result["kind"], kind, "{variant}/{case}/{api}: {result}");
            if kind == "ok" {
                assert_eq!(result["primary"]["physical"], physical(variant, case, None));
                assert!(
                    result["primary"]["xml"]
                        .as_str()
                        .expect("complete primary XML")
                        .starts_with("<?xml ")
                );
                if api.contains("outputs") {
                    let extras = result["extras"].as_array().expect("ordered extras");
                    assert_eq!(extras.len(), project.extra_targets.len());
                    for (index, (actual, target)) in
                        extras.iter().zip(&project.extra_targets).enumerate()
                    {
                        assert_eq!(actual["name"], target.name, "target index {index}");
                        assert_eq!(
                            actual["physical"],
                            physical(variant, case, Some(&target.name)),
                            "target index {index}"
                        );
                    }
                } else {
                    assert!(
                        result.get("extras").is_none(),
                        "old API returns one document"
                    );
                }
            } else {
                assert_eq!(result["no_output"], true);
                for field in ["primary", "extras", "xml"] {
                    assert!(result.get(field).is_none(), "no partial {field}");
                }
                assert!(
                    result["detail"]
                        .as_str()
                        .is_some_and(|detail| !detail.is_empty())
                );
                if api.contains("outputs") {
                    let target = if kind == "Output" {
                        if variant == "named-output" {
                            json!({"kind":"Named","index":0,"name":"Audit"})
                        } else {
                            json!({"kind":"Primary","index":null,"name":null})
                        }
                    } else {
                        Json::Null
                    };
                    assert_eq!(result["target"], target);
                    assert_eq!(result["boundary_preserved"], true);
                }
                if kind == "Output" {
                    assert!(
                        result["inner_sources"]
                            .as_array()
                            .is_some_and(|causes| !causes.is_empty())
                    );
                    if result.get("writer_error").is_some() {
                        assert_eq!(result["writer_error"]["type"], "XmlFormatError");
                    } else {
                        assert_eq!(result["runtime_error"], "XmlSerialization");
                    }
                }
                if case == "missing-factor" {
                    assert_eq!(result["function"], "multiply");
                    assert_eq!(
                        result["found"]
                            .as_str()
                            .expect("typed Null")
                            .to_ascii_lowercase(),
                        "null"
                    );
                }
                if variant == "context" && !with_context {
                    assert_eq!(result["runtime_value"], "CurrentDateTime");
                }
            }
        } else if matches!(api, "source" | "source-bytes") {
            let native = if case == "invalid-utf8" {
                None
            } else {
                format_xml::from_str(xml, &project.source).ok()
            };
            if let Some(native) = native {
                assert_eq!(result["kind"], "ok");
                assert_eq!(result["snapshot"], snapshot::snapshot(&native));
                assert_source_values(&result["snapshot"], case);
            } else {
                assert_eq!(
                    result["kind"],
                    if case == "invalid-utf8" {
                        "Utf8"
                    } else {
                        "Input"
                    }
                );
                assert_eq!(result["no_result"], true);
            }
        } else if matches!(api, "typed-outputs" | "typed-outputs-context") {
            let native = if case == "invalid-utf8" {
                None
            } else {
                format_xml::from_str(xml, &project.source).ok()
            };
            if let Some(native) = native {
                let expected = if with_context {
                    engine::run_outputs_with_sources_and_context(
                        project,
                        &native,
                        Vec::new(),
                        &context,
                    )
                } else {
                    engine::run_outputs(project, &native)
                };
                match expected {
                    Ok(mapped) => {
                        assert_eq!(result["kind"], "ok");
                        assert_eq!(result["primary"], snapshot::snapshot(&mapped.primary));
                        assert_eq!(result["extras"],Json::Array(mapped.extras.iter().map(|target|json!({"name":target.name,"instance":snapshot::snapshot(&target.instance)})).collect()));
                        assert_mapped_values(result, case);
                    }
                    Err(_) => {
                        assert_eq!(result["kind"], "Mapping");
                        assert_eq!(result["no_result"], true);
                    }
                }
            } else {
                assert_eq!(result["kind"], "blocked_input");
                assert_eq!(result["attempted"], false);
            }
        } else {
            panic!("unexpected API {api}");
        }
    }
    let expected = inputs
        .iter()
        .flat_map(|(case, _)| {
            PUBLIC_APIS
                .into_iter()
                .chain([
                    "source",
                    "source-bytes",
                    "typed-outputs",
                    "typed-outputs-context",
                ])
                .filter(move |api| {
                    *case != "invalid-utf8"
                        || api.starts_with("bytes")
                        || *api == "source-bytes"
                        || api.starts_with("typed")
                })
                .map(move |api| ((*case).to_owned(), api.to_owned()))
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(seen, expected, "complete unique API/case matrix");
    for (case, _) in inputs {
        let successful = rows
            .iter()
            .filter(|row| {
                row["case"] == *case
                    && PUBLIC_APIS.contains(&row["api"].as_str().expect("API"))
                    && row["result"]["kind"] == "ok"
            })
            .collect::<Vec<_>>();
        if let Some(first) = successful.first() {
            for row in successful.iter().skip(1) {
                assert_eq!(
                    row["result"]["primary"]["xml"], first["result"]["primary"]["xml"],
                    "all old/new and UTF8/context primary buffers agree"
                );
            }
            let sets = successful
                .iter()
                .filter(|row| row["api"].as_str().expect("API").contains("outputs"))
                .collect::<Vec<_>>();
            for row in sets.iter().skip(1) {
                assert_eq!(
                    row["result"]["extras"], sets[0]["result"]["extras"],
                    "complete text/bytes/context named buffers agree"
                );
            }
        }
    }
    // Selecting the primary result must retain the same original boundary/cause.
    for (case, _) in inputs {
        for context in [false, true] {
            let failures = rows
                .iter()
                .filter(|row| {
                    row["case"] == *case
                        && PUBLIC_APIS.contains(&row["api"].as_str().expect("API"))
                        && row["api"].as_str().expect("API").ends_with("context") == context
                        && row["result"]["kind"] != "ok"
                })
                .collect::<Vec<_>>();
            if let Some(first) = failures.first() {
                for row in failures.iter().skip(1) {
                    for field in [
                        "kind",
                        "detail",
                        "function",
                        "found",
                        "runtime_value",
                        "boundary_bytes",
                        "boundary_limit",
                        "resource",
                        "writer_error",
                        "runtime_error",
                        "inner_sources",
                    ] {
                        assert_eq!(
                            row["result"][field], first["result"][field],
                            "old/new boundary cause retained for {variant}/{case}"
                        );
                    }
                }
            }
        }
    }
    Ok(())
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

fn prepare(
    directory: &Path,
    variant: &str,
    language: &str,
) -> TestResult<(Project, PathBuf, PathBuf, Inputs)> {
    let project = mapping(variant)?;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    let policy = lowered
        .xml_boundary
        .as_ref()
        .expect("XML output-set admission");
    assert_eq!(
        policy.input.profile(),
        Some(codegen::XmlInputProfile::Structured)
    );
    assert_eq!(
        policy
            .extra_outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        project
            .extra_targets
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>()
    );
    let path = directory.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let generated = directory.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        command.arg("--rust-runtime-path").arg(&runtime);
    }
    let output = recorded(&mut command, directory, "generate")?;
    assert!(
        output.status.success(),
        "public CLI generation:{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inputs = inputs(variant);
    std::fs::write(
        directory.join("cases.json"),
        serde_json::to_vec(
            &inputs
                .iter()
                .map(|(case, xml)| json!({"case":case,"xml":xml}))
                .collect::<Vec<_>>(),
        )?,
    )?;
    std::fs::write(
        directory.join("source-schema.json"),
        codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?,
    )?;
    Ok((project, path, generated, inputs))
}

fn exercise(language: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    for variant in VARIANTS {
        let case_directory = directory.path.join(variant);
        std::fs::create_dir(&case_directory)?;
        let (project, path, generated, inputs) = prepare(&case_directory, variant, language)?;
        let unchanged = artifact_files(&generated)?;
        let host = case_directory.join("host");
        std::fs::create_dir_all(host.join("src"))?;
        let output = if language == "rust" {
            std::fs::write(
                host.join("src/main.rs"),
                include_str!("fixtures/xml_output_sets_rust.rs.txt"),
            )?;
            std::fs::write(
                host.join("src/structured_xml_snapshot.rs.txt"),
                include_str!("fixtures/structured_xml_snapshot.rs.txt"),
            )?;
            std::fs::write(
                host.join("Cargo.toml"),
                format!(
                    "[package]\nname=\"ferrule-xml-output-sets-{variant}-regression-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={:?}}}\nserde_json=\"1\"\nroxmltree=\"0.21\"\nformat-xml={{path={:?}}}\n",
                    runtime.join("../ir"),
                    runtime.join("../format-xml")
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
            let mut command = Command::new("cargo");
            command
                .args(["build", "--quiet", "--jobs", "1"])
                .current_dir(&host)
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_INCREMENTAL", "0")
                .env("RUSTFLAGS", "-D warnings");
            let build = recorded(&mut command, &case_directory, "build")?;
            assert!(
                build.status.success(),
                "Rust host build:{}\n{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            let binary = target
                .join("debug")
                .join(format!("ferrule-xml-output-sets-{variant}-regression-host"))
                .with_extension(std::env::consts::EXE_EXTENSION);
            let mut command = Command::new(binary);
            command
                .arg(case_directory.join("cases.json"))
                .arg(case_directory.join("source-schema.json"))
                .arg(&path)
                .current_dir(&host);
            recorded(&mut command, &case_directory, "host")?
        } else {
            std::fs::write(
                host.join("Program.cs"),
                include_str!("fixtures/xml_output_sets_csharp.cs.txt"),
            )?;
            std::fs::write(
                host.join("Host.csproj"),
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
            )?;
            let artifacts = case_directory.join("artifacts");
            let mut build = dotnet_command(&case_directory);
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
                .current_dir(&case_directory)
                .env("DOTNET_PROCESSOR_COUNT", "2");
            let output = recorded(&mut build, &case_directory, "build")?;
            assert!(
                output.status.success(),
                "C# host build:{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let mut command = dotnet_command(&case_directory);
            command
                .arg(artifacts.join("bin/Host/release/Host.dll"))
                .arg(case_directory.join("cases.json"))
                .arg(case_directory.join("source-schema.json"))
                .arg(&path)
                .current_dir(&case_directory)
                .env("DOTNET_PROCESSOR_COUNT", "2");
            recorded(&mut command, &case_directory, "host")?
        };
        assert_eq!(
            artifact_files(&generated)?,
            unchanged,
            "generated library must remain untouched"
        );
        assert_rows(&output, variant, &project, &path, &inputs)?;
    }
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_xml_output_sets_preserve_all_targets_and_atomic_failure_order() -> TestResult<()>
{
    exercise("rust")
}
#[test]
fn generated_csharp_xml_output_sets_preserve_all_targets_and_atomic_failure_order() -> TestResult<()>
{
    exercise("csharp")
}

#[test]
fn generated_named_non_xml_target_keeps_core_outputs_without_xml_adapters() -> TestResult<()> {
    let mut directory = RegressionDirectory::new("core_fallback")?;
    let mut value: Json =
        serde_json::from_str(include_str!("fixtures/xml_output_sets/project.json"))?;
    value["extra_targets"][0]["options"] = json!({"json_document":true});
    let project: Project = serde_json::from_value(value)?;
    assert!(engine::validate(&project).is_empty());
    assert!(codegen::lower(&project)?.xml_boundary.is_none());
    let path = directory.path.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    for language in ["rust", "csharp"] {
        let generated = directory.path.join(language);
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .args(["generate", "--project"])
            .arg(&path)
            .args(["--language", language, "--out"])
            .arg(&generated);
        if language == "rust" {
            command
                .arg("--rust-runtime-path")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"));
        }
        let output = recorded(&mut command, &directory.path, language)?;
        assert!(
            output.status.success(),
            "core-only generation:{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let source = std::fs::read_to_string(generated.join(if language == "rust" {
            "src/lib.rs"
        } else {
            "GeneratedMapping.cs"
        }))?;
        assert!(source.contains(if language == "rust" {
            "pub fn execute_outputs("
        } else {
            " ExecuteOutputs("
        }));
        assert!(
            !source.contains(if language == "rust" {
                "pub fn execute_xml"
            } else {
                " ExecuteXml"
            }),
            "whole XML adapter must be omitted"
        );
    }
    directory.complete = true;
    Ok(())
}
