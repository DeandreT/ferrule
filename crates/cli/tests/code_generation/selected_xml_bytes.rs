//! Self-authored ordinary XML selection, separate from Basic/document-set APIs.
use super::*;
use serde_json::{Value as Json, json};

#[path = "selected_xml_bytes/counterexamples.rs"]
mod counterexamples;

const SOURCE: &str = r#"<o:Orders xmlns:o="urn:orders" token="A&amp;B" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><o:Row><Id>1</Id><Text>bolt😀&lt;&amp;&gt;</Text><Maybe xsi:nil="true"/></o:Row><o:Row><Id>2</Id><Text>nut</Text></o:Row></o:Orders>"#;
const RATES: &str = "<Rates><Value>approved</Value><UseContext>false</UseContext></Rates>";
const UNUSED: &str = "<Unused><Value>unused</Value></Unused>";

fn field(path: &[&str]) -> Node {
    Node::SourceField {
        path: path.iter().map(|part| (*part).into()).collect(),
        frame: None,
    }
}
fn bind(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn options() -> mapping::FormatOptions {
    mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    }
}
fn project(bad: &str) -> Project {
    let row = SchemaNode::group(
        "Row",
        vec![int("Id"), string("Text"), string("Maybe").nillable()],
    )
    .xml_qualified("urn:orders")
    .unwrap()
    .repeating();
    let source = SchemaNode::group("Orders", vec![string("token").attribute(), row.clone()])
        .xml_qualified("urn:orders")
        .unwrap();
    let target = SchemaNode::group("Result", vec![string("token").attribute(), row])
        .xml_qualified("urn:orders")
        .unwrap();
    let mut result = Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: options(),
        target_options: options(),
        extra_sources: vec![
            mapping::NamedSource {
                name: "rates".into(),
                path: "not-opened.xml".into(),
                schema: SchemaNode::group("Rates", vec![string("Value"), bool_("UseContext")]),
                options: options(),
                dynamic_path: None,
            },
            mapping::NamedSource {
                name: "unused".into(),
                path: "not-opened-either.xml".into(),
                schema: SchemaNode::group("Unused", vec![string("Value")]),
                options: options(),
                dynamic_path: None,
            },
        ],
        extra_targets: vec![
            mapping::NamedTarget {
                name: "audit".into(),
                path: None,
                schema: SchemaNode::group("Audit", vec![string("Value")]),
                options: options(),
                root: Scope {
                    target_field: "Audit".into(),
                    bindings: vec![bind("Value", 12)],
                    ..Default::default()
                },
            },
            mapping::NamedTarget {
                name: "broken".into(),
                path: None,
                schema: SchemaNode::group("Broken", vec![string("Value"), string("Other")]),
                options: options(),
                root: Scope {
                    target_field: "Broken".into(),
                    bindings: vec![bind("Value", 6), bind("Other", 7)],
                    ..Default::default()
                },
            },
            mapping::NamedTarget {
                name: "combined".into(),
                path: None,
                schema: SchemaNode::group("Combined", vec![string("Value"), string("Other")]),
                options: options(),
                root: Scope {
                    target_field: "Combined".into(),
                    bindings: vec![bind("Value", 5), bind("Other", 7)],
                    ..Default::default()
                },
            },
        ],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (1, field(&["token"])),
                (2, field(&["Id"])),
                (3, field(&["Text"])),
                (4, field(&["Maybe"])),
                (5, field(&["rates", "Value"])),
                (
                    6,
                    Node::Const {
                        value: Value::String(
                            if bad == "output" {
                                "invalid\0XML"
                            } else {
                                "good"
                            }
                            .into(),
                        ),
                    },
                ),
                (7, field(&["unused", "Value"])),
                (10, field(&["rates", "UseContext"])),
                (
                    11,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::CurrentDateTime,
                    },
                ),
                (
                    12,
                    Node::If {
                        condition: 10,
                        then: 11,
                        else_: 5,
                    },
                ),
            ]),
        },
        root: Scope {
            target_field: "Result".into(),
            bindings: vec![bind("token", 1)],
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![bind("Id", 2), bind("Text", 3), bind("Maybe", 4)],
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    if bad == "mapping" {
        result.graph.nodes.insert(
            8,
            Node::Const {
                value: Value::Int(0),
            },
        );
        result.graph.nodes.insert(
            9,
            Node::Call {
                function: "divide".into(),
                args: vec![8, 8],
            },
        );
        result.extra_targets[1].root.bindings[0].node = 9;
    }
    result
}

