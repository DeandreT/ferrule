use super::structured_xml_snapshot as snapshot;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
#[path = "fixtures/xml_input_document_outputs_native.rs.txt"]
mod native;
struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str, variant: &str) -> io::Result<Self> {
        let d = TempDir::new(&format!("xml_input_document_outputs_{language}_{variant}"))?;
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
                "Retained named-input mixed XML test artifacts: {}",
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
fn field<'a>(group: &'a Json, name: &str) -> &'a Json {
    &group["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .unwrap()["instance"]
}
fn physical(xml: &str) -> Json {
    fn element(n: roxmltree::Node<'_, '_>) -> Json {
        let mut attrs = n
            .attributes()
            .map(|a| (a.namespace().unwrap_or(""), a.name(), a.value()))
            .collect::<Vec<_>>();
        attrs.sort();
        let children = n
            .children()
            .filter(|n| n.is_element())
            .map(element)
            .collect::<Vec<_>>();
        let raw = n
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
        json!({"name":n.tag_name().name(),"namespace":n.tag_name().namespace().unwrap_or(""),"text":text,"attributes":attrs.iter().map(|(namespace,name,value)|json!({"name":name,"namespace":namespace,"value":value})).collect::<Vec<_>>(),"children":children})
    }
    element(roxmltree::Document::parse(xml).unwrap().root_element())
}
fn exercise(language: &str, variant: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language, variant)?;
    let is_context = variant == "context";
    let mut project: Project = serde_json::from_str(if is_context {
        include_str!("fixtures/xml_input_document_outputs/context-project.json")
    } else {
        include_str!("fixtures/xml_input_document_outputs/project.json")
    })?;
    if variant == "reversed" {
        project.extra_targets.reverse();
    }
    let tests: Vec<Json> = serde_json::from_str(if is_context {
        include_str!("fixtures/xml_input_document_outputs/context-cases.json")
    } else {
        include_str!("fixtures/xml_input_document_outputs/cases.json")
    })?;
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        codegen::lower(&project)?.xml_output_mode()?,
        Some(codegen::XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments)
    );
    assert_eq!(
        project
            .extra_sources
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["z-rates", "a-count", "unused-padding"]
    );
    assert_eq!(project.extra_targets.len(), 2);
    const INPUTS: &[(&str, &[u8])] = &[
        (
            "alpha-output.xml",
            include_bytes!("fixtures/xml_input_document_outputs/alpha-output.xml"),
        ),
        (
            "count-malformed.xml",
            include_bytes!("fixtures/xml_input_document_outputs/count-malformed.xml"),
        ),
        (
            "count.xml",
            include_bytes!("fixtures/xml_input_document_outputs/count.xml"),
        ),
        (
            "empty-alpha.xml",
            include_bytes!("fixtures/xml_input_document_outputs/empty-alpha.xml"),
        ),
        (
            "empty-beta.xml",
            include_bytes!("fixtures/xml_input_document_outputs/empty-beta.xml"),
        ),
        (
            "empty-both.xml",
            include_bytes!("fixtures/xml_input_document_outputs/empty-both.xml"),
        ),
        (
            "filtered.xml",
            include_bytes!("fixtures/xml_input_document_outputs/filtered.xml"),
        ),
        (
            "invalid-utf8.bin",
            include_bytes!("fixtures/xml_input_document_outputs/invalid-utf8.bin"),
        ),
        (
            "late-beta-output.xml",
            include_bytes!("fixtures/xml_input_document_outputs/late-beta-output.xml"),
        ),
        (
            "mapping-alpha.xml",
            include_bytes!("fixtures/xml_input_document_outputs/mapping-alpha.xml"),
        ),
        (
            "mapping-primary.xml",
            include_bytes!("fixtures/xml_input_document_outputs/mapping-primary.xml"),
        ),
        (
            "ordered.xml",
            include_bytes!("fixtures/xml_input_document_outputs/ordered.xml"),
        ),
        (
            "padding-malformed.xml",
            include_bytes!("fixtures/xml_input_document_outputs/padding-malformed.xml"),
        ),
        (
            "padding.xml",
            include_bytes!("fixtures/xml_input_document_outputs/padding.xml"),
        ),
        (
            "primary-malformed.xml",
            include_bytes!("fixtures/xml_input_document_outputs/primary-malformed.xml"),
        ),
        (
            "primary-output.xml",
            include_bytes!("fixtures/xml_input_document_outputs/primary-output.xml"),
        ),
        (
            "rates-malformed.xml",
            include_bytes!("fixtures/xml_input_document_outputs/rates-malformed.xml"),
        ),
        (
            "rates-negative-zero.xml",
            include_bytes!("fixtures/xml_input_document_outputs/rates-negative-zero.xml"),
        ),
        (
            "rates.xml",
            include_bytes!("fixtures/xml_input_document_outputs/rates.xml"),
        ),
        (
            "selected.xml",
            include_bytes!("fixtures/xml_input_document_outputs/selected.xml"),
        ),
    ];
    for (name, bytes) in INPUTS {
        std::fs::write(directory.path.join(name), bytes)?;
    }
    let path = directory.path.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    std::fs::write(
        directory.path.join("cases.json"),
        serde_json::to_vec_pretty(&tests)?,
    )?;
    let expected = native::prepare(&directory.path)?;
    for (case, value) in &expected {
        assert_eq!(
            value["inputs"].as_array().unwrap().len(),
            3,
            "complete unused source included"
        );
        let rate = field(&value["inputs"][0]["instance"], "Rate");
        assert_eq!(rate["tag"], "Float");
        assert_eq!(
            rate["bits_hex"],
            if case == "ordered" || case == "context-success" {
                "8000000000000000"
            } else {
                "4340000000000000"
            }
        );
        assert_eq!(
            field(&value["inputs"][1]["instance"], "Count"),
            &json!({"kind":"Scalar","tag":"Int","value":9007199254740993_i64})
        );
        if value["mapping_failure"] == true {
            continue;
        }
        assert_eq!(value["extras"].as_array().unwrap().len(), 2);
        if is_context {
            assert_eq!(
                field(&value["primary"], "Marker")["value"],
                "2026-10-04T00:00:00Z"
            );
        }
        for extra in value["extras"].as_array().unwrap() {
            let alpha = extra["name"] == "z-alpha";
            let members = extra["members"].as_array().unwrap();
            let paths = members
                .iter()
                .map(|m| m["path"].as_str().unwrap())
                .collect::<Vec<_>>();
            match case.as_str() {
                "ordered" | "context-success" => {
                    assert_eq!(
                        paths,
                        if alpha {
                            vec!["same.xml", "same.xml", "résult/雪😀.xml"]
                        } else {
                            vec!["same.xml", "/opaque.xml", "../opaque.xml"]
                        }
                    );
                    let first = field(&members[0]["instance"], "Amount");
                    assert_eq!(first["tag"], if alpha { "Float" } else { "Int" });
                    if alpha {
                        assert_eq!(first["bits_hex"], "4340000000000000");
                        assert_eq!(
                            field(&members[1]["instance"], "Amount")["bits_hex"],
                            "8000000000000000"
                        );
                    } else {
                        assert_eq!(first["value"], json!(9007199254740993_i64));
                    }
                    assert_eq!(field(&members[1]["instance"], "Value")["tag"], "XmlNil");
                    assert_eq!(
                        field(&members[2]["instance"], "Value")["value"],
                        "  keep spaces  "
                    );
                }
                "empty-alpha" => assert_eq!(members.len(), usize::from(!alpha)),
                "empty-beta" => assert_eq!(members.len(), usize::from(alpha)),
                "empty-both" | "filtered" => assert!(members.is_empty()),
                "selected" => assert_eq!(
                    paths,
                    if alpha {
                        vec!["middle-alpha.xml", "last-alpha.xml"]
                    } else {
                        vec!["middle-beta.xml", "last-beta.xml"]
                    }
                ),
                "late-beta-output" if !alpha => {
                    assert_eq!(paths, ["middle-beta.xml", "last-raw-beta.xml"]);
                    assert_eq!(field(&members[1]["instance"], "Value")["value"], "\u{1}");
                }
                "alpha-output" if alpha => {
                    assert_eq!(field(&members[1]["instance"], "Value")["value"], "\u{1}")
                }
                _ => {}
            }
        }
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
    assert_eq!(unchanged.len(), if language == "rust" { 2 } else { 74 });
    let api = std::fs::read_to_string(generated.join(if language == "rust" {
        "src/lib.rs"
    } else {
        "GeneratedMapping.cs"
    }))?;
    if language == "rust" {
        for name in [
            "execute_xml_document_outputs_with_sources",
            "execute_xml_document_outputs_with_sources_and_context",
            "execute_xml_bytes_document_outputs_with_sources",
            "execute_xml_bytes_document_outputs_with_sources_and_context",
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
        assert_eq!(
            api.matches(" ExecuteXmlDocumentOutputsWithSources(")
                .count(),
            2
        );
        assert_eq!(
            api.matches(" ExecuteXmlBytesDocumentOutputsWithSources(")
                .count(),
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
                .join("Runtime/FerruleXml.InputDocumentOutputs.cs")
                .is_file()
        );
    }

    let host = directory.path.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    let output = if language == "rust" {
        let package = "ferrule-xml-input-mixed-output-regression-host";
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("fixtures/xml_input_document_outputs_rust.rs.txt"),
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
            include_str!("fixtures/xml_input_document_outputs_csharp.cs.txt"),
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
    let public = tests
        .iter()
        .map(|t| t["apis"].as_array().unwrap().len())
        .sum::<usize>();
    let typed = tests.iter().filter(|t| t["semantic"] == true).count();
    assert_eq!(
        rows.len(),
        public + typed + if language == "csharp" { 12 } else { 0 }
    );
    let mut identities = BTreeSet::new();
    for row in &rows {
        let name = row["case"].as_str().unwrap();
        let api = row["api"].as_i64().unwrap();
        assert!(identities.insert((name.to_owned(), api)));
        if name.starts_with("csharp-") {
            assert_eq!(language, "csharp");
            assert!((0..4).contains(&api));
            assert_eq!(row["no_partial_result"], true);
            if name == "csharp-null-document-before-decode" {
                assert_eq!(row["kind"], "host-shape");
                assert_eq!(row["exception"], "System.ArgumentNullException");
                assert!(row["param_name"].as_str().is_some_and(|n| !n.is_empty()));
            } else {
                assert!(matches!(
                    name,
                    "csharp-count-accessor-original" | "csharp-indexer-accessor-original"
                ));
                assert_eq!(row["kind"], "host-accessor");
                assert_eq!(row["original_reference"], true);
            }
            continue;
        }
        let test = tests.iter().find(|t| t["case"] == name).unwrap();
        if api == -1 {
            assert_eq!(test["semantic"], true);
            for key in ["source", "inputs", "primary", "extras", "mapping_failure"] {
                assert_eq!(
                    row[key], expected[name][key],
                    "complete typed source/three named inputs/primary/each ordered member, scalar tags/bits/nil/whitespace/XML origins"
                );
            }
            continue;
        }
        assert!(test["apis"].as_array().unwrap().contains(&json!(api)));
        let no_context = test["no_context_missing"] == true && api % 2 == 0;
        let kind = if no_context {
            "Mapping"
        } else {
            test["kind"].as_str().unwrap()
        };
        assert_eq!(row["kind"], kind);
        if kind == "ok" {
            let wanted = &expected[name];
            assert_eq!(
                snapshot::snapshot(&format_xml::from_str(
                    row["primary"]["document"].as_str().unwrap(),
                    &project.target
                )?),
                wanted["primary"]
            );
            assert_eq!(
                row["primary"]["physical"],
                physical(&std::fs::read_to_string(
                    directory.path.join(format!("native-{name}-primary.xml"))
                )?)
            );
            assert_eq!(row["primary"]["native_physical_match"], true);
            let extras = row["extras"].as_array().unwrap();
            let wanted_extras = wanted["extras"].as_array().unwrap();
            assert_eq!(extras.len(), 2);
            for (declaration, (extra, wanted)) in extras.iter().zip(wanted_extras).enumerate() {
                assert_eq!(extra["name"], wanted["name"]);
                let docs = extra["documents"].as_array().unwrap();
                let members = wanted["members"].as_array().unwrap();
                assert_eq!(docs.len(), members.len());
                for (index, (doc, member)) in docs.iter().zip(members).enumerate() {
                    assert_eq!(doc["path"], member["path"]);
                    assert_eq!(
                        snapshot::snapshot(&format_xml::from_str(
                            doc["document"].as_str().unwrap(),
                            &project.extra_targets[declaration].schema
                        )?),
                        member["instance"]
                    );
                    assert_eq!(
                        doc["physical"],
                        physical(&std::fs::read_to_string(
                            directory
                                .path
                                .join(format!("native-{name}-{declaration}-{index}.xml"))
                        )?)
                    );
                    assert_eq!(doc["native_physical_match"], true);
                }
            }
            assert_eq!(
                row["owned_byte_buffers"],
                if api >= 2 { json!(true) } else { Json::Null }
            );
            if language == "csharp" {
                assert_eq!(row["read_only_lists"], true);
            }
        } else {
            assert_eq!(
                row["wrapper"],
                if language == "rust" {
                    "XmlInputDocumentOutputsExecutionError"
                } else {
                    "FerruleXmlInputDocumentOutputsExecutionException"
                }
            );
            assert_eq!(row["boundary_identity"], true);
            assert_eq!(row["original_boundary_match"], true);
            assert_eq!(row["no_partial_result"], true);
            assert_eq!(row["boundary"], row["original_boundary"]);
            assert!(row.get("primary").is_none() && row.get("extras").is_none());
            let owner = if test["primitive"]["phase"] == "output" {
                let target = test["primitive"]["target"].as_str().unwrap();
                json!({"kind":"Output","owner":if target=="primary"{json!({"kind":"Primary"})}else{let declaration=project.extra_targets.iter().position(|t|t.name==target).unwrap();let index=test["primitive"]["index"].as_u64().unwrap();let path=expected[name]["extras"][declaration]["members"][index as usize]["path"].clone();json!({"kind":"NamedMember","declaration_index":declaration,"name":target,"index":index,"path":path})}})
            } else {
                test["owner"].clone()
            };
            assert_eq!(row["owner"], owner);
            if no_context {
                assert_eq!(row["boundary"]["runtime"]["error"], "MissingRuntimeValue");
                assert_eq!(
                    row["boundary"]["runtime"]["runtime_value"],
                    "CurrentDateTime"
                );
                assert!(row["owner"].is_null());
            }
            if test["primitive"]["phase"] == "mapping" {
                assert_eq!(
                    row["boundary"]["runtime"]["error"],
                    "EmptyDynamicTargetPath"
                );
                assert_eq!(row["boundary"]["runtime"]["node"], 3);
            }
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
            if test["primitive"]["phase"] == "count" {
                assert_eq!(
                    row["boundary"]["resource"],
                    json!({"resource":"xml_input_artifact_count","observed_count":4097,"limit":4096})
                );
            }
        }
    }
    let mut required = tests
        .iter()
        .flat_map(|t| {
            t["apis"].as_array().unwrap().iter().map(move |api| {
                (
                    t["case"].as_str().unwrap().to_owned(),
                    api.as_i64().unwrap(),
                )
            })
        })
        .collect::<BTreeSet<_>>();
    for t in tests.iter().filter(|t| t["semantic"] == true) {
        required.insert((t["case"].as_str().unwrap().to_owned(), -1));
    }
    if language == "csharp" {
        for case in [
            "csharp-null-document-before-decode",
            "csharp-count-accessor-original",
            "csharp-indexer-accessor-original",
        ] {
            for api in 0..4 {
                required.insert((case.into(), api));
            }
        }
    }
    assert_eq!(identities, required);
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_rust_static_named_inputs_mixed_xml_document_outputs_preserve_phases_and_context()
-> TestResult<()> {
    for variant in ["normal", "reversed", "context"] {
        exercise("rust", variant)?;
    }
    Ok(())
}
#[test]
fn generated_csharp_static_named_inputs_mixed_xml_document_outputs_preserve_phases_and_context()
-> TestResult<()> {
    for variant in ["normal", "reversed", "context"] {
        exercise("csharp", variant)?;
    }
    Ok(())
}
