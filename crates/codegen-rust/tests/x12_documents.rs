//! Independently authored physical documents. No native reader/writer or mapping
//! interpreter constructs the expected output. The generated libraries are built
//! together and their public document APIs run in a separate native process.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use codegen::{X12BoundaryOptions, X12BoundaryPolicy, X12EnvelopeProfile};
use codegen_rust::{Options, RuntimeDependency, emit_with_x12};
use ir::{ItemCountRange, ScalarType, SchemaNode};
use mapping::{Project, Scope, ScopeConstruction};
use serde_json::{Value as Json, json};

#[path = "../src/tests/host_target.rs"]
#[allow(dead_code)]
mod host_target;
use host_target::GeneratedHostCommand;
#[path = "x12_documents/physical_roles.rs"]
mod physical_roles;
#[path = "x12_documents/review.rs"]
mod review;

fn fields(id: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        id,
        (1..=count)
            .map(|n| SchemaNode::scalar(format!("E{n:02}"), ScalarType::String))
            .collect(),
    )
}

fn envelope(profile: X12EnvelopeProfile, version: &str) -> SchemaNode {
    let mut isa = fields("ISA", 16);
    let mut gs = fields("GS", 8);
    if let ir::SchemaKind::Group { children, .. } = &mut isa.kind {
        children[11].fixed = Some(version.into());
    }
    if let ir::SchemaKind::Group { children, .. } = &mut gs.kind {
        children[7].fixed = Some(format!("{version}0"));
    }
    let st = fields("ST", 2);
    let se = fields("SE", 2);
    let ge = fields("GE", 2);
    let iea = fields("IEA", 2);
    let reference = SchemaNode::group(
        "REF",
        vec![
            SchemaNode::group(
                "E01",
                vec![
                    SchemaNode::scalar("A", ScalarType::String),
                    SchemaNode::scalar("B", ScalarType::String),
                ],
            ),
            SchemaNode::scalar("E02", ScalarType::Int),
            SchemaNode::scalar("E03", ScalarType::Float),
        ],
    );
    let mut notes = fields("NTE", 1).repeating();
    notes.item_count_range = ItemCountRange::new(0, Some(3));
    match profile {
        X12EnvelopeProfile::SingleTransaction => {
            SchemaNode::group("Envelope", vec![isa, gs, st, reference, notes, se, ge, iea])
        }
        X12EnvelopeProfile::GroupedTransactions => SchemaNode::group(
            "Envelope",
            vec![
                isa,
                SchemaNode::group(
                    "Groups",
                    vec![
                        gs,
                        SchemaNode::group("Messages", vec![st, reference, notes, se]).repeating(),
                        ge,
                    ],
                )
                .repeating(),
                iea,
            ],
        ),
        X12EnvelopeProfile::MapForceInterchanges => SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::group(
                    "Interchanges",
                    vec![
                        isa.repeating(),
                        SchemaNode::group(
                            "Groups",
                            vec![
                                gs.repeating(),
                                SchemaNode::group(
                                    "Messages",
                                    vec![st, reference, notes, se.repeating()],
                                )
                                .repeating(),
                                ge.repeating(),
                            ],
                        )
                        .repeating(),
                        iea.repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
    }
}

fn project(schema: SchemaNode) -> Project {
    Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Default::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    }
}

fn segment(values: &[&str]) -> Json {
    let mut map = serde_json::Map::new();
    for (index, value) in values.iter().enumerate() {
        map.insert(format!("E{:02}", index + 1), json!(value));
    }
    Json::Object(map)
}

// Wire fragments are literal X12, not serialized typed instances. Counts and
// controls are chosen independently of any codec or generated output.
fn fixture(profile: X12EnvelopeProfile, version: &str) -> (Json, String) {
    let modern = if version == "00401" { "U" } else { "^" };
    let mut wire = String::new();
    let mut interchanges = Vec::new();
    let mut transaction = 7;
    for interchange in 0..if profile == X12EnvelopeProfile::MapForceInterchanges {
        2
    } else {
        1
    } {
        let control = if interchange == 0 {
            "000000123"
        } else {
            "000000124"
        };
        wire.push_str(&format!("ISA*00*          *00*          *ZZ*ALPHA          *ZZ*BETA           *300615*0945*{modern}*{version}*{control}*0*T*:~\n"));
        let isa = segment(&[
            "00",
            "          ",
            "00",
            "          ",
            "ZZ",
            "ALPHA          ",
            "ZZ",
            "BETA           ",
            "300615",
            "0945",
            modern,
            version,
            control,
            "0",
            "T",
            ":",
        ]);
        let mut groups = Vec::new();
        for group in 0..if profile == X12EnvelopeProfile::SingleTransaction {
            1
        } else {
            2
        } {
            let group_control = if group == 0 { "7" } else { "8" };
            let group_version = format!("{version}0");
            wire.push_str(&format!(
                "GS*PO*ALPHA*BETA*20300615*0945*{group_control}*X*{group_version}~\n"
            ));
            let gs = segment(&[
                "PO",
                "ALPHA",
                "BETA",
                "20300615",
                "0945",
                group_control,
                "X",
                &group_version,
            ]);
            let mut messages = Vec::new();
            for _ in 0..if profile == X12EnvelopeProfile::SingleTransaction {
                1
            } else {
                2
            } {
                let number = format!("{transaction:04}");
                transaction += 1;
                wire.push_str(&format!(
                    "ST*850*{number}~\nREF*AA:BB*3*1.25~\nNTE*one~\nNTE*two~\nSE*5*{number}~\n"
                ));
                let mut message = json!({"ST":segment(&["850", &number]),"REF":{"E01":{"A":"AA","B":"BB"},"E02":3,"E03":1.25},"NTE":[{"E01":"one"},{"E01":"two"}],"SE":segment(&["5",&number])});
                if profile == X12EnvelopeProfile::MapForceInterchanges {
                    message["SE"] = json!([message["SE"].clone()]);
                }
                messages.push(message);
            }
            let count = if profile == X12EnvelopeProfile::SingleTransaction {
                "1"
            } else {
                "2"
            };
            wire.push_str(&format!("GE*{count}*{group_control}~\n"));
            let ge = segment(&[count, group_control]);
            groups.push(if profile == X12EnvelopeProfile::MapForceInterchanges {
                json!({"GS":[gs],"Messages":messages,"GE":[ge]})
            } else {
                json!({"GS":gs,"Messages":messages,"GE":ge})
            });
        }
        let count = if profile == X12EnvelopeProfile::SingleTransaction {
            "1"
        } else {
            "2"
        };
        wire.push_str(&format!("IEA*{count}*{control}~\n"));
        let iea = segment(&[count, control]);
        if profile == X12EnvelopeProfile::SingleTransaction {
            let group = &groups[0];
            let message = &group["Messages"][0];
            return (
                json!({"ISA":isa,"GS":group["GS"],"ST":message["ST"],"REF":message["REF"],"NTE":message["NTE"],"SE":message["SE"],"GE":group["GE"],"IEA":iea}),
                wire,
            );
        }
        interchanges.push(if profile == X12EnvelopeProfile::MapForceInterchanges {
            json!({"ISA":[isa],"Groups":groups,"IEA":[iea]})
        } else {
            json!({"ISA":isa,"Groups":groups,"IEA":iea})
        });
    }
    (
        if profile == X12EnvelopeProfile::MapForceInterchanges {
            json!({"Interchanges":interchanges})
        } else {
            interchanges.remove(0)
        },
        wire,
    )
}

/// Evidence under `FERRULE_X12_TEST_EVIDENCE_DIR` is retained; otherwise it is removed on drop,
/// including after a panic.
struct Evidence(PathBuf);

impl std::ops::Deref for Evidence {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Evidence {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Evidence {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_X12_TEST_EVIDENCE_DIR").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn evidence() -> Result<Evidence, Box<dyn Error>> {
    let parent = std::env::var_os("FERRULE_X12_TEST_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    fs::create_dir_all(&parent)?;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = parent.join(format!(
        "x12-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root)?;
    Ok(Evidence(root))
}

fn artifacts(
    root: &Path,
    name: &str,
    program: &codegen::Program,
    policy: &X12BoundaryPolicy,
) -> Result<(), Box<dyn Error>> {
    let options = Options {
        package_name: name.into(),
        runtime_dependency: RuntimeDependency::Path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .display()
                .to_string(),
        ),
    };
    let files = emit_with_x12(program, &options, policy)?;
    for file in files.files() {
        let path = root.join(name).join(file.path.as_str());
        fs::create_dir_all(path.parent().ok_or("parent")?)?;
        fs::write(path, &file.contents)?;
    }
    Ok(())
}

#[test]
fn generated_primary_x12_documents_compile_and_run_all_profiles_and_directions()
-> Result<(), Box<dyn Error>> {
    let root = evidence()?;
    let mut manifest = String::from(
        "[package]\nname = \"x12-public-host\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[[bin]]\nname = \"x12-public-host\"\npath = \"../src/main.rs\"\n[workspace]\n[dependencies]\n",
    );
    manifest.push_str(&format!("codegen-runtime = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\", \"float_roundtrip\"] }}\n", Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime").display().to_string().replace('\\', "/")));
    let mut host = String::from(include_str!("x12_documents/host.rs.txt"));
    host.push_str("\nfn main() -> Result<(), Box<dyn std::error::Error>> {\n");
    let mut cases = 0;
    for (label, envelope_profile) in [
        ("single", X12EnvelopeProfile::SingleTransaction),
        ("grouped", X12EnvelopeProfile::GroupedTransactions),
        ("interchanges", X12EnvelopeProfile::MapForceInterchanges),
    ] {
        for version in ["00401", "00501", "00604"] {
            let schema = envelope(envelope_profile, version);
            let project = project(schema);
            fs::write(
                root.join(format!("{label}-{version}.project.json")),
                serde_json::to_vec_pretty(&project)?,
            )?;
            let program = codegen::lower(&project)?;
            let (values, wire) = fixture(envelope_profile, version);
            let json = serde_json::to_string_pretty(&values)? + "\n";
            fs::write(root.join(format!("{label}-{version}.json")), &json)?;
            fs::write(root.join(format!("{label}-{version}.x12")), &wire)?;
            for (direction, source_x12, target_x12) in [
                ("x12_to_x12", true, true),
                ("x12_to_json", true, false),
                ("json_to_x12", false, true),
            ] {
                let name = format!("{label}_{version}_{direction}");
                let selected = X12BoundaryOptions {
                    envelope_profile,
                    interchange_version: Some(version.into()),
                    ..Default::default()
                };
                let policy = X12BoundaryPolicy {
                    source: source_x12.then(|| selected.clone()),
                    target: target_x12.then_some(selected),
                };
                artifacts(&root, &name, &program, &policy)?;
                manifest.push_str(&format!(
                    "{name} = {{ path = {:?} }}\n",
                    format!("../{name}")
                ));
                host.push_str(&format!("    physical({name}::execute_{direction}, {name}::execute_{direction}_bytes, {name}::execute_{direction}_with_context, {name}::execute_{direction}_bytes_with_context, include_str!({:?}), include_str!({:?}))?;\n", format!("../{label}-{version}.{}", if source_x12 { "x12" } else { "json" }), format!("../{label}-{version}.{}", if target_x12 { "x12" } else { "json" })));
                if direction == "x12_to_x12" {
                    host.push_str(&format!("    let input = include_str!({:?});\n    let parsed = {name}::parse_x12(input)?;\n    assert_eq!({name}::parse_x12_bytes(input.as_bytes())?, parsed);\n    let before = parsed.clone();\n    assert_eq!({name}::serialize_x12(&parsed)?, input);\n    assert_eq!({name}::serialize_x12_bytes(&parsed)?, input.as_bytes());\n    assert_eq!({name}::serialize_x12_with_context(&parsed, &context())?, input);\n    assert_eq!({name}::serialize_x12_bytes_with_context(&parsed, &context())?, input.as_bytes());\n    assert_eq!(parsed, before);\n", format!("../{label}-{version}.x12")));
                    host.push_str(&format!("    malformed({name}::execute_x12_to_x12, {name}::execute_x12_to_x12_bytes, include_str!({:?}))?;\n", format!("../{label}-{version}.x12")));
                }
                cases += 1;
            }
            let selected = X12BoundaryOptions {
                envelope_profile,
                interchange_version: Some(version.into()),
                autocomplete: Some(mapping::X12Autocomplete {
                    request_acknowledgement: false,
                    transaction_set: Some("850".into()),
                }),
                ..Default::default()
            };
            let name = format!("{label}_{version}_completion");
            artifacts(
                &root,
                &name,
                &program,
                &X12BoundaryPolicy {
                    source: None,
                    target: Some(selected),
                },
            )?;
            manifest.push_str(&format!(
                "{name} = {{ path = {:?} }}\n",
                format!("../{name}")
            ));
            let mut incomplete = values.clone();
            incomplete_headers(&mut incomplete);
            fs::write(
                root.join(format!("{label}-{version}-incomplete.json")),
                serde_json::to_string_pretty(&incomplete)?,
            )?;
            // Native completion supplies GS time with seconds, independently of
            // the preserved four-digit time in the already complete fixture.
            fs::write(
                root.join(format!("{label}-{version}-completed.x12")),
                wire.replace("*20300615*0945*", "*20300615*094500*"),
            )?;
            host.push_str(&format!("    completion({name}::execute_json_to_x12, {name}::execute_json_to_x12_bytes, {name}::execute_json_to_x12_with_context, {name}::execute_json_to_x12_bytes_with_context, include_str!({:?}), include_str!({:?}))?;\n", format!("../{label}-{version}-incomplete.json"), format!("../{label}-{version}-completed.x12")));
        }
    }
    option_libraries(&root, &mut manifest, &mut host)?;
    review::add_cases(&root, &mut manifest, &mut host)?;
    physical_roles::add_cases(&root, &mut manifest, &mut host)?;
    host.push_str("    limits(single_00401_x12_to_x12::execute_x12_to_x12_bytes, single_00401_x12_to_x12::parse_x12, single_00401_x12_to_x12::serialize_x12, include_str!(\"../single-00401.x12\"))?;\n    println!(\"27 direction/profile/version libraries, 9 completion libraries; real text/byte/context, malformed and budget calls passed\");\n    Ok(())\n}\n");
    assert_eq!(cases, 27);
    let harness = root.join("Harness");
    fs::create_dir(&harness)?;
    fs::write(harness.join("Cargo.toml"), manifest)?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/main.rs"), host)?;
    let result = Command::new(env!("CARGO"))
        .args(["run", "--offline", "--jobs", "1"])
        .current_dir(&harness)
        .generated_host_output(&root)?;
    fs::write(root.join("host.stdout"), &result.stdout)?;
    fs::write(root.join("host.stderr"), &result.stderr)?;
    assert!(
        result.status.success(),
        "public X12 host failed; complete evidence at {}\n{}\n{}",
        root.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

fn option_libraries(
    root: &Path,
    manifest: &mut String,
    host: &mut String,
) -> Result<(), Box<dyn Error>> {
    let mut schema = envelope(X12EnvelopeProfile::SingleTransaction, "00501");
    if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
        let notes = children
            .iter_mut()
            .find(|node| node.name == "NTE")
            .ok_or("NTE")?;
        if let ir::SchemaKind::Group { children, .. } = &mut notes.kind {
            children[0].repeating = true;
        }
    }
    let program = codegen::lower(&project(schema))?;
    let syntax = mapping::X12Separators {
        element: '|',
        component: '>',
        segment: '!',
        repetition: Some('~'),
        release: None,
    };
    let (mut values, original) = fixture(X12EnvelopeProfile::SingleTransaction, "00501");
    values["ISA"]["E11"] = json!("~");
    values["ISA"]["E16"] = json!(">");
    values["NTE"][0]["E01"] = json!(["one", "uno"]);
    values["NTE"][1]["E01"] = json!(["two"]);
    let wire: String = original
        .chars()
        .map(|c| match c {
            '*' => '|',
            ':' => '>',
            '~' => '!',
            '^' => '~',
            other => other,
        })
        .collect();
    let wire = wire.replace("NTE|one!", "NTE|one~uno!");
    let input = wire
        .replace("REF|AA>BB|3|1.25!", "REF|AA>BB|3|125!\nZZZ|ignored!")
        .replace("SE|5|", "SE|6|");
    let mut parsed = values.clone();
    parsed["SE"]["E01"] = json!("6");
    let source = X12BoundaryOptions {
        separators: Some(syntax),
        lenient_segments: true,
        implied_decimals: vec![
            mapping::EdiImpliedDecimal::new(vec!["REF".into(), "E03".into()], 2)
                .ok_or("implied")?,
        ],
        constraints: vec![
            mapping::EdiValueConstraint::new(
                vec!["REF".into(), "E02".into()],
                1,
                1,
                vec!["3".into()],
            )
            .ok_or("constraint")?,
        ],
        ..Default::default()
    };
    artifacts(
        root,
        "options_reader",
        &program,
        &X12BoundaryPolicy {
            source: Some(source),
            target: None,
        },
    )?;
    let target = X12BoundaryOptions {
        separators: Some(syntax),
        lexical_formats: vec![
            mapping::EdiLexicalFormat::new(
                vec!["GS".into(), "E04".into()],
                mapping::EdiLexicalKind::CompactDate8,
            )
            .ok_or("date")?,
            mapping::EdiLexicalFormat::new(
                vec!["REF".into(), "E03".into()],
                mapping::EdiLexicalKind::Decimal { max_chars: 8 },
            )
            .ok_or("decimal")?,
        ],
        ..Default::default()
    };
    artifacts(
        root,
        "options_writer",
        &program,
        &X12BoundaryPolicy {
            source: None,
            target: Some(target),
        },
    )?;
    for name in ["options_reader", "options_writer"] {
        manifest.push_str(&format!(
            "{name} = {{ path = {:?} }}\n",
            format!("../{name}")
        ));
    }
    fs::write(root.join("options-input.x12"), input)?;
    fs::write(
        root.join("options-parsed.json"),
        serde_json::to_string_pretty(&parsed)? + "\n",
    )?;
    values["GS"]["E04"] = json!("2030-06-15");
    fs::write(
        root.join("options-input.json"),
        serde_json::to_string_pretty(&values)?,
    )?;
    fs::write(root.join("options-output.x12"), wire)?;
    host.push_str("    physical(options_reader::execute_x12_to_json, options_reader::execute_x12_to_json_bytes, options_reader::execute_x12_to_json_with_context, options_reader::execute_x12_to_json_bytes_with_context, include_str!(\"../options-input.x12\"), include_str!(\"../options-parsed.json\"))?;\n    physical(options_writer::execute_json_to_x12, options_writer::execute_json_to_x12_bytes, options_writer::execute_json_to_x12_with_context, options_writer::execute_json_to_x12_bytes_with_context, include_str!(\"../options-input.json\"), include_str!(\"../options-output.x12\"))?;\n    assert!(matches!(options_reader::execute_x12_to_json(&include_str!(\"../options-input.x12\").replace(\"REF|AA>BB|3|\", \"REF|AA>BB|4|\")), Err(Failure::Value { reason: \"lexical length or code-list constraint failed\", .. })));\n");
    let mut failing = codegen::lower(&project(envelope(
        X12EnvelopeProfile::SingleTransaction,
        "00401",
    )))?;
    failing.expressions.push(codegen::ExpressionNode {
        id: 1,
        expression: codegen::Expression::Const {
            value: ir::Value::String("authored stop".into()),
        },
    });
    failing.failure_rules.push(codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(Vec::new())),
        selection: codegen::FailureSelection::All,
        message: Some(1),
    });
    artifacts(
        root,
        "failure_boundary",
        &failing,
        &X12BoundaryPolicy {
            source: None,
            target: Some(Default::default()),
        },
    )?;
    manifest.push_str("failure_boundary = { path = \"../failure_boundary\" }\n");
    host.push_str("    assert!(matches!(failure_boundary::execute_json_to_x12(include_str!(\"../single-00401.json\")), Err(Failure::Mapping(codegen_runtime::RuntimeError::MappingFailure { rule: 1, message: Some(message) })) if message == \"authored stop\"));\n    assert!(matches!(failure_boundary::execute_json_to_x12_bytes(b\"not JSON\"), Err(Failure::Json(_))));\n");
    let mut schema = envelope(X12EnvelopeProfile::MapForceInterchanges, "00501");
    fn numeric(node: &mut SchemaNode) {
        if let ir::SchemaKind::Group { children, .. } = &mut node.kind {
            for (at, child) in children.iter_mut().enumerate() {
                if matches!(
                    (node.name.as_str(), at),
                    ("ISA", 12) | ("GS", 5) | ("SE", 0) | ("GE", 0 | 1) | ("IEA", 0 | 1)
                ) {
                    child.kind = ir::SchemaKind::Scalar {
                        ty: if node.name == "ISA" {
                            ScalarType::Float
                        } else {
                            ScalarType::Int
                        },
                    };
                }
                numeric(child);
            }
        }
    }
    numeric(&mut schema);
    let program = codegen::lower(&project(schema))?;
    let options = X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some("00501".into()),
        ..Default::default()
    };
    artifacts(
        root,
        "numeric_controls",
        &program,
        &X12BoundaryPolicy {
            source: Some(options.clone()),
            target: Some(options),
        },
    )?;
    manifest.push_str("numeric_controls = { path = \"../numeric_controls\" }\n");
    let (_, wire) = fixture(X12EnvelopeProfile::MapForceInterchanges, "00501");
    fs::write(
        root.join("numeric-controls.x12"),
        wire.replace("000000123", "100000123")
            .replace("000000124", "100000124"),
    )?;
    host.push_str("    physical(numeric_controls::execute_x12_to_x12, numeric_controls::execute_x12_to_x12_bytes, numeric_controls::execute_x12_to_x12_with_context, numeric_controls::execute_x12_to_x12_bytes_with_context, include_str!(\"../numeric-controls.x12\"), include_str!(\"../numeric-controls.x12\"))?;\n");
    Ok(())
}

fn incomplete_headers(value: &mut Json) {
    match value {
        Json::Object(fields) => {
            fields.remove("SE");
            fields.remove("GE");
            fields.remove("IEA");
            if let Some(isa) = fields.get_mut("ISA") {
                for value in if let Json::Array(items) = isa {
                    items.iter_mut().collect::<Vec<_>>()
                } else {
                    vec![isa]
                } {
                    if let Some(fields) = value.as_object_mut() {
                        fields.remove("E09");
                        fields.remove("E10");
                    }
                }
            }
            if let Some(gs) = fields.get_mut("GS") {
                for value in if let Json::Array(items) = gs {
                    items.iter_mut().collect::<Vec<_>>()
                } else {
                    vec![gs]
                } {
                    if let Some(fields) = value.as_object_mut() {
                        fields.remove("E04");
                        fields.remove("E05");
                    }
                }
            }
            if let Some(st) = fields.get_mut("ST").and_then(Json::as_object_mut) {
                st.remove("E01");
            }
            fields.values_mut().for_each(incomplete_headers);
        }
        Json::Array(items) => items.iter_mut().for_each(incomplete_headers),
        _ => {}
    }
}

#[test]
fn shared_admission_and_native_syntax_refuse_before_artifacts() -> Result<(), Box<dyn Error>> {
    let program = codegen::lower(&project(envelope(
        X12EnvelopeProfile::SingleTransaction,
        "00401",
    )))?;
    let options = Options {
        package_name: "refusal".into(),
        runtime_dependency: RuntimeDependency::Path("unused".into()),
    };
    assert!(matches!(
        emit_with_x12(&program, &options, &X12BoundaryPolicy::default()),
        Err(codegen_rust::X12EmitError::Policy(
            codegen::X12BoundaryPolicyError::NoX12Side
        ))
    ));
    let selected = X12BoundaryOptions {
        interchange_version: Some("00403".into()),
        ..Default::default()
    };
    assert!(matches!(
        emit_with_x12(
            &program,
            &options,
            &X12BoundaryPolicy {
                source: Some(selected),
                target: None
            }
        ),
        Err(codegen_rust::X12EmitError::Policy(_))
    ));
    let selected = X12BoundaryOptions {
        separators: Some(mapping::X12Separators {
            element: '*',
            component: ':',
            segment: '\n',
            repetition: Some('^'),
            release: None,
        }),
        ..Default::default()
    };
    assert!(matches!(
        emit_with_x12(
            &program,
            &options,
            &X12BoundaryPolicy {
                source: Some(selected),
                target: None
            }
        ),
        Err(codegen_rust::X12EmitError::UnsupportedNativeSetting { side: "source", .. })
    ));
    let mut named = program.clone();
    named.extra_sources.push(codegen::NamedSourceProgram {
        name: "lookup".into(),
        source: named.source.clone(),
        dynamic: None,
    });
    assert!(matches!(
        emit_with_x12(
            &named,
            &options,
            &X12BoundaryPolicy {
                source: Some(Default::default()),
                target: None
            }
        ),
        Err(codegen_rust::X12EmitError::Policy(
            codegen::X12BoundaryPolicyError::ProgramField {
                field: "extra_sources"
            }
        ))
    ));
    let mut named = program.clone();
    named.extra_targets.push(codegen::NamedTargetProgram {
        name: "other".into(),
        target: named.target.clone(),
        root: named.root.clone(),
    });
    assert!(matches!(
        emit_with_x12(
            &named,
            &options,
            &X12BoundaryPolicy {
                source: Some(Default::default()),
                target: None
            }
        ),
        Err(codegen_rust::X12EmitError::Policy(
            codegen::X12BoundaryPolicyError::ProgramField {
                field: "extra_targets"
            }
        ))
    ));
    Ok(())
}
