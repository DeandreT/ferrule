use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiBoundaryKind, EdiValueConstraint, FormatOptions, Graph, Node, Project, Scope,
};

struct Directory(PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("ferrule_edi_source_{label}_{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(explicit: bool) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("ISA", vec![]),
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::group(
                        "G62",
                        vec![
                            SchemaNode::scalar("G6201", ScalarType::String),
                            SchemaNode::scalar("G6202", ScalarType::String),
                            SchemaNode::scalar("G6203", ScalarType::String),
                        ],
                    )],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        source_path: Some("input.edi".into()),
        target_path: Some("output.json".into()),
        source_options: FormatOptions {
            edi_kind: explicit.then_some(EdiBoundaryKind::X12),
            lenient_segments: true,
            edi_value_constraints: vec![
                EdiValueConstraint::new(
                    vec!["Rows".into(), "G62".into(), "G6203".into()],
                    6,
                    6,
                    vec!["173000".into()],
                )
                .unwrap(),
            ],
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        graph: Graph {
            nodes: vec![(
                0,
                Node::Const {
                    value: Value::String("mapped".into()),
                },
            )]
            .into_iter()
            .collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "value".into(),
                node: 0,
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn wire(value: &str) -> String {
    format!(
        "ISA*00*          *00*          *ZZ*SOURCE         *ZZ*DEST           *261102*1200*^*00501*000000001*0*T*:~G62*10*20261102*{value}~"
    )
}

fn save(directory: &Path, project: &Project) -> PathBuf {
    let path = directory.join("mapping.json");
    std::fs::write(&path, serde_json::to_vec(project).unwrap()).unwrap();
    path
}

#[test]
fn file_cli_refuses_constraints_without_replacing_output() {
    let directory = Directory::new("file");
    for explicit in [false, true] {
        let project = project(explicit);
        let mapping = save(&directory.0, &project);
        let input = directory.0.join(if explicit {
            "input.capture"
        } else {
            "input.edi"
        });
        let output = directory.0.join("output.json");
        for (value, valid) in [("173000", true), ("1730", false), ("173000", true)] {
            std::fs::write(&input, wire(value)).unwrap();
            std::fs::write(&output, "preserved").unwrap();
            let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
                .arg("run")
                .arg(&mapping)
                .arg(&input)
                .arg(&output)
                .output()
                .unwrap();
            assert_eq!(
                result.status.success(),
                valid,
                "explicit={explicit}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if valid {
                assert!(std::fs::read_to_string(&output).unwrap().contains("mapped"));
            } else {
                assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserved");
                let error = String::from_utf8_lossy(&result.stderr);
                assert!(
                    error.contains("Rows[1]/G62/G6203")
                        && error.contains("minimum is 6")
                        && error.contains("code-list"),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn stdin_cli_refuses_constraints_without_partial_stdout() {
    let directory = Directory::new("stdin");
    for explicit in [false, true] {
        let mapping = save(&directory.0, &project(explicit));
        let mut child = Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .arg("run")
            .arg(&mapping)
            .args(["-", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(wire("1730").as_bytes())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(!result.status.success(), "explicit={explicit}");
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Rows[1]/G62/G6203"));
    }
}

#[derive(Default)]
struct Observe(std::cell::Cell<usize>);
impl engine::DebugHook for Observe {
    fn before_target_write(&self, _: &engine::PendingTargetWrite) -> engine::DebugDecision {
        self.0.set(self.0.get() + 1);
        engine::DebugDecision::Resume
    }
    fn after_node_value(&self, _: &engine::PendingNodeValue) -> engine::DebugDecision {
        self.0.set(self.0.get() + 1);
        engine::DebugDecision::Resume
    }
}

#[test]
fn payload_profiles_validate_before_mapping_and_do_not_retain_failure_state() {
    for profile in 0..3 {
        let project = project(true);
        let observe = Observe::default();
        for (value, valid) in [("173000", true), ("1730", false), ("173000", true)] {
            let wire = wire(value);
            let primary =
                cli::PayloadDocument::new(Path::new("input.edi"), wire.as_bytes()).unwrap();
            let options = cli::PayloadRunOptions::new(primary).with_debug_hook(&observe);
            let options = match profile {
                1 => options.with_bounded_tabular(),
                2 => options.with_bounded_documents(),
                _ => options,
            };
            observe.0.set(0);
            let result = cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &options,
            );
            if valid {
                let result = result.unwrap();
                assert_eq!(result.artifacts.len(), 1);
                assert!(observe.0.get() > 0);
            } else {
                let error = result.unwrap_err();
                assert_eq!(observe.0.get(), 0, "mapping must not run");
                let Some(format_edi::EdiFormatError::Validation(report)) =
                    error.downcast_ref::<format_edi::EdiFormatError>()
                else {
                    panic!("{error:#}")
                };
                assert_eq!(report.issues()[0].location(), "Rows[1]/G62/G6203");
            }
        }
    }
}

#[test]
fn numeric_file_and_payload_routes_validate_wire_before_typing_and_scaling() {
    let directory = Directory::new("numbers");
    for (ty, wire_value, expected, implied) in [
        (ScalarType::Int, "000001", serde_json::json!(1), false),
        (ScalarType::Float, "001234", serde_json::json!(12.34), true),
    ] {
        let mut project = project(true);
        project.source = SchemaNode::group(
            "Input",
            vec![SchemaNode::group(
                "G62",
                vec![SchemaNode::scalar("Value", ty)],
            )],
        );
        project.target = SchemaNode::group("Result", vec![SchemaNode::scalar("value", ty)]);
        project.graph.nodes.insert(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["G62".into(), "Value".into()],
            },
        );
        project.source_options.edi_value_constraints = vec![
            EdiValueConstraint::new(
                vec!["G62".into(), "Value".into()],
                6,
                6,
                vec![wire_value.into()],
            )
            .unwrap(),
        ];
        if implied {
            project.source_options.edi_implied_decimals.push(
                mapping::EdiImpliedDecimal::new(vec!["G62".into(), "Value".into()], 2).unwrap(),
            );
        }
        let input = directory.0.join("input.capture");
        let output = directory.0.join("output.json");
        let wire = wire(wire_value).replace("G62*10*20261102*", "G62*");
        std::fs::write(&input, &wire).unwrap();
        let mapping = save(&directory.0, &project);
        cli::run_project(&mapping, &input, &output).unwrap();
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(actual["value"], expected);
        let options = cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(Path::new("input.capture"), wire.as_bytes()).unwrap(),
        );
        let result =
            cli::run_project_value_payloads(&project, Path::new("/virtual/project.json"), &options)
                .unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&result.artifacts[0].bytes).unwrap();
        assert_eq!(actual["value"], expected);
    }
}

#[test]
fn inferred_routes_keep_existing_numeric_scaling_policy_with_or_without_constraints() {
    let directory = Directory::new("inferred_scale");
    for constrained in [false, true] {
        let mut project = project(false);
        project.source = SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("ISA", vec![]),
                SchemaNode::group("G62", vec![SchemaNode::scalar("Value", ScalarType::Float)]),
            ],
        );
        project.target = SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("value", ScalarType::Float)],
        );
        project.graph.nodes.insert(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["G62".into(), "Value".into()],
            },
        );
        project.source_options.edi_value_constraints = if constrained {
            vec![
                EdiValueConstraint::new(
                    vec!["G62".into(), "Value".into()],
                    6,
                    6,
                    vec!["001234".into()],
                )
                .unwrap(),
            ]
        } else {
            vec![]
        };
        project
            .source_options
            .edi_implied_decimals
            .push(mapping::EdiImpliedDecimal::new(vec!["G62".into(), "Value".into()], 2).unwrap());
        let wire = wire("001234").replace("G62*10*20261102*", "G62*");
        let input = directory.0.join("input.edi");
        let output = directory.0.join("output.json");
        std::fs::write(&input, &wire).unwrap();
        let mapping = save(&directory.0, &project);
        cli::run_project(&mapping, &input, &output).unwrap();
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(actual["value"], serde_json::json!(1234.0));
        let options = cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(Path::new("input.edi"), wire.as_bytes()).unwrap(),
        );
        let result =
            cli::run_project_value_payloads(&project, Path::new("/virtual/project.json"), &options)
                .unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&result.artifacts[0].bytes).unwrap();
        assert_eq!(actual["value"], serde_json::json!(1234.0));
    }
}