fn cases() -> Vec<Json> {
    let case = |name, target, primary: &[u8], inputs: Vec<(&str, &[u8])>, legacy| {
        json!({
            "name": name, "target": target, "source": primary,
            "inputs": inputs.iter().map(|(name, bytes)| json!({"name": name,"bytes":bytes})).collect::<Vec<_>>(), "legacy":legacy,
        })
    };
    let all = || vec![("unused", UNUSED.as_bytes()), ("rates", RATES.as_bytes())];
    let mut cases = vec![
        case("primary", "primary", SOURCE.as_bytes(), vec![], false),
        case(
            "audit",
            "audit",
            SOURCE.as_bytes(),
            vec![("rates", RATES.as_bytes())],
            false,
        ),
        case(
            "broken",
            "broken",
            SOURCE.as_bytes(),
            vec![("unused", UNUSED.as_bytes())],
            false,
        ),
        case("all", "all", SOURCE.as_bytes(), all(), false),
        case("combined", "combined", SOURCE.as_bytes(), all(), false),
        case(
            "missing-combined",
            "combined",
            b"malformed",
            vec![("rates", RATES.as_bytes())],
            false,
        ),
        case(
            "bad-combined-second",
            "combined",
            SOURCE.as_bytes(),
            vec![("rates", RATES.as_bytes()), ("unused", b"<Unused>")],
            false,
        ),
        case("old-singular", "primary", SOURCE.as_bytes(), all(), true),
        case("missing", "audit", b"malformed", vec![], false),
        case(
            "unused-malformed",
            "primary",
            SOURCE.as_bytes(),
            vec![("unused", b"malformed")],
            false,
        ),
        case(
            "duplicate",
            "audit",
            SOURCE.as_bytes(),
            vec![("rates", b"malformed"), ("rates", b"malformed")],
            false,
        ),
        case("unknown-target", "missing", b"malformed", vec![], false),
        case("bad-primary", "primary", b"<Orders>", vec![], false),
        case(
            "bad-rates",
            "audit",
            SOURCE.as_bytes(),
            vec![("rates", b"<Rates>")],
            false,
        ),
        case(
            "bad-unused",
            "broken",
            SOURCE.as_bytes(),
            vec![("unused", b"<Unused>")],
            false,
        ),
        case(
            "wrong-case",
            "audit",
            SOURCE.as_bytes(),
            vec![("Rates", RATES.as_bytes())],
            false,
        ),
        case("bad-primary-utf8", "primary", b"\xff", vec![], false),
        case("input-size", "primary", b"", vec![], false),
        case("output-size", "primary", b"", vec![], false),
        case(
            "doctype",
            "primary",
            b"<!DOCTYPE Orders [<!ENTITY x SYSTEM 'file:///not-opened'>]><Orders>&x;</Orders>",
            vec![],
            false,
        ),
        case(
            "bad-rates-utf8",
            "audit",
            SOURCE.as_bytes(),
            vec![("rates", b"\xff")],
            false,
        ),
        case(
            "wrong-namespace",
            "primary",
            SOURCE.replace("urn:orders", "urn:wrong").as_bytes(),
            vec![],
            false,
        ),
        case(
            "trailing",
            "primary",
            format!("{SOURCE}<extra/>").as_bytes(),
            vec![],
            false,
        ),
        case(
            "count",
            "audit",
            b"malformed",
            vec![("rates", b"malformed"); 4096],
            false,
        ),
    ];
    let deep = format!(
        "{}{}",
        "<x>".repeat(ir::MAX_STRUCTURED_XML_DEPTH + 1),
        "</x>".repeat(ir::MAX_STRUCTURED_XML_DEPTH + 1)
    );
    cases.push(case("depth", "primary", deep.as_bytes(), vec![], false));
    cases.push(case(
        "namespace-alias",
        "primary",
        SOURCE
            .replace("o:", "p:")
            .replace("xmlns:o", "xmlns:p")
            .as_bytes(),
        vec![],
        false,
    ));
    cases.push(case(
        "line-endings",
        "primary",
        SOURCE.replace("nut", "first&#13;&#10;last").as_bytes(),
        vec![],
        false,
    ));
    cases.push(case(
        "recovery",
        "primary",
        SOURCE.as_bytes(),
        vec![],
        false,
    ));
    cases.push(case(
        "corrected-retry",
        "audit",
        SOURCE.as_bytes(),
        vec![("rates", RATES.as_bytes())],
        false,
    ));
    cases.push(case(
        "audit-context",
        "audit",
        SOURCE.as_bytes(),
        vec![("rates", RATES.replace("false", "true").as_bytes())],
        false,
    ));
    cases
}

