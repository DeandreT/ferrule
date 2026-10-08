//! Compiled singular XML loaders: unprimed request counts and original typed refusals.
use super::super::*;
use serde_json::{Value as Json, json};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[path = "request_count/support.rs"]
mod support;
use support::*;
const APIS: [&str; 4] = ["text", "text-context", "bytes", "bytes-context"];
const STATIC: &str = "<Rates><Marker>fixed</Marker></Rates>";
const CATALOG: &str = "<Catalog><Row><Value>7</Value></Row></Catalog>";
const LOGICAL_PATH: &str = "same.xml";
static SERIAL: Mutex<()> = Mutex::new(());

fn mapping() -> Project {
    let mut value = project();
    let mut driver = SchemaNode::group("Driver", vec![int("Id"), string("Path")]);
    driver.repeating = true;
    let mut row = SchemaNode::group("Row", vec![int("Value")]);
    row.repeating = true;
    let mut line = SchemaNode::group("Line", vec![int("Id"), int("Value")]);
    line.repeating = true;
    value.source = SchemaNode::group("Manifest", vec![driver]);
    value.target = SchemaNode::group("Output", vec![line]);
    value.source_path = None;
    value.target_path = None;
    value.source_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    value.target_options = value.source_options.clone();
    value.extra_sources = vec![
        mapping::NamedSource {
            name: "rates".into(),
            path: "rates.xml".into(),
            schema: SchemaNode::group("Rates", vec![string("Marker")]),
            options: value.source_options.clone(),
            dynamic_path: None,
        },
        mapping::NamedSource {
            name: "catalog".into(),
            path: String::new(),
            schema: SchemaNode::group("Catalog", vec![row]),
            options: value.source_options.clone(),
            dynamic_path: Some(mapping::DynamicSourcePath {
                node: 0,
                iteration: vec!["Driver".into()],
            }),
        },
    ];
    value.extra_targets.clear();
    value.failure_rules.clear();
    value.user_functions.clear();
    value.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Path".into()],
                frame: Some(vec!["Driver".into()]),
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Id".into()],
                frame: Some(vec!["Driver".into()]),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: Some(vec!["catalog".into(), "Row".into()]),
            },
        ),
    ]);
    value.root = Scope {
        target_field: "Output".into(),
        children: vec![Scope {
            target_field: "Line".into(),
            iteration: mapping::ScopeIteration::Source(vec!["catalog".into(), "Row".into()]),
            bindings: vec![
                Binding {
                    target_field: "Id".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Value".into(),
                    node: 2,
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    value
}
fn source_literal(rows: usize) -> String {
    let mut text = String::from("<Manifest>");
    for id in 1..=rows {
        text.push_str(&format!(
            "<Driver><Id>{id}</Id><Path>same.xml</Path></Driver>"
        ));
    }
    text.push_str("</Manifest>");
    text
}
fn output_literal(rows: usize) -> String {
    let mut text = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n");
    for id in 1..=rows {
        text.push_str(&format!(
            "  <Line>\n    <Id>{id}</Id>\n    <Value>7</Value>\n  </Line>\n"
        ));
    }
    text.push_str("</Output>");
    text
}
fn input_tree(rows: usize) -> Instance {
    Instance::Group(
        vec![(
            "Driver".into(),
            Instance::Repeated(
                (1..=rows)
                    .map(|id| {
                        Instance::Group(
                            vec![
                                ("Id".into(), Instance::Scalar(Value::Int(id as i64))),
                                (
                                    "Path".into(),
                                    Instance::Scalar(Value::String(LOGICAL_PATH.into())),
                                ),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}
fn output_tree(rows: usize) -> Instance {
    Instance::Group(
        vec![(
            "Line".into(),
            Instance::Repeated(
                (1..=rows)
                    .map(|id| {
                        Instance::Group(
                            vec![
                                ("Id".into(), Instance::Scalar(Value::Int(id as i64))),
                                ("Value".into(), Instance::Scalar(Value::Int(7))),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}
fn calls(count: usize) -> Vec<Json> {
    (1..=count)
        .map(|ordinal| json!({"ordinal":ordinal,"source":"catalog","path":LOGICAL_PATH}))
        .collect()
}
struct Case {
    name: &'static str,
    rows: usize,
    refused: bool,
}
fn corpus(directory: &Path, boundary: bool) -> TestResult<(PathBuf, Vec<Case>)> {
    let root = directory.join("inputs");
    std::fs::create_dir(&root)?;
    let cases = if boundary {
        vec![
            Case {
                name: "count-exact",
                rows: 4094,
                refused: false,
            },
            Case {
                name: "count-one-over",
                rows: 4095,
                refused: true,
            },
        ]
    } else {
        vec![
            Case {
                name: "empty-prototype",
                rows: 0,
                refused: false,
            },
            Case {
                name: "three-real-loads",
                rows: 3,
                refused: false,
            },
        ]
    };
    for case in &cases {
        exclusive_file(
            &root.join(format!("{}.xml", case.name)),
            source_literal(case.rows).as_bytes(),
        )?;
    }
    exclusive_file(&root.join("rates.xml"), STATIC.as_bytes())?;
    exclusive_file(&root.join(LOGICAL_PATH), CATALOG.as_bytes())?;
    let manifest = Json::Object(
        cases
            .iter()
            .map(|case| {
                (
                    case.name.into(),
                    json!({"path":format!("{}.xml",case.name),"rows":case.rows}),
                )
            })
            .collect(),
    );
    json_file(&directory.join("cases.json"), &manifest)?;
    let identities = cases
        .iter()
        .map(|case| identity(&root.join(format!("{}.xml", case.name))))
        .chain([
            identity(&root.join("rates.xml")),
            identity(&root.join(LOGICAL_PATH)),
        ])
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
struct PrototypeLoader {
    document: Arc<Instance>,
    calls: RefCell<Vec<Json>>,
}
impl engine::DynamicSourceLoader for PrototypeLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        let mut records = self.calls.borrow_mut();
        let ordinal = records.len() + 1;
        records.push(json!({"ordinal":ordinal,"source":source,"path":path}));
        if source != "catalog" || path != LOGICAL_PATH {
            return Err("unexpected prototype request".into());
        }
        Ok(self.document.clone())
    }
}
fn prototype(
    project: &Project,
    root: &Path,
    controls: &[Case],
    directory: &Path,
) -> TestResult<()> {
    let static_result = format_xml::from_str(STATIC, &project.extra_sources[0].schema);
    let catalog_result = format_xml::from_str(CATALOG, &project.extra_sources[1].schema);
    std::fs::write(
        directory.join("complete-original-static-catalog-parses.txt"),
        format!("{static_result:#?}\n{catalog_result:#?}\n"),
    )?;
    let rates = static_result?;
    let catalog = catalog_result?;
    assert_eq!(
        rates,
        Instance::Group(
            vec![(
                "Marker".into(),
                Instance::Scalar(Value::String("fixed".into()))
            )]
            .into()
        )
    );
    assert_eq!(
        catalog,
        Instance::Group(
            vec![(
                "Row".into(),
                Instance::Repeated(vec![Instance::Group(
                    vec![("Value".into(), Instance::Scalar(Value::Int(7)))].into()
                )])
            )]
            .into()
        )
    );
    for case in controls {
        let raw = std::fs::read_to_string(root.join(format!("{}.xml", case.name)))?;
        let parsed = format_xml::from_str(&raw, &project.source);
        std::fs::write(
            directory.join(format!("{}-original-primary-parse.txt", case.name)),
            format!("{parsed:#?}\n"),
        )?;
        let input = parsed?;
        let expected = input_tree(case.rows);
        std::fs::write(
            directory.join(format!("{}-independent-complete-input.txt", case.name)),
            format!("{expected:#?}\n"),
        )?;
        assert_eq!(input, expected);
        let host = PrototypeLoader {
            document: Arc::new(catalog.clone()),
            calls: RefCell::default(),
        };
        let execution = engine::ExecutionContext::new(Path::new("prototype.json"))
            .with_dynamic_source_loader(&host);
        let result = engine::run_outputs_with_sources_and_context(
            project,
            &input,
            vec![("rates".into(), rates.clone())],
            &execution,
        );
        std::fs::write(
            directory.join(format!("{}-original-engine-result.txt", case.name)),
            format!("{result:#?}\n"),
        )?;
        json_file(
            &directory.join(format!("{}-original-engine-calls.json", case.name)),
            &json!(&*host.calls.borrow()),
        )?;
        let expected = output_tree(case.rows);
        std::fs::write(
            directory.join(format!("{}-independent-complete-output.txt", case.name)),
            format!("{expected:#?}\n"),
        )?;
        let result = result?;
        assert_eq!(result.primary, expected);
        assert!(result.extras.is_empty());
        assert_eq!(*host.calls.borrow(), calls(case.rows));
        let written = format_xml::to_string(&project.target, &result.primary);
        std::fs::write(
            directory.join(format!("{}-original-writer-result.txt", case.name)),
            format!("{written:#?}\n"),
        )?;
        assert_eq!(written?, output_literal(case.rows));
    }
    Ok(())
}
fn assert_row(row: &Json, case: &Case, api: &str, language: &str) {
    assert_eq!(row["case"], case.name);
    assert_eq!(row["api"], api);
    let admitted = if case.refused { 4094 } else { case.rows };
    assert_eq!(row["calls"], json!(calls(admitted)));
    let result = &row["result"];
    if !case.refused {
        assert_eq!(result["kind"], "ok");
        assert_eq!(result["returned"], true);
        assert_eq!(result["primary"], output_literal(case.rows));
        if api.starts_with("bytes") {
            assert_eq!(
                result["original_return_bytes"],
                json!(output_literal(case.rows).as_bytes())
            );
        }
        return;
    }
    assert_eq!(result["kind"], "Input");
    assert_eq!(result["returned"], false);
    assert!(result.get("primary").is_none());
    assert_eq!(
        result["input"],
        json!({"kind":"Named","index":1,"name":"catalog"})
    );
    assert_eq!(result["output"], Json::Null);
    assert_eq!(
        result["request"],
        json!({"declaration_index":1,"source":"catalog","path":LOGICAL_PATH,"ordinal":4095,"callback_invoked":false})
    );
    assert_eq!(
        result["resource"],
        json!({"resource":"xml_input_artifact_count","observed_count":4097,"limit":4096})
    );
    assert_eq!(result["boundary_bytes"], Json::Null);
    assert_eq!(result["boundary_limit"], Json::Null);
    assert_eq!(result["boundary_preserved"], true);
    assert_eq!(result["resource_cause_present"], true);
    assert_eq!(result["runtime_cause_present"], false);
    assert!(
        result["chain"]
            .as_array()
            .is_some_and(|chain| chain.len() >= 3)
    );
    if language == "csharp" {
        assert_eq!(
            result["resource_type"],
            "Ferrule.Runtime.FerruleXmlInputSetResourceException"
        );
    }
}
fn exercise(boundary: bool) -> TestResult<()> {
    let _serial = SERIAL
        .lock()
        .map_err(|_| io::Error::other("XML request-count lock poisoned"))?;
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
                std::env::var_os("FERRULE_XML_REQUEST_COUNT_BOUNDARY").as_deref(),
                Some(OsStr::new("1")),
                "explicit ignored count opt-in required"
            );
            assert_eq!(
                std::env::var_os("FERRULE_XML_REQUEST_COUNT_HEADROOM_REVIEWED").as_deref(),
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
                    assert_eq!(seen.len(), 16);
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
                                output.stdout.len() < 2 * 1024 * 1024,
                                "bounded complete small output and full callback log"
                            );
                            let row: Json = serde_json::from_slice(&output.stdout)?;
                            assert_eq!(
                                row,
                                serde_json::from_slice::<Json>(&std::fs::read(
                                    original.join("actual-public-result.json")
                                )?)?
                            );
                            assert!(original.join("original-typed-result.txt").is_file());
                            assert_eq!(
                                row["calls"],
                                serde_json::from_slice::<Json>(&std::fs::read(
                                    original.join("original-complete-callback-log.json")
                                )?)?
                            );
                            let raw = std::fs::read_to_string(
                                original.join("original-callback-events.jsonl"),
                            )?;
                            let events = raw
                                .lines()
                                .map(serde_json::from_str::<Json>)
                                .collect::<Result<Vec<_>, _>>()?;
                            assert_eq!(json!(events), row["calls"]);
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
        assert_eq!(seen.len(), if boundary { 32 } else { 16 });
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
fn generated_xml_dynamic_loader_small_actual_callback_prototype() -> TestResult<()> {
    exercise(false)
}
#[test]
#[ignore = "serial public 4096/4097 XML request boundary; explicit opt-in/headroom and GNU timeout/time required"]
fn generated_xml_dynamic_loader_exact_and_one_over_request_count_opt_in() -> TestResult<()> {
    exercise(true)
}