#[test]
fn inferred_non_x12_families_validate_wire_and_keep_unscaled_outputs() {
    let directory = Directory::new("inferred_families");
    for (extension, header, tag, prefix, suffix) in [
        ("edifact", "UNH", "BGM", "UNH+1'BGM+", "'"),
        ("hl7", "MSH", "PID", "MSH|^~\\&|SOURCE\rPID|", "\r"),
        ("edi", "STX", "DTL", "STX=ANA:1'DTL=", "'"),
    ] {
        for constrained in [false, true] {
            let mut project = project(false);
            let header_fields = if header == "MSH" {
                vec![
                    SchemaNode::scalar("Separator", ScalarType::String),
                    SchemaNode::scalar("Encoding", ScalarType::String),
                ]
            } else {
                vec![]
            };
            project.source = SchemaNode::group(
                "Input",
                vec![
                    SchemaNode::group(header, header_fields),
                    SchemaNode::group(tag, vec![SchemaNode::scalar("Value", ScalarType::Float)]),
                ],
            );
            project.source_path = Some(format!("input.{extension}"));
            project.target = SchemaNode::group(
                "Result",
                vec![SchemaNode::scalar("value", ScalarType::Float)],
            );
            project.graph.nodes.insert(
                0,
                Node::SourceField {
                    frame: None,
                    path: vec![tag.into(), "Value".into()],
                },
            );
            project.source_options.edi_value_constraints = if constrained {
                vec![
                    EdiValueConstraint::new(
                        vec![tag.into(), "Value".into()],
                        6,
                        6,
                        vec!["001234".into()],
                    )
                    .unwrap(),
                ]
            } else {
                vec![]
            };
            project.source_options.edi_implied_decimals =
                vec![mapping::EdiImpliedDecimal::new(vec![tag.into(), "Value".into()], 2).unwrap()];
            assert_eq!(project.source_options.edi_kind, None);
            let mapping = save(&directory.0, &project);
            let input = directory.0.join(format!("input.{extension}"));
            let output = directory.0.join("output.json");
            for raw in ["001234", "1234"] {
                let valid = !constrained || raw == "001234";
                let wire = format!("{prefix}{raw}{suffix}");
                std::fs::write(&input, &wire).unwrap();
                std::fs::write(&output, "preserved").unwrap();
                let file = cli::run_project(&mapping, &input, &output);
                let options = cli::PayloadRunOptions::new(
                    cli::PayloadDocument::new(
                        Path::new(project.source_path.as_deref().unwrap()),
                        wire.as_bytes(),
                    )
                    .unwrap(),
                );
                let payload = cli::run_project_value_payloads(&project, &mapping, &options);
                if valid {
                    assert_eq!(file.unwrap(), 1);
                    let expected = serde_json::json!({"value": 1234.0});
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(
                            &std::fs::read(&output).unwrap()
                        )
                        .unwrap(),
                        expected
                    );
                    let payload = payload.unwrap();
                    assert_eq!(payload.artifacts.len(), 1);
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&payload.artifacts[0].bytes)
                            .unwrap(),
                        expected
                    );
                } else {
                    assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserved");
                    for error in [file.unwrap_err(), payload.unwrap_err()] {
                        let Some(format_edi::EdiFormatError::Validation(report)) =
                            error.downcast_ref::<format_edi::EdiFormatError>()
                        else {
                            panic!("{header}: {error:#}")
                        };
                        assert_eq!(
                            report
                                .issues()
                                .iter()
                                .map(|issue| (issue.location().to_string(), issue.violation()))
                                .collect::<Vec<_>>(),
                            vec![
                                (
                                    format!("{tag}/Value"),
                                    format_edi::EdiConstraintViolation::TooShort {
                                        minimum: 6,
                                        actual: 4
                                    }
                                ),
                                (
                                    format!("{tag}/Value"),
                                    format_edi::EdiConstraintViolation::NotAllowed {
                                        allowed_count: 1
                                    }
                                ),
                            ]
                        );
                        assert!(!report.truncated());
                    }
                }
            }
        }
    }
}

