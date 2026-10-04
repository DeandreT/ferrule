use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

#[path = "xml_input_sets/cases.rs"]
mod fixture_cases;
use fixture_cases::*;

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        let directory = TempDir::new(&format!("xml_input_sets_{language}"))?;
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
                "Retained named XML input test artifacts: {}",
                self.path.display()
            );
        }
    }
}
fn assert_shape(row: &Json) {
    let result = &row["result"];
    assert_eq!(row["api"], "csharp-host-shape");
    assert_eq!(result["no_output"], true);
    if matches!(
        row["case"].as_str(),
        Some("primary-invalid-unicode" | "named-invalid-unicode-before-primary-parse")
    ) {
        assert_eq!(result["kind"], "Utf8");
        assert_eq!(result["output"], Json::Null);
        assert_eq!(result["boundary_preserved"], true);
        assert_eq!(
            result["input"],
            if row["case"] == "primary-invalid-unicode" {
                json!({"kind":"Primary","index":null,"name":null})
            } else {
                json!({"kind":"Named","index":0,"name":"rates"})
            }
        );
    } else if row["case"] == "later-null-before-size" {
        assert_eq!(result["kind"], "ArgumentNull");
        assert_eq!(result["type"], "System.ArgumentNullException");
        assert_eq!(result["parameter"], "input.Document");
    } else {
        assert_eq!(result["kind"], "Mapping");
        assert_eq!(result["input"], Json::Null);
        assert_eq!(result["output"], Json::Null);
        assert_eq!(result["boundary_preserved"], true);
        let (kind, name) = match row["case"].as_str().expect("shape case") {
            "unknown-before-null" => ("UnexpectedNamedSource", "unknown"),
            "duplicate-before-null" => ("DuplicateNamedSource", "rates"),
            "missing-before-null" => ("MissingNamedSource", "labels"),
            other => panic!("unexpected shape case {other}"),
        };
        assert_eq!(result["name_error"], json!({"kind":kind,"name":name}));
    }
    assert!(result.get("primary").is_none() && result.get("extras").is_none());
}
fn native_document(bytes: &[u8], schema: &SchemaNode) -> Option<Instance> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| format_xml::from_str(text, schema).ok())
}
fn source_schema<'a>(project: &'a Project, role: &str) -> &'a SchemaNode {
    if role == "primary" {
        &project.source
    } else {
        &project
            .extra_sources
            .iter()
            .find(|source| source.name == role)
            .expect("exact fixture schema owner")
            .schema
    }
}
fn assert_rows(
    output: &Output,
    variant: &str,
    language: &str,
    project: &Project,
    path: &Path,
    cases: &[Case],
) -> TestResult<()> {
    assert!(
        output.status.success(),
        "host stdout:{}\nstderr:{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let rows = std::str::from_utf8(&output.stdout)?
        .lines()
        .map(serde_json::from_str::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    let context = engine::ExecutionContext::new(path).with_current_datetime(CURRENT);
    let without_datetime = engine::ExecutionContext::new(path);
    let mut seen = BTreeSet::new();
    for row in &rows {
        let name = row["case"].as_str().expect("case");
        let api = row["api"].as_str().expect("API");
        assert!(
            seen.insert((name.to_owned(), api.to_owned())),
            "duplicate {name}/{api}"
        );
        if api == "csharp-host-shape" {
            assert_eq!(language, "csharp");
            assert_eq!(variant, "base");
            assert_shape(row);
            continue;
        }
        let case = cases
            .iter()
            .find(|case| case.name == name)
            .expect("known case");
        let result = &row["result"];
        if PUBLIC_APIS.contains(&api) {
            let (kind, input_owner, output_owner, name_error) =
                expected(variant, name, api, project);
            assert_eq!(result["kind"], kind, "{variant}/{name}/{api}: {result}");
            if kind == "ok" {
                assert_eq!(result["primary"]["physical"], physical(variant, name, None));
                assert!(
                    result["primary"]["xml"]
                        .as_str()
                        .expect("complete XML")
                        .starts_with("<?xml ")
                );
                if api.contains("outputs") {
                    let extras = result["extras"].as_array().expect("ordered outputs");
                    assert_eq!(extras.len(), project.extra_targets.len());
                    for (actual, target) in extras.iter().zip(&project.extra_targets) {
                        assert_eq!(actual["name"], target.name);
                        assert_eq!(
                            actual["physical"],
                            physical(variant, name, Some(&target.name))
                        );
                    }
                } else {
                    assert!(
                        result.get("extras").is_none(),
                        "singular API selects primary after complete set"
                    );
                }
            } else {
                assert_eq!(result["no_output"], true);
                for key in ["primary", "extras", "xml"] {
                    assert!(result.get(key).is_none(), "no partial {key}");
                }
                assert!(
                    result["detail"]
                        .as_str()
                        .is_some_and(|detail| !detail.is_empty())
                );
                if api.contains("sources") {
                    assert!(result.get("input").is_some() && result.get("output").is_some());
                    assert_eq!(result["input"], input_owner);
                    assert_eq!(result["output"], output_owner);
                    assert_eq!(result["boundary_preserved"], true);
                    assert!(
                        result["input"].is_null() || result["output"].is_null(),
                        "exclusive phase owner"
                    );
                } else if api.contains("outputs") {
                    assert_eq!(result["target"], output_owner);
                    assert_eq!(result["boundary_preserved"], true);
                }
                if let Some(expected) = name_error {
                    assert_eq!(result["name_error"], expected);
                }
                if name == "count-over" && api.contains("sources") {
                    assert_eq!(
                        result["resource"],
                        json!({"resource":"xml_input_artifact_count","observed_count":4097,"limit":4096})
                    );
                    assert_eq!(result["boundary_bytes"], Json::Null);
                    assert_eq!(result["boundary_limit"], Json::Null);
                }
                if kind == "Mapping" && api.contains("sources") && name == "missing-factor" {
                    assert_eq!(result["function"], "multiply");
                    assert_eq!(
                        result["found"]
                            .as_str()
                            .expect("Null cause")
                            .to_ascii_lowercase(),
                        "null"
                    );
                }
                if kind == "Mapping"
                    && api.contains("sources")
                    && matches!(name, "need-context" | "competing-failures")
                {
                    assert_eq!(result["runtime_value"], "CurrentDateTime");
                }
                if kind == "Output" {
                    assert!(
                        result["inner_sources"]
                            .as_array()
                            .is_some_and(|causes| !causes.is_empty())
                    );
                    if language == "rust" {
                        assert_eq!(result["writer_error"]["type"], "XmlFormatError");
                    } else {
                        assert_eq!(result["runtime_error"], "XmlSerialization");
                    }
                }
            }
        } else if api.starts_with("source-") {
            let base = api.strip_suffix("-bytes").unwrap_or(api);
            let (bytes, role) = if base == "source-primary" {
                (&case.primary, "primary")
            } else {
                let index = base
                    .strip_prefix("source-")
                    .expect("source index")
                    .parse::<usize>()?;
                let input = &case.inputs[index];
                (&input.bytes, input.schema)
            };
            let native = native_document(bytes, source_schema(project, role));
            if let Some(native) = native {
                assert_eq!(result["kind"], "ok");
                assert_eq!(result["snapshot"], snapshot::snapshot(&native));
                if name != "swapped-documents" {
                    assert_source_facts(&result["snapshot"], role, name);
                }
            } else {
                assert_eq!(
                    result["kind"],
                    if std::str::from_utf8(bytes).is_err() {
                        "Utf8"
                    } else {
                        "Input"
                    }
                );
                assert_eq!(result["no_output"], true);
                assert!(result.get("snapshot").is_none());
            }
        } else if api.starts_with("typed-outputs") {
            let primary = native_document(&case.primary, &project.source);
            let named = case
                .inputs
                .iter()
                .map(|input| native_document(&input.bytes, source_schema(project, input.schema)))
                .collect::<Option<Vec<_>>>();
            if let (Some(primary), Some(named)) = (primary, named) {
                // Generated boundaries require the complete declared input set,
                // including sources the interpreter would never read.
                let (_, _, _, name_error) =
                    expected(variant, name, "xml-outputs-sources-context", project);
                if let Some(name_error) = name_error {
                    assert_eq!(result["kind"], "Mapping");
                    assert_eq!(result["name_error"], name_error);
                    assert_eq!(result["no_output"], true);
                    assert!(result.get("primary").is_none() && result.get("extras").is_none());
                    continue;
                }
                let inputs = case
                    .inputs
                    .iter()
                    .zip(named)
                    .map(|(input, instance)| (input.name.to_owned(), instance))
                    .collect::<Vec<_>>();
                let mapped = if api.ends_with("context") {
                    engine::run_outputs_with_sources_and_context(
                        project, &primary, inputs, &context,
                    )
                } else {
                    engine::run_outputs_with_sources_and_context(
                        project,
                        &primary,
                        inputs,
                        &without_datetime,
                    )
                };
                match mapped {
                    Ok(mapped) => {
                        assert_eq!(result["kind"], "ok");
                        assert_eq!(result["primary"], snapshot::snapshot(&mapped.primary));
                        assert_eq!(result["extras"],Json::Array(mapped.extras.iter().map(|target|json!({"name":target.name,"instance":snapshot::snapshot(&target.instance)})).collect()));
                        if variant != "zero" {
                            assert_eq!(
                                field(&result["primary"], "Product").expect("Product")["tag"],
                                "Float"
                            );
                            assert_eq!(
                                field(&result["primary"], "Product").expect("Product")["bits_hex"],
                                match name {
                                    "nonintegral-factor" => "4026800000000000",
                                    "competing-failures" => "4025000000000000",
                                    _ => "4024000000000000",
                                }
                            );
                        }
                    }
                    Err(_) => {
                        assert_eq!(result["kind"], "Mapping");
                        assert_eq!(result["no_output"], true);
                        assert!(result.get("primary").is_none() && result.get("extras").is_none());
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
    let mut expected_rows = BTreeSet::new();
    for case in cases {
        for api in PUBLIC_APIS {
            if api.starts_with("bytes") || case.text_api(api.contains("sources")) {
                expected_rows.insert((case.name.to_owned(), api.to_owned()));
            }
        }
        if case.diagnostics {
            for (index, bytes) in std::iter::once(&case.primary)
                .chain(case.inputs.iter().map(|input| &input.bytes))
                .enumerate()
            {
                let api = if index == 0 {
                    "source-primary".to_owned()
                } else {
                    format!("source-{}", index - 1)
                };
                if std::str::from_utf8(bytes).is_ok() {
                    expected_rows.insert((case.name.to_owned(), api.clone()));
                }
                expected_rows.insert((case.name.to_owned(), format!("{api}-bytes")));
            }
            for api in ["typed-outputs", "typed-outputs-context"] {
                expected_rows.insert((case.name.to_owned(), api.to_owned()));
            }
        }
    }
    if language == "csharp" && variant == "base" {
        for name in [
            "primary-invalid-unicode",
            "named-invalid-unicode-before-primary-parse",
            "later-null-before-size",
            "unknown-before-null",
            "duplicate-before-null",
            "missing-before-null",
        ] {
            expected_rows.insert((name.to_owned(), "csharp-host-shape".to_owned()));
        }
    }
    assert_eq!(
        seen, expected_rows,
        "complete unique public/typed/source matrix"
    );
    for case in cases {
        let successful = rows
            .iter()
            .filter(|row| {
                row["case"] == case.name
                    && PUBLIC_APIS.contains(&row["api"].as_str().expect("API"))
                    && row["result"]["kind"] == "ok"
            })
            .collect::<Vec<_>>();
        if let Some(first) = successful.first() {
            for row in successful.iter().skip(1) {
                assert_eq!(
                    row["result"]["primary"], first["result"]["primary"],
                    "all text/bytes/singular/set/fixed-context primary buffers agree"
                );
            }
            let sets = successful
                .iter()
                .filter(|row| row["api"].as_str().expect("API").contains("outputs"))
                .collect::<Vec<_>>();
            if let Some(first) = sets.first() {
                for row in sets.iter().skip(1) {
                    assert_eq!(row["result"]["extras"], first["result"]["extras"]);
                }
            }
        }
        // Old empty-source and new empty-source calls retain the same original boundary.
        if variant == "zero" {
            let failures = rows
                .iter()
                .filter(|row| {
                    row["case"] == case.name
                        && PUBLIC_APIS.contains(&row["api"].as_str().expect("API"))
                        && row["result"]["kind"] != "ok"
                })
                .collect::<Vec<_>>();
            if let Some(first) = failures.first() {
                for row in failures.iter().skip(1) {
                    for key in [
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
                            row["result"][key], first["result"][key],
                            "original boundary/cause retained"
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
) -> TestResult<(Project, PathBuf, PathBuf, Vec<Case>)> {
    let project = mapping(variant)?;
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    let policy = lowered
        .xml_boundary
        .as_ref()
        .expect("complete XML input/output admission");
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
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        project
            .extra_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(lowered.extra_sources.len(), project.extra_sources.len());
    for (lowered, source) in lowered.extra_sources.iter().zip(&project.extra_sources) {
        assert_eq!(lowered.name, source.name);
        assert_eq!(lowered.source, source.schema);
        assert!(lowered.dynamic.is_none());
    }
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
    let inputs = cases(variant);
    std::fs::write(
        directory.join("cases.json"),
        serde_json::to_vec(&inputs.iter().map(Case::json).collect::<Vec<_>>())?,
    )?;
    let mut descriptors = serde_json::Map::new();
    descriptors.insert(
        "primary".into(),
        json!(codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES
        )?),
    );
    for source in &project.extra_sources {
        descriptors.insert(
            source.name.clone(),
            json!(codegen::serialize_embedded_schema(
                &source.schema,
                codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES
            )?),
        );
    }
    std::fs::write(
        directory.join("descriptors.json"),
        serde_json::to_vec(&descriptors)?,
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
                include_str!("fixtures/xml_input_sets_rust.rs.txt"),
            )?;
            std::fs::write(
                host.join("src/structured_xml_snapshot.rs.txt"),
                include_str!("fixtures/structured_xml_snapshot.rs.txt"),
            )?;
            std::fs::write(
                host.join("Cargo.toml"),
                format!(
                    "[package]\nname=\"ferrule-xml-input-sets-{variant}-regression-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={:?}}}\nserde_json=\"1\"\nroxmltree=\"0.21\"\nformat-xml={{path={:?}}}\n",
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
                .join(format!("ferrule-xml-input-sets-{variant}-regression-host"))
                .with_extension(std::env::consts::EXE_EXTENSION);
            let mut command = Command::new(binary);
            command
                .arg(case_directory.join("cases.json"))
                .arg(case_directory.join("descriptors.json"))
                .arg(&path)
                .arg(variant)
                .current_dir(&host);
            recorded(&mut command, &case_directory, "host")?
        } else {
            std::fs::write(
                host.join("Program.cs"),
                include_str!("fixtures/xml_input_sets_csharp.cs.txt"),
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
                .arg(case_directory.join("descriptors.json"))
                .arg(&path)
                .arg(variant)
                .current_dir(&case_directory)
                .env("DOTNET_PROCESSOR_COUNT", "2");
            recorded(&mut command, &case_directory, "host")?
        };
        assert_eq!(
            artifact_files(&generated)?,
            unchanged,
            "generated library must remain untouched"
        );
        assert_rows(&output, variant, language, &project, &path, &inputs)?;
    }
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_rust_static_named_xml_inputs_preserve_sources_and_ordered_outputs() -> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_static_named_xml_inputs_preserve_sources_and_ordered_outputs() -> TestResult<()>
{
    exercise("csharp")
}

#[test]
fn named_xml_source_admission_is_complete_and_root_view_apis_remain_unchanged() -> TestResult<()> {
    let mut directory = RegressionDirectory::new("admission")?;
    for variant in ["non-xml", "unproved", "root-view"] {
        let project = if variant == "root-view" {
            serde_json::from_str::<Project>(include_str!("fixtures/xml_input_sets/root_view.json"))?
        } else {
            let mut value: Json =
                serde_json::from_str(include_str!("fixtures/xml_input_sets/project.json"))?;
            value["target_options"]["xml_schema_hints"] =
                json!({"no_namespace_location":"literal-result.xsd"});
            value["extra_targets"][0]["options"]["xml_schema_hints"] =
                json!({"no_namespace_location":"literal-audit.xsd"});
            if variant == "non-xml" {
                value["extra_sources"][1]["options"] = json!({});
            } else {
                value["extra_sources"][1]["schema"]["kind"]["children"][0]["fixed"] = json!("Ω");
            }
            serde_json::from_value(value)?
        };
        assert!(engine::validate(&project).is_empty());
        let program = codegen::lower(&project)?;
        if variant == "root-view" {
            assert_eq!(
                program
                    .xml_boundary
                    .as_ref()
                    .expect("original observed boundary")
                    .input
                    .profile(),
                Some(codegen::XmlInputProfile::RootView)
            );
        } else {
            assert!(program.xml_boundary.is_none());
            assert_eq!(program.extra_sources.len(), project.extra_sources.len());
            assert_eq!(program.extra_targets.len(), project.extra_targets.len());
        }
        let path = directory.path.join(format!("{variant}.json"));
        std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
        for language in ["rust", "csharp"] {
            let generated = directory.path.join(format!("{variant}-{language}"));
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
            let output = recorded(
                &mut command,
                &directory.path,
                &format!("{variant}-{language}"),
            )?;
            assert!(
                output.status.success(),
                "complete core/observed generation:{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let source = std::fs::read_to_string(generated.join(if language == "rust" {
                "src/lib.rs"
            } else {
                "GeneratedMapping.cs"
            }))?;
            assert!(
                !source.contains(if language == "rust" {
                    "pub fn execute_xml_outputs_with_sources("
                } else {
                    " ExecuteXmlOutputsWithSources("
                }),
                "unadmitted source adapters must not be emitted"
            );
            if variant == "root-view" {
                for method in if language == "rust" {
                    vec![
                        "pub fn execute_xml_outputs(",
                        "pub fn execute_xml_outputs_with_context(",
                        "pub fn execute_xml_bytes_outputs(",
                        "pub fn execute_xml_bytes_outputs_with_context(",
                        "pub fn execute_xml(",
                        "pub fn execute_xml_with_context(",
                        "pub fn execute_xml_bytes(",
                        "pub fn execute_xml_bytes_with_context(",
                    ]
                } else {
                    vec![
                        " ExecuteXmlOutputs(",
                        " ExecuteXmlBytesOutputs(",
                        " ExecuteXml(",
                        " ExecuteXmlBytes(",
                    ]
                } {
                    assert!(source.contains(method), "original method {method}");
                }
            } else {
                assert!(!source.contains(if language == "rust" {
                    "pub fn execute_xml"
                } else {
                    " ExecuteXml"
                }));
                assert!(source.contains(if language == "rust" {
                    "pub fn execute_outputs_with_sources("
                } else {
                    " ExecuteOutputsWithSources("
                }));
                assert!(source.contains(if language == "rust" {
                    "pub fn execute_json_outputs_with_sources("
                } else {
                    " ExecuteJsonOutputsWithSources("
                }));
            }
        }
    }
    // An observed named policy is an explicit refusal before any artifact exists.
    let mut project = mapping("base")?;
    project.extra_sources[1]
        .options
        .xml_allow_inactive_root_type_members = true;
    project.extra_sources[1].options.xml_root_view_read_policy = true;
    assert!(codegen::lower(&project).is_err());
    let path = directory.path.join("observed-named.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    for language in ["rust", "csharp"] {
        let generated = directory.path.join(format!("refused-{language}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .args(["--diagnostics", "json", "generate", "--project"])
            .arg(&path)
            .args(["--language", language, "--out"])
            .arg(&generated);
        if language == "rust" {
            command
                .arg("--rust-runtime-path")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"));
        }
        let output = recorded(
            &mut command,
            &directory.path,
            &format!("refused-{language}"),
        )?;
        assert!(!output.status.success());
        assert!(!generated.exists());
    }
    directory.complete = true;
    Ok(())
}