#[test]
fn selected_xml_emission_requires_the_proved_static_structured_family() -> TestResult<()> {
    let p = project("good");
    let program = codegen::lower(&p)?;
    let plan = codegen::prepare_boundary_emission(&program, codegen::CompanionSelection::Complete)?;
    let inputs = plan.xml().unwrap().selected_inputs().unwrap();
    assert!(
        inputs
            .primary
            .entries
            .iter()
            .all(|entry| entry.availability == codegen::NamedInputAvailability::Optional)
    );
    assert_eq!(
        inputs.named_targets[0].catalogue.entries[0].availability,
        codegen::NamedInputAvailability::RequiredLegacy
    );
    let mut observed = program.clone();
    observed.xml_boundary.as_mut().unwrap().input = codegen::XmlInputPolicy {
        allow_inactive_root_type_members: true,
        root_view_policy: true,
    };
    // Invalid or observed profiles must never acquire the new API merely from XML identity.
    if let Ok(plan) =
        codegen::prepare_boundary_emission(&observed, codegen::CompanionSelection::Complete)
    {
        assert!(plan.xml().is_none_or(|xml| xml.selected_inputs().is_none()));
    }
    let typed =
        codegen::prepare_boundary_emission(&program, codegen::CompanionSelection::TypedOnly)?;
    assert!(typed.xml().is_none());
    let mut rules = p;
    rules.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: vec!["Row".into()],
        },
        selection: mapping::FailureSelection::All,
        message: Some(5),
    });
    let rules = codegen::lower(&rules)?;
    let plan = codegen::prepare_boundary_emission(&rules, codegen::CompanionSelection::Complete)?;
    assert_eq!(
        plan.xml()
            .unwrap()
            .selected_inputs()
            .unwrap()
            .primary
            .entries[0]
            .availability,
        codegen::NamedInputAvailability::RequiredLegacy,
        "global failure messages retain their required source"
    );
    Ok(())
}

