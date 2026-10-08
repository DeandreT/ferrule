//! Public static-input boundaries. Large controls are explicitly opt-in and serial.
use super::super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::time::{Duration, Instant};

const M: usize = 64 * 1024 * 1024;
const PROTOTYPE: usize = 256;
const XML: &str =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Result>ok</Result>\n</Output>";
const APIS: [&str; 8] = [
    "text-set",
    "text-set-context",
    "text-singular",
    "text-singular-context",
    "bytes-set",
    "bytes-set-context",
    "bytes-singular",
    "bytes-singular-context",
];
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "combined_bytes/support.rs"]
mod support;
use support::*;

fn mapping() -> Project {
    let mut value = project();
    value.source = SchemaNode::group("Input", vec![bool_("Fail"), string("Pad")]);
    value.target = SchemaNode::group("Output", vec![string("Result")]);
    value.source_path = None;
    value.target_path = None;
    value.source_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    value.target_options = value.source_options.clone();
    value.extra_sources = ["a", "b", "c", "d"]
        .into_iter()
        .map(|name| mapping::NamedSource {
            name: name.into(),
            path: format!("{name}.xml"),
            schema: SchemaNode::group("Data", vec![string("Pad")]),
            options: value.source_options.clone(),
            dynamic_path: None,
        })
        .collect();
    value.extra_targets.clear();
    value.failure_rules.clear();
    value.user_functions.clear();
    value.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Fail".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("ok".into()),
            },
        ),
        (2, Node::Raise { message: Some(3) }),
        (
            3,
            Node::Const {
                value: Value::String("must-not-map".into()),
            },
        ),
        (
            4,
            Node::If {
                condition: 0,
                then: 2,
                else_: 1,
            },
        ),
    ]);
    value.root = Scope {
        target_field: "Output".into(),
        bindings: vec![Binding {
            target_field: "Result".into(),
            node: 4,
        }],
        ..Default::default()
    };
    value
}
fn document(path: &Path, bytes: usize, primary: Option<bool>, malformed: bool) -> TestResult<()> {
    let prefix = primary.map_or_else(
        || "<Data><Pad>".to_owned(),
        |fail| format!("<Input><Fail>{fail}</Fail><Pad>"),
    );
    let suffix = if primary.is_some() {
        if malformed {
            "</Pad></Input!"
        } else {
            "</Pad></Input>"
        }
    } else {
        "</Pad></Data>"
    };
    let padding = bytes
        .checked_sub(prefix.len() + suffix.len())
        .ok_or("prototype too small")?;
    let mut file = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    );
    file.write_all(prefix.as_bytes())?;
    let block = [b'x'; 65536];
    let mut remaining = padding;
    while remaining != 0 {
        let count = remaining.min(block.len());
        file.write_all(&block[..count])?;
        remaining -= count;
    }
    file.write_all(suffix.as_bytes())?;
    file.flush()?;
    file.get_ref().sync_all()?;
    assert_eq!(std::fs::metadata(path)?.len(), bytes as u64);
    Ok(())
}
struct Case {
    name: &'static str,
    primary: &'static str,
    d: &'static str,
    reverse: bool,
    expected: &'static str,
}
impl Case {
    fn json(&self) -> Json {
        let mut named = vec![
            json!({"name":"a","path":"a.xml"}),
            json!({"name":"b","path":"b.xml"}),
            json!({"name":"c","path":"c.xml"}),
            json!({"name":"d","path":self.d}),
        ];
        if self.reverse {
            named.reverse();
        }
        json!({"primary":self.primary,"named":named})
    }
}
fn corpus(directory: &Path, large: bool) -> TestResult<(PathBuf, Vec<Case>, Json)> {
    let root = directory.join("inputs");
    std::fs::create_dir(&root)?;
    let size = if large { M } else { PROTOTYPE };
    let mut specs = vec![
        ("primary-false.xml", size, Some(false), false),
        ("primary-true.xml", size, Some(true), false),
        ("a.xml", size, None, false),
        ("b.xml", size, None, false),
        ("c.xml", size - 64, None, false),
        ("d64.xml", 64, None, false),
        ("d65.xml", 65, None, false),
    ];
    if large {
        specs.extend([
            ("primary-malformed.xml", size, Some(true), true),
            ("d-oversized.xml", M + 1, None, false),
        ]);
    }
    for (name, bytes, primary, malformed) in &specs {
        document(&root.join(name), *bytes, *primary, *malformed)?;
    }
    let cases = if large {
        vec![
            Case {
                name: "exact-forward",
                primary: "primary-false.xml",
                d: "d64.xml",
                reverse: false,
                expected: "ok",
            },
            Case {
                name: "exact-reverse",
                primary: "primary-false.xml",
                d: "d64.xml",
                reverse: true,
                expected: "ok",
            },
            Case {
                name: "over-forward",
                primary: "primary-true.xml",
                d: "d65.xml",
                reverse: false,
                expected: "Input",
            },
            Case {
                name: "over-reverse",
                primary: "primary-true.xml",
                d: "d65.xml",
                reverse: true,
                expected: "Input",
            },
            Case {
                name: "malformed-primary-later-oversized",
                primary: "primary-malformed.xml",
                d: "d-oversized.xml",
                reverse: true,
                expected: "DocumentLimit",
            },
        ]
    } else {
        vec![
            Case {
                name: "prototype-forward",
                primary: "primary-false.xml",
                d: "d64.xml",
                reverse: false,
                expected: "ok",
            },
            Case {
                name: "prototype-reverse",
                primary: "primary-false.xml",
                d: "d64.xml",
                reverse: true,
                expected: "ok",
            },
            Case {
                name: "prototype-plus-one",
                primary: "primary-false.xml",
                d: "d65.xml",
                reverse: false,
                expected: "ok",
            },
            Case {
                name: "reachable-mapping-sentinel",
                primary: "primary-true.xml",
                d: "d64.xml",
                reverse: true,
                expected: "Mapping",
            },
        ]
    };
    let manifest = Json::Object(
        cases
            .iter()
            .map(|case| (case.name.into(), case.json()))
            .collect(),
    );
    json_file(&directory.join("cases.json"), &manifest)?;
    let identities = Json::Array(
        specs
            .iter()
            .map(|(name, _, _, _)| identity(&root.join(name)))
            .collect::<Result<_, _>>()?,
    );
    json_file(
        &directory.join("complete-input-body-identities-before.json"),
        &identities,
    )?;
    if large {
        for case in &cases {
            let total = std::fs::metadata(root.join(case.primary))?.len() + 3 * M as u64 - 64
                + std::fs::metadata(root.join(case.d))?.len();
            if case.expected == "ok" {
                assert_eq!(total, 268_435_456);
            }
            if case.expected == "Input" {
                assert_eq!(total, 268_435_457);
            }
        }
    }
    Ok((root, cases, identities))
}
fn assert_prototype(
    project: &Project,
    root: &Path,
    cases: &[Case],
    directory: &Path,
) -> TestResult<()> {
    for case in cases {
        let primary_text = std::fs::read_to_string(root.join(case.primary))?;
        let parsed = format_xml::from_str(&primary_text, &project.source);
        std::fs::write(
            directory.join(format!("{}-primary-original-parse.txt", case.name)),
            format!("{parsed:#?}\n"),
        )?;
        let mut inputs = Vec::new();
        for input in case.json()["named"].as_array().ok_or("prototype names")? {
            let schema = &project
                .extra_sources
                .iter()
                .find(|source| source.name == input["name"].as_str().unwrap())
                .ok_or("prototype owner")?
                .schema;
            let text = std::fs::read_to_string(root.join(input["path"].as_str().unwrap()))?;
            let parsed = format_xml::from_str(&text, schema);
            std::fs::write(
                directory.join(format!(
                    "{}-{}-original-parse.txt",
                    case.name,
                    input["name"].as_str().unwrap()
                )),
                format!("{parsed:#?}\n"),
            )?;
            inputs.push((input["name"].as_str().unwrap().to_owned(), parsed?));
        }
        let input = parsed?;
        let primary_fail = case.expected == "Mapping";
        let primary_prefix = format!("<Input><Fail>{primary_fail}</Fail><Pad>");
        let expected_input = Instance::Group(
            vec![
                ("Fail".into(), Instance::Scalar(Value::Bool(primary_fail))),
                (
                    "Pad".into(),
                    Instance::Scalar(Value::String(
                        "x".repeat(PROTOTYPE - primary_prefix.len() - "</Pad></Input>".len()),
                    )),
                ),
            ]
            .into(),
        );
        let expected_named = inputs
            .iter()
            .map(|(name, _)| {
                let bytes = if name == "c" {
                    PROTOTYPE - 64
                } else if name == "d" {
                    if case.name == "prototype-plus-one" {
                        65
                    } else {
                        64
                    }
                } else {
                    PROTOTYPE
                };
                (
                    name.clone(),
                    Instance::Group(
                        vec![(
                            "Pad".into(),
                            Instance::Scalar(Value::String(
                                "x".repeat(bytes - "<Data><Pad>".len() - "</Pad></Data>".len()),
                            )),
                        )]
                        .into(),
                    ),
                )
            })
            .collect::<Vec<_>>();
        std::fs::write(
            directory.join(format!(
                "{}-independent-complete-input-oracle.txt",
                case.name
            )),
            format!("{expected_input:#?}\n{expected_named:#?}\n"),
        )?;
        assert_eq!(input, expected_input);
        assert_eq!(inputs, expected_named);
        let execution = engine::ExecutionContext::new(Path::new("prototype.json"));
        let result =
            engine::run_outputs_with_sources_and_context(project, &input, inputs, &execution);
        std::fs::write(
            directory.join(format!("{}-original-engine-result.txt", case.name)),
            format!("{result:#?}\n"),
        )?;
        if case.expected == "Mapping" {
            assert!(
                matches!(result,Err(engine::EngineError::MappingException { node:2,message:Some(message) }) if message=="must-not-map")
            );
        } else {
            let result = result?;
            assert_eq!(
                result.primary,
                Instance::Group(
                    vec![(
                        "Result".into(),
                        Instance::Scalar(Value::String("ok".into()))
                    )]
                    .into()
                )
            );
            assert!(result.extras.is_empty());
            let written = format_xml::to_string(&project.target, &result.primary);
            std::fs::write(
                directory.join(format!("{}-original-writer-result.txt", case.name)),
                format!("{written:#?}\n"),
            )?;
            assert_eq!(written?, XML);
        }
    }
    Ok(())
}
fn assert_row(row: &Json, case: &Case, api: &str) {
    assert_eq!(row["case"], case.name);
    assert_eq!(row["api"], api);
    let result = &row["result"];
    assert_eq!(result["kind"], case.expected, "{row}");
    if case.expected == "ok" {
        assert_eq!(result["returned"], true);
        assert_eq!(result["primary"], XML);
        if api.contains("set") {
            assert_eq!(result["extras"], json!([]));
        } else {
            assert_eq!(result["extras"], Json::Null);
        }
        if api.starts_with("bytes") {
            assert_eq!(result["original_return_bytes"], json!(XML.as_bytes()));
        }
        return;
    }
    assert_eq!(result["returned"], false);
    assert!(result.get("primary").is_none() && result.get("extras").is_none());
    assert_eq!(result["boundary_preserved"], true);
    assert_eq!(result["output"], Json::Null);
    assert_eq!(result["request"], Json::Null);
    assert!(
        result["chain"]
            .as_array()
            .is_some_and(|chain| !chain.is_empty())
    );
    if case.expected == "Mapping" {
        assert_eq!(result["input"], Json::Null);
        assert_eq!(
            result["mapping_exception"],
            json!({"node":2,"message":"must-not-map"})
        );
        assert_eq!(result["resource"], Json::Null);
        assert_eq!(result["boundary_has_source"], true);
    } else {
        assert_eq!(
            result["input"],
            json!({"kind":"Named","index":3,"name":"d"})
        );
        assert_eq!(
            result["mapping_exception"],
            Json::Null,
            "reached preflight must not evaluate the mapping sentinel"
        );
        if case.expected == "Input" {
            assert_eq!(
                result["resource"],
                json!({"resource":"xml_input_set_utf8_bytes","observed_count":268_435_457u64,"limit":268_435_456u64})
            );
            assert_eq!(result["boundary_bytes"], Json::Null);
            assert_eq!(result["boundary_limit"], Json::Null);
            assert_eq!(result["boundary_has_source"], true);
        } else {
            assert_eq!(result["resource"], Json::Null);
            assert_eq!(result["boundary_bytes"], 67_108_865u64);
            assert_eq!(result["boundary_limit"], 67_108_864u64);
            assert_eq!(
                result["boundary_has_source"], false,
                "DocumentLimit must precede malformed primary parsing and aggregate charging"
            );
        }
    }
}
fn exercise(large: bool) -> TestResult<()> {
    let _serial = SERIAL
        .lock()
        .map_err(|_| io::Error::other("XML boundary lock poisoned"))?;
    let mut directory = Originals::new()?;
    let deadline = Instant::now() + Duration::from_secs(if large { 1800 } else { 600 });
    let before = source_identities()?;
    json_file(
        &directory.path.join("source-and-selected-ELFs-before.json"),
        &before,
    )?;
    let result = caught(&directory.path, "test", || -> TestResult<()> {
        if large {
            assert_eq!(
                std::env::var_os("FERRULE_XML_INPUT_BOUNDARY_LARGE").as_deref(),
                Some(OsStr::new("1")),
                "explicit ignored boundary opt-in required"
            );
            assert_eq!(
                std::env::var_os("FERRULE_XML_INPUT_BOUNDARY_HEADROOM_REVIEWED").as_deref(),
                Some(OsStr::new("1")),
                "caller must coordinate all shared build/memory work first"
            );
            headroom(&directory.path, deadline, "before-large")?;
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
        if large {
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
        let (inputs, cases, _) = corpus(&directory.path, large)?;
        // Every large run first qualifies the same ASCII grammar through a small independent prototype.
        let proto = directory.path.join("prototype");
        std::fs::create_dir(&proto)?;
        let (proto_inputs, proto_cases, _) = corpus(&proto, false)?;
        assert_prototype(&project, &proto_inputs, &proto_cases, &proto)?;
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
            // Bind both actual libraries before the all-language small prerequisite cohort.
            for language in ["rust", "csharp"] {
                let language_dir = directory.path.join(language);
                std::fs::create_dir(&language_dir)?;
                let (path, generated) = prepare(&language_dir, language, &project, deadline)?;
                let library_before = artifact_files(&generated)?;
                let executable = build(&language_dir, language, &generated, deadline)?;
                let selected_binary = if language == "rust" {
                    PathBuf::from(executable.get_program())
                } else {
                    PathBuf::from(executable.get_args().next().ok_or("host DLL argument")?)
                };
                let binary_before = identity(&selected_binary)?;
                json_file(
                    &language_dir.join("selected-complete-host-identity-before.json"),
                    &binary_before,
                )?;
                libraries.push(Library {
                    language,
                    phase: language_dir,
                    project: path,
                    generated,
                    before: library_before,
                    command: executable,
                    binary: selected_binary,
                    binary_before,
                });
            }
            let cohorts = if large {
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
                    assert_eq!(seen.len(), 64);
                }
                for library in &libraries {
                    for case in controls {
                        for api in APIS {
                            let originals =
                                library.phase.join(format!("{cohort}-{}-{api}", case.name));
                            std::fs::create_dir(&originals)?;
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
                                .arg(&originals)
                                .current_dir(&library.phase);
                            let output =
                                recorded(&mut command, &originals, "host", deadline, 120, measure)?;
                            let memory = if measure {
                                Some(std::fs::read(originals.join("host-memory.txt"))?)
                            } else {
                                None
                            };
                            if let Some(memory) = &memory {
                                std::fs::write(
                                    originals.join("original-memory-observation.bin"),
                                    memory,
                                )?;
                            }
                            require_success(&output);
                            assert!(output.stderr.is_empty());
                            assert!(output.stdout.len() < 65536, "host never dumps input bodies");
                            let row: Json = serde_json::from_slice(&output.stdout)?;
                            assert_eq!(
                                row,
                                serde_json::from_slice::<Json>(&std::fs::read(
                                    originals.join("actual-public-result.json")
                                )?)?
                            );
                            assert!(originals.join("original-typed-result.txt").is_file());
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
                                    &originals.join("actual-memory-observation.json"),
                                    &json!({"peak_RSS_kib":peak,"scope":"one complete host process; not a product memory bound"}),
                                )?;
                            }
                            assert!(seen.insert((
                                library.language.to_owned(),
                                cohort.to_owned(),
                                case.name.to_owned(),
                                api.to_owned()
                            )));
                            assert_row(&row, case, api);
                        }
                    }
                }
            }
            Ok(())
        });
        std::fs::write(
            directory
                .path
                .join("original-complete-all-host-phases-result.txt"),
            format!("{host_result:#?}\n"),
        )?;
        // Every prepared host receives its afterguards before any ordinary host panic is returned.
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
                            &library
                                .phase
                                .join("selected-complete-host-identity-after.json"),
                            &binary_after,
                        )?;
                        assert_eq!(
                            binary_after, library.binary_before,
                            "selected actual host binary remains unchanged"
                        );
                        let library_after = artifact_files(&library.generated)?;
                        json_file(
                            &library.phase.join("library-after-whole-body-equality.json"),
                            &json!({"exact":library_after==library.before,"file_count":library_after.len()}),
                        )?;
                        assert_eq!(
                            library_after, library.before,
                            "generated library bodies remain untouched by host builds/calls"
                        );
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
            directory
                .path
                .join("original-complete-all-library-after-result.txt"),
            format!("{library_after_result:#?}\n"),
        )?;
        library_after_result?;
        host_result?;
        json_file(
            &directory
                .path
                .join("complete-unique-actual-route-inventory.json"),
            &json!(seen),
        )?;
        let cohorts = if large {
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
        assert_eq!(seen.len(), if large { 144 } else { 64 });
        if large {
            headroom(&directory.path, deadline, "after-large")?;
        }
        Ok(())
    });
    std::fs::write(
        directory.path.join("original-complete-test-result.txt"),
        format!("{result:#?}\n"),
    )?;
    let corpus_after = caught(&directory.path, "corpus-after", || -> TestResult<()> {
        let prototype_path = directory.path.join("prototype");
        for path in [directory.path.as_path(), prototype_path.as_path()] {
            let before_path = path.join("complete-input-body-identities-before.json");
            if before_path.is_file() {
                let original: Json = serde_json::from_slice(&std::fs::read(&before_path)?)?;
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
                    &json!(after),
                )?;
                assert_eq!(
                    json!(after),
                    original,
                    "complete original corpus bodies remain unchanged even on host refusal"
                );
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
    corpus_after?;
    result?;
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_xml_static_input_combined_bytes_small_public_prototype() -> TestResult<()> {
    exercise(false)
}
#[test]
#[ignore = "serial 256 MiB public XML input boundary; requires explicit headroom opt-in and GNU timeout/time"]
fn generated_xml_static_input_combined_bytes_exact_and_one_over_opt_in() -> TestResult<()> {
    exercise(true)
}