fn conditional_dynamic_edi_project() -> Project {
    let mut project = project(true);
    project.source = SchemaNode::group("Input", vec![SchemaNode::scalar("Use", ScalarType::Bool)]);
    project.source_path = Some("input.json".into());
    project.extra_sources.push(mapping::NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: SchemaNode::group(
            "Document",
            vec![
                SchemaNode::group("G62", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        ),
        options: FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            lenient_segments: true,
            edi_value_constraints: vec![
                EdiValueConstraint::new(
                    vec!["G62".into(), "Value".into()],
                    6,
                    6,
                    vec!["173000".into()],
                )
                .unwrap(),
            ],
            ..Default::default()
        },
        dynamic_path: Some(mapping::DynamicSourcePath::legacy(1, vec![])),
    });
    project.source_options = FormatOptions {
        json_document: true,
        ..Default::default()
    };
    project.graph.nodes = [
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["Use".into()],
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("dynamic.edi".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                frame: Some(vec!["document".into(), "G62".into()]),
                path: vec!["Value".into()],
            },
        ),
    ]
    .into_iter()
    .collect();
    project.target = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::group(
                "Conditional",
                vec![
                    SchemaNode::group("Row", vec![SchemaNode::scalar("value", ScalarType::String)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    project.root = Scope {
        children: vec![Scope {
            target_field: "Conditional".into(),
            iteration: mapping::ScopeIteration::OptionalPresence { condition: 0 },
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: mapping::ScopeIteration::Source(vec!["document".into(), "G62".into()]),
                bindings: vec![Binding {
                    target_field: "value".into(),
                    node: 2,
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    project
}

fn run_dynamic_edi_payload(
    use_document: bool,
    document: Option<&[u8]>,
) -> anyhow::Result<cli::PayloadRunOutcome> {
    let primary = serde_json::to_vec(&serde_json::json!({"Use": use_document}))?;
    let named = document
        .map(|bytes| {
            cli::NamedPayloadInput::new(
                "document",
                cli::PayloadDocument::new(Path::new("dynamic.edi"), bytes)?,
            )
        })
        .transpose()?
        .into_iter()
        .collect::<Vec<_>>();
    let options = cli::PayloadRunOptions::new(cli::PayloadDocument::new(
        Path::new("input.json"),
        &primary,
    )?)
    .with_extra_sources(&named);
    cli::run_project_value_payloads(
        &conditional_dynamic_edi_project(),
        Path::new("/virtual/project.json"),
        &options,
    )
}

#[test]
fn dynamic_edi_payload_controls_load_valid_and_skip_unsupplied_unused_inputs() {
    let valid = wire("173000").replace("G62*10*20261102*", "G62*");
    for (use_document, document, expected) in [
        (
            true,
            Some(valid.as_bytes()),
            serde_json::json!({"Conditional":[{"Row":[{"value":"173000"}]}]}),
        ),
        (false, None, serde_json::json!({"Conditional":[]})),
    ] {
        let result = run_dynamic_edi_payload(use_document, document).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&result.artifacts[0].bytes).unwrap(),
            expected
        );
    }
}

fn assert_dynamic_edi_violations(report: &format_edi::EdiValidationReport) {
    assert_eq!(
        report
            .issues()
            .iter()
            .map(|issue| (issue.location(), issue.violation()))
            .collect::<Vec<_>>(),
        vec![
            (
                "G62[1]/Value",
                format_edi::EdiConstraintViolation::TooShort {
                    minimum: 6,
                    actual: 4
                }
            ),
            (
                "G62[1]/Value",
                format_edi::EdiConstraintViolation::NotAllowed { allowed_count: 1 }
            ),
        ]
    );
    assert!(!report.truncated());
}

#[test]
fn supplied_dynamic_edi_payloads_are_validated_before_conditional_execution() {
    let invalid = wire("1730").replace("G62*10*20261102*", "G62*");
    // Supplied documents are decoded during admission, even on an untaken branch.
    for use_document in [false, true] {
        let error = run_dynamic_edi_payload(use_document, Some(invalid.as_bytes()))
            .expect_err("invalid supplied payload must fail admission without artifacts");
        assert_eq!(error.to_string(), "reading payload source `document`");
        assert!(error.downcast_ref::<engine::EngineError>().is_none());
        let Some(format_edi::EdiFormatError::Validation(report)) =
            error.downcast_ref::<format_edi::EdiFormatError>()
        else {
            panic!("typed payload admission cause: {error:#}")
        };
        assert_dynamic_edi_violations(report);
    }
}

#[test]
fn filesystem_dynamic_edi_constraints_are_lazy_and_retain_typed_source_context() {
    let directory = Directory::new("dynamic_file");
    let mapping = save(&directory.0, &conditional_dynamic_edi_project());
    let input = directory.0.join("input.json");
    let output = directory.0.join("output.json");
    let dynamic = directory.0.join("dynamic.edi");
    std::fs::write(&dynamic, wire("1730").replace("G62*10*20261102*", "G62*")).unwrap();
    for use_document in [false, true, false] {
        std::fs::write(
            &input,
            serde_json::to_vec(&serde_json::json!({"Use": use_document})).unwrap(),
        )
        .unwrap();
        std::fs::write(&output, "preserved").unwrap();
        let result = cli::run_project(&mapping, &input, &output);
        if !use_document {
            assert_eq!(result.unwrap(), 1);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&std::fs::read(&output).unwrap())
                    .unwrap(),
                serde_json::json!({"Conditional":[]})
            );
            continue;
        }
        let error = result.expect_err("requested invalid file must fail without publication");
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserved");
        let Some(engine::EngineError::DynamicSourceFailure(failure)) =
            error.downcast_ref::<engine::EngineError>()
        else {
            panic!("typed dynamic source wrapper: {error:#}")
        };
        assert_eq!(failure.source_name, "document");
        assert_eq!(failure.path, "dynamic.edi");
        let read = failure
            .fault
            .find_cause::<cli::DynamicSourceReadError>()
            .expect("filesystem reader context");
        assert_eq!(
            read.to_string(),
            format!("reading input {}", dynamic.display())
        );
        let Some(format_edi::EdiFormatError::Validation(report)) =
            failure.fault.find_cause::<format_edi::EdiFormatError>()
        else {
            panic!("typed EDI validation cause: {failure:?}")
        };
        assert_dynamic_edi_violations(report);
    }
}

#[test]
fn remaining_segment_families_and_idoc_use_the_same_file_and_payload_constraints() {
    use mapping::{IdocFieldLayout, IdocLayout, IdocSegmentLayout};
    use std::num::NonZeroU32;
    let directory = Directory::new("families");
    for (kind, tag, prefix, suffix) in [
        (EdiBoundaryKind::Edifact, "BGM", "UNH+1'BGM+", "'"),
        (EdiBoundaryKind::Hl7, "PID", "MSH|^~\\&|SOURCE\rPID|", "\r"),
        (EdiBoundaryKind::Tradacoms, "DTL", "STX=ANA:1'DTL=", "'"),
        (EdiBoundaryKind::Idoc, "ITEM000001", "ITEM000001 ", "  \r"),
    ] {
        let mut project = project(true);
        project.source = SchemaNode::group(
            "Input",
            vec![SchemaNode::group(
                tag,
                vec![SchemaNode::scalar("Code", ScalarType::Int)],
            )],
        );
        project.source_options.edi_kind = Some(kind);
        project.source_options.edi_value_constraints = vec![
            EdiValueConstraint::new(vec![tag.into(), "Code".into()], 6, 6, vec!["000001".into()])
                .unwrap(),
        ];
        if kind == EdiBoundaryKind::Idoc {
            project.source_options.idoc = Some(
                IdocLayout::new(vec![
                    IdocSegmentLayout::new(
                        tag,
                        vec![
                            IdocFieldLayout::new(
                                "Code",
                                NonZeroU32::new(12).unwrap(),
                                NonZeroU32::new(19).unwrap(),
                            )
                            .unwrap(),
                        ],
                    )
                    .unwrap(),
                ])
                .unwrap(),
            );
        }
        let mapping = save(&directory.0, &project);
        let input = directory.0.join("input.capture");
        let output = directory.0.join("output.json");
        for (value, valid) in [("000001", true), ("1", false)] {
            let wire = format!("{prefix}{value}{suffix}");
            std::fs::write(&input, &wire).unwrap();
            std::fs::write(&output, "preserved").unwrap();
            let result = cli::run_project(&mapping, &input, &output);
            assert_eq!(result.is_ok(), valid, "{kind:?}: {result:?}");
            if !valid {
                assert_eq!(std::fs::read_to_string(&output).unwrap(), "preserved");
            }
            let options = cli::PayloadRunOptions::new(
                cli::PayloadDocument::new(Path::new("input.capture"), wire.as_bytes()).unwrap(),
            );
            let result = cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &options,
            );
            assert_eq!(result.is_ok(), valid, "{kind:?}: {result:?}");
        }
    }
}

#[test]
fn named_sources_and_primary_document_members_enforce_constraints() {
    let directory = Directory::new("named_primary");
    let mut source = project(true);
    source.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::group(
            "G62",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )],
    );
    source.source_options.edi_value_constraints =
        vec![EdiValueConstraint::new(vec!["G62".into(), "Value".into()], 6, 6, vec![]).unwrap()];
    let mut project = project(true);
    project.source = SchemaNode::group("Input", vec![]);
    project.source_options = FormatOptions {
        json_document: true,
        ..Default::default()
    };
    project.extra_sources.push(mapping::NamedSource {
        name: "edi".into(),
        path: "named.capture".into(),
        schema: source.source,
        options: source.source_options,
        dynamic_path: None,
    });
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            frame: None,
            path: vec!["edi".into(), "G62".into(), "Value".into()],
        },
    );
    let mapping = save(&directory.0, &project);
    let input = directory.0.join("input.json");
    let output = directory.0.join("output.json");
    std::fs::write(&input, "{}").unwrap();
    let bad = wire("1").replace("G62*10*20261102*", "G62*");
    std::fs::write(directory.0.join("named.capture"), &bad).unwrap();
    let error = cli::run_project(&mapping, &input, &output).unwrap_err();
    assert!(format!("{error:#}").contains("G62/Value"));
    assert!(!output.exists());
    let named = [cli::NamedPayloadInput::new(
        "edi",
        cli::PayloadDocument::new(Path::new("named.capture"), bad.as_bytes()).unwrap(),
    )
    .unwrap()];
    let options = cli::PayloadRunOptions::new(
        cli::PayloadDocument::new(Path::new("input.json"), b"{}").unwrap(),
    )
    .with_extra_sources(&named);
    let error = cli::run_project_value_payloads(&project, &mapping, &options).unwrap_err();
    assert!(format!("{error:#}").contains("G62/Value"));

    let schema = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "MSH",
                vec![
                    SchemaNode::scalar("Separator", ScalarType::String),
                    SchemaNode::scalar("Encoding", ScalarType::String),
                ],
            ),
            SchemaNode::group("PID", vec![SchemaNode::scalar("Code", ScalarType::String)]),
        ],
    );
    project.source = schema.clone();
    project.target = schema;
    project.extra_sources.clear();
    project.source_options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Hl7),
        edi_value_constraints: vec![
            EdiValueConstraint::new(vec!["PID".into(), "Code".into()], 6, 6, vec![]).unwrap(),
        ],
        ..Default::default()
    };
    project.target_options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Hl7),
        ..Default::default()
    };
    project.graph.nodes.clear();
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("input.hl7".into()),
        },
    );
    project.root = Scope {
        construction: mapping::ScopeConstruction::CopyCurrentSource,
        ..Default::default()
    };
    let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(1));
    std::fs::write(directory.0.join("input.hl7"), "MSH|^~\\&\rPID|1\r").unwrap();
    let error = cli::execute_primary_document(
        &document,
        &mapping,
        &cli::PrimaryDocumentPolicy::new(&directory.0),
        None,
    )
    .unwrap_err();
    assert!(format!("{error}").contains("PID/Code"), "{error}");
}