fn write_artifacts(root: &Path, artifacts: codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.into_files() {
        let path = root.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, file.contents)?;
    }
    Ok(())
}
fn artifact_fingerprints(root: &Path) -> TestResult<Vec<(String, usize, String)>> {
    use sha2::{Digest, Sha256};
    Ok(artifact_files(root)?
        .into_iter()
        .map(|(path, bytes)| (path, bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
        .collect())
}
fn recorded(command: &mut Command, root: &Path, name: &str) -> TestResult<Output> {
    let result = command.isolated_output()?;
    std::fs::write(root.join(format!("{name}.stdout")), &result.stdout)?;
    std::fs::write(root.join(format!("{name}.stderr")), &result.stderr)?;
    assert!(
        result.status.success(),
        "{name}: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(result)
}
fn native(project: &Project, case: &Json) -> TestResult<Vec<(String, Vec<u8>)>> {
    let mut source: Vec<u8> = serde_json::from_value(case["source"].clone())?;
    if case["name"] == "input-size" {
        source = vec![b'x'; 64 * 1024 * 1024 + 1];
    }
    if case["name"] == "output-size" {
        source = format!("<o:Orders xmlns:o=\"urn:orders\"><o:Row><Text><![CDATA[{}]]></Text></o:Row></o:Orders>", "&".repeat(14 * 1024 * 1024)).into_bytes();
    }
    let input_values: Vec<(String, Vec<u8>)> = case["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            Ok((
                i["name"].as_str().unwrap().to_string(),
                serde_json::from_value(i["bytes"].clone())?,
            ))
        })
        .collect::<TestResult<_>>()?;
    let paths: Vec<_> = (0..input_values.len())
        .map(|i| PathBuf::from(format!("/xml-selected/{i}.xml")))
        .collect();
    let inputs = input_values
        .iter()
        .zip(&paths)
        .map(|((name, bytes), path)| {
            cli::NamedPayloadInput::new(name, cli::PayloadDocument::new(path, bytes).unwrap())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut project = project.clone();
    // Synthetic output identities only; the payload API performs no file I/O.
    for output in &mut project.extra_targets {
        output.path = Some(format!("/xml-selected/{}.xml", output.name));
    }
    let source = cli::PayloadDocument::new(Path::new("/xml-selected/input.xml"), &source)?;
    let mut options = cli::PayloadRunOptions::new(source)
        .with_extra_sources(&inputs)
        .with_output_path(Path::new("/xml-selected/output.xml"))
        .with_current_datetime("2026-10-25T00:00:00Z")?;
    let target = case["target"].as_str().unwrap();
    if target != "all" {
        options = options.with_target(if target == "primary" {
            engine::TargetSelection::Primary
        } else {
            engine::TargetSelection::Named(target)
        });
    }
    let output = cli::run_project_value_payloads(
        &project,
        Path::new("/xml-selected/mapping.json"),
        &options,
    )?;
    Ok(output
        .artifacts
        .into_iter()
        .map(|a| (a.target, a.bytes))
        .collect())
}

fn exercise(language: &str) -> TestResult<()> {
    let directory = TempDir::new(&format!("selected-xml-{language}"))?;
    let root = &directory.0;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    std::fs::write(root.join("cases.json"), serde_json::to_vec(&cases())?)?;
    for variant in ["good", "output", "mapping"] {
        let dir = root.join(variant);
        std::fs::create_dir(&dir)?;
        let project = project(variant);
        assert!(engine::validate(&project).is_empty(), "{variant}");
        std::fs::write(
            dir.join("project.json"),
            mapping::project_file::encode_pretty(&project)?,
        )?;
        let program = codegen::lower(&project)?;
        let generated = dir.join("generated");
        if language == "rust" {
            write_artifacts(
                &generated,
                codegen_rust::emit(
                    &program,
                    &codegen_rust::Options {
                        package_name: "xml-selected-mapping".into(),
                        runtime_dependency: codegen_rust::RuntimeDependency::Path(
                            runtime.display().to_string(),
                        ),
                    },
                )?,
            )?;
        } else {
            write_artifacts(&generated, codegen_csharp::emit(&program)?)?;
        }
        let original = artifact_fingerprints(&generated)?;
        let host = dir.join("host");
        std::fs::create_dir_all(host.join("src"))?;
        let output = if language == "rust" {
            std::fs::write(
                host.join("src/main.rs"),
                include_str!("selected_xml_bytes/host.rs.txt"),
            )?;
            std::fs::write(
                host.join("Cargo.toml"),
                format!(
                    "[package]\nname=\"xml-selected-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nxml-selected-mapping={{path=\"../generated\"}}\ncodegen-runtime={{path={runtime:?}}}\nserde_json=\"=1.0.150\"\n"
                ),
            )?;
            let mut command = Command::new("cargo");
            command
                .args(["run", "--quiet", "--offline", "--jobs", "2", "--"])
                .arg(root.join("cases.json"))
                .current_dir(&host)
                .env("RUSTFLAGS", "-D warnings")
                .env("CARGO_TARGET_DIR", root.join("rust-host-target"));
            recorded(&mut command, &dir, "host")?
        } else {
            std::fs::write(
                host.join("Program.cs"),
                include_str!("selected_xml_bytes/host.cs.txt"),
            )?;
            std::fs::write(
                host.join("Host.csproj"),
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
            )?;
            std::fs::write(
                dir.join("NuGet.Config"),
                "<configuration><packageSources><clear /></packageSources></configuration>",
            )?;
            let mut command = dotnet_command(&dir);
            command
                .args([
                    "build",
                    "host/Host.csproj",
                    "--nologo",
                    "--verbosity",
                    "quiet",
                    "-m:1",
                    "/p:UseSharedCompilation=false",
                    "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                ])
                .arg("--artifacts-path")
                .arg(dir.join("artifacts"))
                .current_dir(&dir);
            recorded(&mut command, &dir, "build")?;
            let mut command = dotnet_command(&dir);
            command
                .arg(dir.join("artifacts/bin/Host/debug/Host.dll"))
                .arg(root.join("cases.json"))
                .current_dir(&dir);
            recorded(&mut command, &dir, "host")?
        };
        assert_eq!(
            original,
            artifact_fingerprints(&generated)?,
            "generated source is immutable"
        );
        let rows: Vec<Json> = String::from_utf8(output.stdout)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(rows.len(), cases().len());
        for (row, case) in rows.iter().zip(cases()) {
            let name = case["name"].as_str().unwrap();
            assert_eq!(row["case"], name);
            let success = matches!(
                name,
                "primary"
                    | "audit"
                    | "namespace-alias"
                    | "line-endings"
                    | "recovery"
                    | "corrected-retry"
                    | "audit-context"
                    | "combined"
            ) || variant == "good"
                && matches!(name, "all" | "old-singular" | "broken");
            if success {
                assert_eq!(row["kind"], "ok", "{language}/{variant}/{name}: {row}");
                let mut native_case = case.clone();
                if name == "old-singular" {
                    native_case["target"] = json!("all");
                }
                let expected = native(&project, &native_case)?;
                let expected: Vec<_> = expected
                    .into_iter()
                    .take(if name == "old-singular" {
                        1
                    } else {
                        usize::MAX
                    })
                    .map(|(_, bytes)| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
                    .collect();
                assert_eq!(
                    row["documents"],
                    json!(expected),
                    "{language}/{variant}/{name}"
                );
                if name == "primary" {
                    let bytes = native(&project, &case)?.remove(0).1;
                    let text = std::str::from_utf8(&bytes)?;
                    let parsed = roxmltree::Document::parse(text)?;
                    assert_eq!(
                        parsed.root_element().tag_name().namespace(),
                        Some("urn:orders")
                    );
                    assert_eq!(parsed.root_element().attribute("token"), Some("A&B"));
                    assert_eq!(
                        parsed
                            .descendants()
                            .filter(|n| n.has_tag_name(("urn:orders", "Row")))
                            .count(),
                        2
                    );
                    assert_eq!(
                        parsed
                            .descendants()
                            .filter(|n| n.has_tag_name("Maybe"))
                            .count(),
                        1
                    );
                    assert!(text.contains("bolt😀&lt;&amp;&gt;"));
                }
            } else {
                let mut native_case = case.clone();
                if name == "old-singular" {
                    native_case["target"] = json!("all");
                }
                let native_result = native(&project, &native_case);
                // The native payload API ignores unused declared sources. The
                // selected companion instead requires the exact selected input
                // set and rejects an extra name without parsing its document.
                if name == "unused-malformed" {
                    assert!(
                        native_result.is_ok(),
                        "native selection must not read an unused malformed source"
                    );
                } else {
                    std::fs::write(
                        dir.join(format!("native-{name}.txt")),
                        match &native_result {
                            Ok(outputs) => {
                                format!("unexpected success: {} documents", outputs.len())
                            }
                            Err(error) => format!("{error:#}"),
                        },
                    )?;
                    assert!(
                        native_result.is_err(),
                        "native negative control {variant}/{name} unexpectedly succeeded"
                    );
                }
                assert!(row.get("documents").is_none(), "no partial output: {row}");
                let kind = match name {
                    "input-size" | "output-size" => "DocumentLimit",
                    "bad-primary-utf8" | "bad-rates-utf8" => "Utf8",
                    "missing" | "duplicate" | "unused-malformed" | "unknown-target"
                    | "wrong-case" => "Mapping",
                    "missing-combined" => "Mapping",
                    "broken" | "all" | "old-singular" if variant == "mapping" => "Mapping",
                    "broken" | "all" | "old-singular" => "Output",
                    _ => "Input",
                };
                assert_eq!(row["kind"], kind, "{language}/{variant}/{name}: {row}");
                if matches!(
                    name,
                    "bad-primary" | "bad-primary-utf8" | "wrong-namespace" | "trailing" | "depth"
                ) {
                    assert_eq!(row["input"], "primary");
                }
                if matches!(name, "bad-rates" | "bad-rates-utf8") {
                    assert_eq!(row["input"], "rates");
                    assert_eq!(row["inputIndex"], 0);
                }
                if matches!(name, "bad-unused" | "bad-combined-second") {
                    assert_eq!(row["input"], "unused");
                    assert_eq!(row["inputIndex"], 1);
                }
                if name == "input-size" {
                    assert_eq!(row["input"], "primary");
                }
                if name == "output-size" {
                    assert_eq!(row["output"], "primary");
                }
                if kind == "Output" {
                    assert_eq!(row["output"], "broken");
                    assert_eq!(row["outputIndex"], 1);
                }
                if name == "count" {
                    assert!(
                        row["detail"]
                            .as_str()
                            .unwrap()
                            .contains("xml_input_artifact_count")
                    );
                }
                assert_eq!(row["causePreserved"], true);
            }
        }
    }
    Ok(())
}

#[test]
fn selected_xml_bytes_rust_native_parity() -> TestResult<()> {
    exercise("rust")
}
#[test]
fn selected_xml_bytes_csharp_native_parity() -> TestResult<()> {
    exercise("csharp")
}
