//! Public mixed XML mapping exceptions must precede final output-count admission.
use super::super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
#[path = "late_mapping_count/support.rs"]
mod support;
use support::*;
const APIS: [&str; 4] = ["text", "text-context", "bytes", "bytes-context"];
const PATH: &str = "same.xml";
static SERIAL: Mutex<()> = Mutex::new(());
fn mapping() -> Project {
    let mut value = project();
    let mut row = SchemaNode::group("Row", vec![int("Id"), bool_("Fail")]);
    row.repeating = true;
    value.source = SchemaNode::group("Input", vec![row]);
    value.target = SchemaNode::group("Summary", vec![string("Marker")]);
    value.source_path = None;
    value.target_path = None;
    value.extra_sources.clear();
    value.failure_rules.clear();
    value.user_functions.clear();
    value.source_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    value.target_options = value.source_options.clone();
    value.graph.nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("primary".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("static".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Id".into()],
                frame: Some(vec!["Row".into()]),
            },
        ),
        (
            3,
            Node::SourceField {
                path: vec!["Fail".into()],
                frame: Some(vec!["Row".into()]),
            },
        ),
        (4, Node::Raise { message: Some(5) }),
        (
            5,
            Node::Const {
                value: Value::String("late".into()),
            },
        ),
        (
            6,
            Node::If {
                condition: 3,
                then: 4,
                else_: 7,
            },
        ),
        (
            7,
            Node::Const {
                value: Value::Int(7),
            },
        ),
        (
            8,
            Node::Const {
                value: Value::String(PATH.into()),
            },
        ),
    ]);
    value.root = Scope {
        target_field: "Summary".into(),
        bindings: vec![Binding {
            target_field: "Marker".into(),
            node: 0,
        }],
        ..Default::default()
    };
    value.extra_targets = vec![
        mapping::NamedTarget {
            name: "receipt".into(),
            path: None,
            schema: SchemaNode::group("Receipt", vec![string("Marker")]),
            options: value.target_options.clone(),
            root: Scope {
                target_field: "Receipt".into(),
                bindings: vec![Binding {
                    target_field: "Marker".into(),
                    node: 1,
                }],
                ..Default::default()
            },
        },
        mapping::NamedTarget {
            name: "items".into(),
            path: None,
            schema: SchemaNode::group("Member", vec![int("Id"), int("Value")]),
            options: value.target_options.clone(),
            root: Scope {
                target_field: "Member".into(),
                iteration: mapping::ScopeIteration::DynamicDocuments {
                    source: vec!["Row".into()],
                    output_path: 8,
                },
                bindings: vec![
                    Binding {
                        target_field: "Id".into(),
                        node: 2,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 6,
                    },
                ],
                ..Default::default()
            },
        },
    ];
    value
}
fn source_literal(rows: usize, late: bool) -> String {
    let mut text = String::from("<Input>");
    for id in 1..=rows {
        let selected = late && id == rows;
        text.push_str(&format!("<Row><Id>{id}</Id><Fail>{selected}</Fail></Row>"));
    }
    text.push_str("</Input>");
    text
}
fn primary_literal() -> String {
    String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Summary>\n  <Marker>primary</Marker>\n</Summary>",
    )
}
fn static_literal() -> String {
    String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Receipt>\n  <Marker>static</Marker>\n</Receipt>",
    )
}
fn member_literal(id: usize) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Member>\n  <Id>{id}</Id>\n  <Value>7</Value>\n</Member>"
    )
}
fn member_tree(id: usize) -> Instance {
    Instance::Group(
        vec![
            ("Id".into(), Instance::Scalar(Value::Int(id as i64))),
            ("Value".into(), Instance::Scalar(Value::Int(7))),
        ]
        .into(),
    )
}
struct Case {
    name: &'static str,
    rows: usize,
    late: bool,
    expected: &'static str,
}
fn corpus(directory: &Path, boundary: bool) -> TestResult<(PathBuf, Vec<Case>)> {
    let root = directory.join("inputs");
    std::fs::create_dir(&root)?;
    let cases = if boundary {
        vec![
            Case {
                name: "count-exact",
                rows: 4094,
                late: false,
                expected: "ok",
            },
            Case {
                name: "count-one-over",
                rows: 4095,
                late: false,
                expected: "Output",
            },
            Case {
                name: "late-before-count",
                rows: 4095,
                late: true,
                expected: "Mapping",
            },
        ]
    } else {
        vec![
            Case {
                name: "empty",
                rows: 0,
                late: false,
                expected: "ok",
            },
            Case {
                name: "three-pass",
                rows: 3,
                late: false,
                expected: "ok",
            },
            Case {
                name: "three-late",
                rows: 3,
                late: true,
                expected: "Mapping",
            },
        ]
    };
    for case in &cases {
        exclusive_file(
            &root.join(format!("{}.xml", case.name)),
            source_literal(case.rows, case.late).as_bytes(),
        )?;
    }
    json_file(
        &directory.join("cases.json"),
        &Json::Object(
            cases
                .iter()
                .map(|case| {
                    (
                        case.name.into(),
                        json!({"path":format!("{}.xml",case.name)}),
                    )
                })
                .collect(),
        ),
    )?;
    let identities = cases
        .iter()
        .map(|case| identity(&root.join(format!("{}.xml", case.name))))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(identities.iter().all(|record| {
        record["size"]
            .as_u64()
            .is_some_and(|size| size < 1024 * 1024)
    }));
    json_file(
        &directory.join("complete-input-body-identities-before.json"),
        &json!(identities),
    )?;
    Ok((root, cases))
}
fn prototype(project: &Project, root: &Path, cases: &[Case], directory: &Path) -> TestResult<()> {
    for case in cases {
        let text = std::fs::read_to_string(root.join(format!("{}.xml", case.name)))?;
        let parsed = format_xml::from_str(&text, &project.source);
        std::fs::write(
            directory.join(format!("{}-original-input-parse.txt", case.name)),
            format!("{parsed:#?}\n"),
        )?;
        let expected = Instance::Group(
            vec![(
                "Row".into(),
                Instance::Repeated(
                    (1..=case.rows)
                        .map(|id| {
                            Instance::Group(
                                vec![
                                    ("Id".into(), Instance::Scalar(Value::Int(id as i64))),
                                    (
                                        "Fail".into(),
                                        Instance::Scalar(Value::Bool(case.late && id == case.rows)),
                                    ),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        );
        std::fs::write(
            directory.join(format!("{}-independent-complete-input.txt", case.name)),
            format!("{expected:#?}\n"),
        )?;
        let input = parsed?;
        assert_eq!(input, expected);
        let execution = engine::ExecutionContext::new(Path::new("prototype.json"));
        let mapped =
            engine::run_outputs_with_sources_and_context(project, &input, Vec::new(), &execution);
        std::fs::write(
            directory.join(format!("{}-original-engine-result.txt", case.name)),
            format!("{mapped:#?}\n"),
        )?;
        if case.late {
            assert!(
                matches!(mapped,Err(engine::EngineError::MappingException {node:4,message:Some(message)}) if message=="late")
            );
            continue;
        }
        let expected_primary = Instance::Group(
            vec![(
                "Marker".into(),
                Instance::Scalar(Value::String("primary".into())),
            )]
            .into(),
        );
        let expected_static = Instance::Group(
            vec![(
                "Marker".into(),
                Instance::Scalar(Value::String("static".into())),
            )]
            .into(),
        );
        let expected_list = Instance::DocumentSet(
            (1..=case.rows)
                .map(|id| {
                    ir::DocumentMember::new(PATH, member_tree(id))
                        .ok_or("manual portable member path")
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        std::fs::write(
            directory.join(format!("{}-independent-complete-output.txt", case.name)),
            format!("{expected_primary:#?}\n{expected_static:#?}\n{expected_list:#?}\n"),
        )?;
        let mapped = mapped?;
        assert_eq!(mapped.primary, expected_primary);
        assert_eq!(mapped.extras.len(), 2);
        assert_eq!(mapped.extras[0].name, "receipt");
        assert_eq!(mapped.extras[0].instance, expected_static);
        assert_eq!(mapped.extras[1].name, "items");
        assert_eq!(mapped.extras[1].instance, expected_list);
        for (label, schema, value, expected) in [
            (
                "primary",
                &project.target,
                &mapped.primary,
                primary_literal(),
            ),
            (
                "receipt",
                &project.extra_targets[0].schema,
                &mapped.extras[0].instance,
                static_literal(),
            ),
        ] {
            let written = format_xml::to_string(schema, value);
            std::fs::write(
                directory.join(format!("{}-{label}-original-writer.txt", case.name)),
                format!("{written:#?}\n"),
            )?;
            assert_eq!(written?, expected);
        }
        let Instance::DocumentSet(members) = &mapped.extras[1].instance else {
            return Err("manual DocumentSet".into());
        };
        for (index, member) in members.iter().enumerate() {
            let written = format_xml::to_string(&project.extra_targets[1].schema, member.value());
            std::fs::write(
                directory.join(format!("{}-member{index}-original-writer.txt", case.name)),
                format!("{written:#?}\n"),
            )?;
            assert_eq!(written?, member_literal(index + 1));
        }
    }
    Ok(())
}
fn document(actual: &Json, literal: &str, bytes: bool) {
    assert_eq!(actual["document"], literal);
    if bytes {
        assert_eq!(actual["original_return_bytes"], json!(literal.as_bytes()));
    } else {
        assert_eq!(actual["original_return_bytes"], Json::Null);
    }
}
fn assert_row(row: &Json, case: &Case, api: &str, language: &str) {
    assert_eq!(row["case"], case.name);
    assert_eq!(row["api"], api);
    let result = &row["result"];
    assert_eq!(result["kind"], case.expected);
    if case.expected == "ok" {
        assert_eq!(result["returned"], true);
        let outputs = &result["outputs"];
        let bytes = api.starts_with("bytes");
        document(&outputs["primary"], &primary_literal(), bytes);
        let extras = outputs["extras"].as_array().expect("complete mixed extras");
        assert_eq!(extras.len(), 2);
        assert_eq!(extras[0]["kind"], "SingleDocument");
        assert_eq!(extras[0]["declaration_index"], 0);
        assert_eq!(extras[0]["name"], "receipt");
        document(&extras[0], &static_literal(), bytes);
        assert_eq!(extras[1]["kind"], "DocumentList");
        assert_eq!(extras[1]["declaration_index"], 1);
        assert_eq!(extras[1]["name"], "items");
        let members = extras[1]["documents"]
            .as_array()
            .expect("complete ordered document list");
        assert_eq!(members.len(), case.rows);
        for (index, member) in members.iter().enumerate() {
            assert_eq!(member["path"], PATH);
            document(member, &member_literal(index + 1), bytes);
        }
        return;
    }
    assert_eq!(result["returned"], false);
    assert_eq!(result["outputs"], Json::Null);
    assert_eq!(result["owner"], Json::Null);
    assert_eq!(result["boundary_bytes"], Json::Null);
    assert_eq!(result["boundary_limit"], Json::Null);
    assert_eq!(result["boundary_preserved"], true);
    assert!(
        result["chain"]
            .as_array()
            .is_some_and(|chain| chain.len() >= 3)
    );
    if case.expected == "Mapping" {
        assert_eq!(
            result["mapping_exception"],
            json!({"node":4,"message":"late"})
        );
        assert_eq!(result["runtime_cause_present"], true);
        assert_eq!(result["resource"], Json::Null);
    } else {
        assert_eq!(result["mapping_exception"], Json::Null);
        assert_eq!(result["runtime_cause_present"], false);
        assert_eq!(
            result["resource"],
            json!({"resource":"xml_output_artifact_count","observed_count":4097,"limit":4096})
        );
    }
    if language == "csharp" {
        assert_eq!(
            result["cause_type"],
            if case.expected == "Mapping" {
                "Ferrule.Runtime.FerruleRuntimeException"
            } else {
                "Ferrule.Runtime.FerruleXmlOutputSetResourceException"
            }
        );
    }
}
fn exercise(boundary: bool) -> TestResult<()> {
    let _serial = SERIAL
        .lock()
        .map_err(|_| io::Error::other("XML mixed-count lock poisoned"))?;
    let mut directory = Originals::new()?;
    let deadline = Instant::now() + Duration::from_secs(if boundary { 900 } else { 600 });
    let before = source_identities()?;
    json_file(
        &directory.path.join("source-and-selected-ELFs-before.json"),
        &before,
    )?;
    let result = caught(&directory.path, "test", || -> TestResult<()> {
        if boundary {
            assert_eq!(
                std::env::var_os("FERRULE_XML_MIXED_COUNT_BOUNDARY").as_deref(),
                Some(OsStr::new("1")),
                "explicit ignored count opt-in required"
            );
            assert_eq!(
                std::env::var_os("FERRULE_XML_MIXED_COUNT_HEADROOM_REVIEWED").as_deref(),
                Some(OsStr::new("1")),
                "caller must coordinate shared build/resource work"
            );
            headroom(&directory.path, deadline, "before-boundary")?;
        }
        for (tool, args) in [
            ("rustc", vec!["-vV"]),
            ("cargo", vec!["--version"]),
            ("dotnet", vec!["--info"]),
            ("timeout", vec!["--version"]),
        ] {
            let mut command = Command::new(tool);
            command.args(args);
            let output = recorded(
                &mut command,
                &directory.path,
                &format!("{tool}-identity"),
                deadline,
                30,
                false,
            )?;
            require_success(&output);
        }
        if boundary {
            let mut command = Command::new(gnu_time()?);
            command.arg("--version");
            let output = recorded(
                &mut command,
                &directory.path,
                "time-identity",
                deadline,
                30,
                false,
            )?;
            require_success(&output);
        }
        let project = mapping();
        let (inputs, cases) = corpus(&directory.path, boundary)?;
        let proto = directory.path.join("prototype");
        std::fs::create_dir(&proto)?;
        let (proto_inputs, proto_cases) = corpus(&proto, false)?;
        prototype(&project, &proto_inputs, &proto_cases, &proto)?;
        struct Library {
            language: &'static str,
            phase: PathBuf,
            project: PathBuf,
            generated: PathBuf,
            before: ArtifactFiles,
            command: Command,
            binary: PathBuf,
            binary_before: Json,
        }
        let mut libraries = Vec::new();
        let mut seen = BTreeSet::new();
        let host_result = caught(&directory.path, "all-host-phases", || -> TestResult<()> {
            for language in ["rust", "csharp"] {
                let phase = directory.path.join(language);
                std::fs::create_dir(&phase)?;
                let (path, generated) = prepare(&phase, language, &project, deadline)?;
                let library_before = artifact_files(&generated)?;
                let executable = build(&phase, language, &generated, deadline)?;
                let selected_binary = if language == "rust" {
                    PathBuf::from(executable.get_program())
                } else {
                    PathBuf::from(executable.get_args().next().ok_or("host DLL argument")?)
                };
                let binary_before = identity(&selected_binary)?;
                json_file(
                    &phase.join("selected-host-identity-before.json"),
                    &binary_before,
                )?;
                libraries.push(Library {
                    language,
                    phase,
                    project: path,
                    generated,
                    before: library_before,
                    command: executable,
                    binary: selected_binary,
                    binary_before,
                });
            }
            let cohorts = if boundary {
                vec![
                    ("prototype", &proto, &proto_inputs, &proto_cases, false),
                    ("boundary", &directory.path, &inputs, &cases, true),
                ]
            } else {
                vec![("prototype", &directory.path, &inputs, &cases, false)]
            };
            for (cohort, manifest_dir, root, controls, measure) in cohorts {
                if measure {
                    let mut prerequisites = BTreeSet::new();
                    for language in ["rust", "csharp"] {
                        for case in &proto_cases {
                            for api in APIS {
                                prerequisites.insert((
                                    language.to_owned(),
                                    "prototype".to_owned(),
                                    case.name.to_owned(),
                                    api.to_owned(),
                                ));
                            }
                        }
                    }
                    json_file(
                        &directory
                            .path
                            .join("all-small-public-prerequisites-before-first-boundary.json"),
                        &json!({"actual":&seen,"expected":&prerequisites}),
                    )?;
                    assert_eq!(seen, prerequisites);
                    assert_eq!(seen.len(), 24);
                }
                for library in &libraries {
                    for case in controls {
                        for api in APIS {
                            let original =
                                library.phase.join(format!("{cohort}-{}-{api}", case.name));
                            std::fs::create_dir(&original)?;
                            let mut command = if library.language == "rust" {
                                Command::new(library.command.get_program())
                            } else {
                                let mut command = dotnet_command(&library.phase);
                                command.args(library.command.get_args());
                                command
                            };
                            for (key, value) in library.command.get_envs() {
                                if let Some(value) = value {
                                    command.env(key, value);
                                } else {
                                    command.env_remove(key);
                                }
                            }
                            command
                                .arg(manifest_dir.join("cases.json"))
                                .arg(root)
                                .arg(case.name)
                                .arg(api)
                                .arg(&library.project)
                                .arg(&original)
                                .current_dir(&library.phase);
                            let output =
                                recorded(&mut command, &original, "host", deadline, 60, measure)?;
                            let memory = if measure {
                                Some(std::fs::read(original.join("host-memory.txt"))?)
                            } else {
                                None
                            };
                            if let Some(memory) = &memory {
                                std::fs::write(
                                    original.join("original-memory-observation.bin"),
                                    memory,
                                )?;
                            }
                            require_success(&output);
                            assert!(output.stderr.is_empty());
                            assert!(
                                output.stdout.len() < 4 * 1024 * 1024,
                                "bounded complete tiny mixed output envelope"
                            );
                            let row: Json = serde_json::from_slice(&output.stdout)?;
                            assert_eq!(
                                row,
                                serde_json::from_slice::<Json>(&std::fs::read(
                                    original.join("actual-public-result.json")
                                )?)?
                            );
                            assert!(original.join("original-typed-result.txt").is_file());
                            if let Some(memory) = memory {
                                let text = std::str::from_utf8(&memory)?;
                                let peak = text
                                    .lines()
                                    .find_map(|line| {
                                        line.trim()
                                            .strip_prefix("Maximum resident set size (kbytes):")
                                    })
                                    .ok_or("GNU time peak RSS missing")?
                                    .trim()
                                    .parse::<u64>()?;
                                json_file(
                                    &original.join("actual-memory-observation.json"),
                                    &json!({"peak_RSS_kib":peak,"scope":"one host process; not a memory guarantee"}),
                                )?;
                            }
                            assert!(seen.insert((
                                library.language.to_owned(),
                                cohort.to_owned(),
                                case.name.to_owned(),
                                api.to_owned()
                            )));
                            assert_row(&row, case, api, library.language);
                        }
                    }
                }
            }
            Ok(())
        });
        std::fs::write(
            directory.path.join("original-all-host-phases-result.txt"),
            format!("{host_result:#?}\n"),
        )?;
        let library_after_result = caught(
            &directory.path,
            "all-library-after",
            || -> TestResult<()> {
                let mut failures = Vec::new();
                for library in &libraries {
                    let observed = caught(&library.phase, "library-after", || -> TestResult<()> {
                        let binary_after = identity(&library.binary);
                        std::fs::write(
                            library
                                .phase
                                .join("original-host-identity-after-result.txt"),
                            format!("{binary_after:#?}\n"),
                        )?;
                        let binary_after = binary_after?;
                        json_file(
                            &library.phase.join("selected-host-identity-after.json"),
                            &binary_after,
                        )?;
                        assert_eq!(binary_after, library.binary_before);
                        let library_after = artifact_files(&library.generated)?;
                        json_file(
                            &library.phase.join("library-after-whole-body-equality.json"),
                            &json!({"exact":library_after==library.before,"file_count":library_after.len()}),
                        )?;
                        assert_eq!(library_after, library.before);
                        Ok(())
                    });
                    std::fs::write(
                        library.phase.join("original-library-after-result.txt"),
                        format!("{observed:#?}\n"),
                    )?;
                    if let Err(error) = observed {
                        failures.push(format!("{}: {error}", library.language));
                    }
                }
                if !failures.is_empty() {
                    return Err(io::Error::other(failures.join("; ")).into());
                }
                Ok(())
            },
        );
        std::fs::write(
            directory.path.join("original-all-library-after-result.txt"),
            format!("{library_after_result:#?}\n"),
        )?;
        library_after_result?;
        host_result?;
        json_file(
            &directory.path.join("complete-unique-route-inventory.json"),
            &json!(&seen),
        )?;
        let cohorts = if boundary {
            vec![("prototype", &proto_cases), ("boundary", &cases)]
        } else {
            vec![("prototype", &cases)]
        };
        let mut expected = BTreeSet::new();
        for language in ["rust", "csharp"] {
            for (cohort, controls) in &cohorts {
                for case in *controls {
                    for api in APIS {
                        expected.insert((
                            language.to_owned(),
                            (*cohort).to_owned(),
                            case.name.to_owned(),
                            api.to_owned(),
                        ));
                    }
                }
            }
        }
        assert_eq!(seen, expected);
        assert_eq!(seen.len(), if boundary { 48 } else { 24 });
        Ok(())
    });
    std::fs::write(
        directory.path.join("original-complete-test-result.txt"),
        format!("{result:#?}\n"),
    )?;
    let corpus_after = caught(&directory.path, "corpus-after", || -> TestResult<()> {
        let proto = directory.path.join("prototype");
        for path in [directory.path.as_path(), proto.as_path()] {
            let before_path = path.join("complete-input-body-identities-before.json");
            if before_path.is_file() {
                let original: Json = serde_json::from_slice(&std::fs::read(before_path)?)?;
                let after = original
                    .as_array()
                    .ok_or("input identity array")?
                    .iter()
                    .map(|record| {
                        identity(Path::new(
                            record["path"]
                                .as_str()
                                .ok_or_else(|| io::Error::other("input identity path"))?,
                        ))
                    })
                    .collect::<Result<Vec<_>, io::Error>>()?;
                json_file(
                    &path.join("complete-input-body-identities-after.json"),
                    &json!(&after),
                )?;
                assert_eq!(json!(after), original);
            }
        }
        Ok(())
    });
    std::fs::write(
        directory.path.join("original-corpus-after-result.txt"),
        format!("{corpus_after:#?}\n"),
    )?;
    let after = source_identities();
    std::fs::write(
        directory.path.join("original-source-after-result.txt"),
        format!("{after:#?}\n"),
    )?;
    let after = after?;
    json_file(
        &directory.path.join("source-and-selected-ELFs-after.json"),
        &after,
    )?;
    assert_eq!(after, before);
    if boundary {
        let observed = caught(&directory.path, "headroom-after", || {
            headroom(&directory.path, deadline, "after-boundary")
        });
        std::fs::write(
            directory.path.join("original-headroom-after-result.txt"),
            format!("{observed:#?}\n"),
        )?;
        observed?;
    }
    corpus_after?;
    result?;
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_xml_mixed_outputs_small_late_mapping_prototype() -> TestResult<()> {
    exercise(false)
}
#[test]
#[ignore = "serial public mixed 4096/4097 output count versus last-row Raise; explicit opt-in/headroom"]
fn generated_xml_mixed_outputs_late_raise_precedes_count_opt_in() -> TestResult<()> {
    exercise(true)
}