#[test]
fn swift_generic_source_constraints_are_refused_before_loading_or_decoding() {
    use mapping::{
        SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtLayout, SwiftValueExpr,
    };
    let directory = Directory::new("swift");
    let mut project = project(true);
    project.source = SchemaNode::group(
        "SWIFT",
        vec![
            SchemaNode::group(
                "Message",
                vec![SchemaNode::group(
                    "MT950",
                    vec![SchemaNode::scalar("Code", ScalarType::String)],
                )],
            )
            .repeating(),
        ],
    );
    project.source_options = FormatOptions {
        swift_mt: Some(
            SwiftMtLayout::new(vec![SwiftMessageLayout::new(
                "MT950",
                vec![SwiftFieldLayout::new(
                    "20",
                    vec!["MT950".into(), "Code".into()],
                    false,
                    SwiftValueExpr::Capture {
                        path: vec![],
                        min: 1,
                        max: 16,
                        charset: SwiftCharset::Text,
                    },
                )],
            )])
            .unwrap(),
        ),
        edi_value_constraints: vec![
            EdiValueConstraint::new(
                vec!["Message".into(), "MT950".into(), "Code".into()],
                6,
                6,
                vec![],
            )
            .unwrap(),
        ],
        ..Default::default()
    };
    let mapping = save(&directory.0, &project);
    let error = cli::run_project(
        &mapping,
        &directory.0.join("missing.capture"),
        &directory.0.join("output.json"),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("does not support `edi_value_constraints`"),
        "{error:#}"
    );
    let options = cli::PayloadRunOptions::new(
        cli::PayloadDocument::new(Path::new("input.capture"), b"\xff").unwrap(),
    );
    let error = cli::run_project_value_payloads(&project, &mapping, &options).unwrap_err();
    assert!(
        format!("{error:#}").contains("does not support `edi_value_constraints`"),
        "{error:#}"
    );
    let error = cli::plan_payload_run(&project, &mapping, &options).unwrap_err();
    assert!(format!("{error:#}").contains("does not support `edi_value_constraints`"));
    struct NoRead;
    impl std::io::Read for NoRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("unsupported source metadata must refuse before stdin is read");
        }
    }
    let options = cli::StandardIoRunOptions::new()
        .with_input_path(Path::new("-"))
        .with_output_path(Path::new("-"));
    let error =
        cli::run_project_with_standard_streams(&mapping, &options, &mut NoRead).unwrap_err();
    assert!(format!("{error:#}").contains("does not support `edi_value_constraints`"));
}
