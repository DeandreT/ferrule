use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::EngineError;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, NamedTarget, Node, Pipeline,
    PipelineInput, PipelineStage, Project, Scope, ScopeIteration, SequenceWindow, SortFilterOrder,
};
use mfd::{
    ExportCompatibility, ExportCompatibilityFeature as Feature, ExportProfile, ImportOptions,
    ImportProfile,
};

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_global_order_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn child(&self, label: &str) -> PathBuf {
        let path = self.0.join(label);
        std::fs::create_dir(&path).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!("retained global-order originals: {}", self.0.display());
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn retain(path: &Path, label: &str, value: &impl Debug) {
    std::fs::write(
        path.join(format!("{label}.debug.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn item(value: i64, blocked: bool, message: &str) -> Instance {
    group(vec![
        ("Value", Instance::Scalar(Value::Int(value))),
        ("Blocked", Instance::Scalar(Value::Bool(blocked))),
        ("Message", Instance::Scalar(Value::String(message.into()))),
    ])
}

fn input(first: bool, second: bool) -> Instance {
    group(vec![(
        "Item",
        Instance::Repeated(vec![
            item(10, first, "earlier-unselected"),
            item(20, second, "late-selected"),
        ]),
    )])
}

fn expected_rows(values: &[i64]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            values
                .iter()
                .map(|value| group(vec![("Result", Instance::Scalar(Value::Int(*value)))]))
                .collect(),
        ),
    )])
}

fn project(when_true: bool) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::Int),
                        SchemaNode::scalar("Blocked", ScalarType::Bool),
                        SchemaNode::scalar("Message", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Result", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: Default::default(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: vec!["Item".into()],
            },
            selection: if when_true {
                FailureSelection::WhenTrue { predicate: 2 }
            } else {
                FailureSelection::WhenFalse { predicate: 4 }
            },
            message: Some(3),
        }],
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Blocked".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Message".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "not".into(),
                        args: vec![2],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                filter: Some(4),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn evaluate(
    path: &Path,
    label: &str,
    project: &Project,
    source: &Instance,
) -> Result<Instance, EngineError> {
    std::fs::write(
        path.join(format!("{label}-project.json")),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    std::fs::write(
        path.join(format!("{label}-input.json")),
        serde_json::to_vec_pretty(source).unwrap(),
    )
    .unwrap();
    let result = engine::run(project, source);
    retain(path, label, &result);
    result
}

fn directory_bytes(path: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn reject_native(dir: &Directory, label: &str, project: &Project) {
    let validation = engine::validate(project);
    retain(&dir.0, &format!("{label}-validation"), &validation);
    assert!(validation.is_empty(), "{validation:?}");
    for sentinel in [false, true] {
        let path = dir.child(&format!(
            "{label}-{}",
            if sentinel { "sentinel" } else { "fresh" }
        ));
        if sentinel {
            for (name, bytes) in [
                ("design.mfd", &b"existing design"[..]),
                ("design-source.xsd", &b"existing source schema"[..]),
                ("design-target.xsd", &b"existing target schema"[..]),
            ] {
                std::fs::write(path.join(name), bytes).unwrap();
            }
        }
        let before = directory_bytes(&path);
        let report = mfd::preflight_export(project, &path.join("design.mfd"));
        retain(&dir.0, &format!("{label}-{sentinel}-preflight"), &report);
        let report = report.unwrap();
        assert_eq!(report.compatibility, ExportCompatibility::FerruleExtensions);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.feature == Feature::GlobalFailureOrdering)
        );
        assert_eq!(
            serde_json::to_value(Feature::GlobalFailureOrdering).unwrap(),
            "global_failure_ordering"
        );
        assert_eq!(directory_bytes(&path), before);
        let actual =
            mfd::export_with_profile(project, &path.join("design.mfd"), ExportProfile::NativeMfd);
        retain(
            &dir.0,
            &format!("{label}-{sentinel}-native-result"),
            &actual,
        );
        let mfd::MfdError::IncompatibleExport(actual) = actual.unwrap_err() else {
            panic!("expected typed native incompatibility");
        };
        assert_eq!(*actual, report);
        assert_eq!(directory_bytes(&path), before);
    }
}

fn cycles(dir: &Directory, label: &str, original: &Project, native: bool) -> Vec<Project> {
    let mut current = original.clone();
    let mut projects = Vec::new();
    for cycle in 0..2 {
        let path = dir.child(&format!("{label}-{cycle}"));
        let source_xml = format_xml::to_string(&current.source, &input(false, false));
        retain(&path, "source-xml-result", &source_xml);
        std::fs::write(path.join("input.xml"), source_xml.unwrap()).unwrap();
        let exported = mfd::export_with_profile(
            &current,
            &path.join("design.mfd"),
            if native {
                ExportProfile::NativeMfd
            } else {
                ExportProfile::FerruleExtensions
            },
        );
        retain(&path, "export-result", &exported);
        let report = exported.unwrap();
        assert!(report.warnings.is_empty(), "{report:?}");
        if native {
            assert!(report.is_native_compatible(), "{report:?}");
        }
        let imported = mfd::import_with_profile(
            &path.join("design.mfd"),
            &ImportOptions::default().with_package_root(&dir.0),
            ImportProfile::Executable,
        );
        match &imported {
            Ok(outcome) => {
                retain(&path, "import-report", &outcome.report);
                retain(&path, "import-warnings", &outcome.imported.warnings);
                std::fs::write(
                    path.join("imported-project.json"),
                    serde_json::to_vec_pretty(&outcome.imported.project).unwrap(),
                )
                .unwrap();
            }
            Err(error) => retain(&path, "import-error", error),
        }
        let imported = imported.unwrap();
        assert!(imported.report.executable && imported.report.issues.is_empty());
        assert!(imported.imported.warnings.is_empty());
        current = imported.imported.project;
        assert!(engine::validate(&current).is_empty());
        projects.push(current.clone());
    }
    projects
}

#[test]
fn total_primary_branches_keep_full_rows_first_late_failures_and_two_profile_cycles() {
    let dir = Directory::new("total");
    for when_true in [false, true] {
        let original = project(when_true);
        let mut projects = vec![original.clone()];
        projects.extend(cycles(
            &dir,
            &format!("false-selection-{when_true}-default"),
            &original,
            false,
        ));
        projects.extend(cycles(
            &dir,
            &format!("false-selection-{when_true}-native"),
            &original,
            true,
        ));
        for (index, current) in projects.iter().enumerate() {
            let actual = evaluate(
                &dir.0,
                &format!("total-{when_true}-{index}"),
                current,
                &input(false, false),
            );
            assert_eq!(actual, Ok(expected_rows(&[10, 20])));
            let output = format_xml::to_string(&current.target, &actual.unwrap());
            retain(&dir.0, &format!("total-xml-{when_true}-{index}"), &output);
            let expected =
                format_xml::to_string(&original.target, &expected_rows(&[10, 20])).unwrap();
            let output = output.unwrap();
            assert_eq!(output.as_bytes(), expected.as_bytes());
            assert_eq!(
                output,
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Row>\n    <Result>10</Result>\n  </Row>\n  <Row>\n    <Result>20</Result>\n  </Row>\n</Output>"
            );
            for (first, second, message) in [
                (true, false, "earlier-unselected"),
                (false, true, "late-selected"),
            ] {
                let actual = evaluate(
                    &dir.0,
                    &format!("selected-{when_true}-{index}-{first}"),
                    current,
                    &input(first, second),
                );
                assert_eq!(
                    actual,
                    Err(EngineError::MappingFailure {
                        rule: 1,
                        message: Some(message.into())
                    })
                );
            }
        }
    }
}

#[test]
fn exact_integer_predicate_position_and_reused_boolean_not_are_total() {
    let dir = Directory::new("integer-position");
    let mut original = project(true);
    original.graph.nodes.insert(
        2,
        Node::Call {
            function: "greater_than".into(),
            args: vec![0, 10],
        },
    );
    original.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(100),
        },
    );
    original.graph.nodes.insert(
        11,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    original.graph.nodes.insert(
        15,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    original.graph.nodes.insert(
        12,
        Node::Call {
            function: "not".into(),
            args: vec![4],
        },
    );
    original.failure_rules[0].message = Some(11);
    original.graph.nodes.insert(
        13,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    original.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Marker", ScalarType::Bool),
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Result", ScalarType::Int),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Allowed", ScalarType::Bool),
                    SchemaNode::scalar("BlockedEcho", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    original.root.bindings.push(Binding {
        target_field: "Marker".into(),
        node: 13,
    });
    original.root.children[0].bindings.extend([
        Binding {
            target_field: "Rank".into(),
            node: 15,
        },
        Binding {
            target_field: "Allowed".into(),
            node: 4,
        },
        Binding {
            target_field: "BlockedEcho".into(),
            node: 12,
        },
    ]);
    let mut projects = vec![original.clone()];
    projects.extend(cycles(&dir, "total-positions-default", &original, false));
    projects.extend(cycles(&dir, "total-positions-native", &original, true));
    let expected = group(vec![
        ("Marker", Instance::Scalar(Value::Bool(true))),
        (
            "Row",
            Instance::Repeated(vec![
                group(vec![
                    ("Result", Instance::Scalar(Value::Int(10))),
                    ("Rank", Instance::Scalar(Value::Int(1))),
                    ("Allowed", Instance::Scalar(Value::Bool(true))),
                    ("BlockedEcho", Instance::Scalar(Value::Bool(false))),
                ]),
                group(vec![
                    ("Result", Instance::Scalar(Value::Int(20))),
                    ("Rank", Instance::Scalar(Value::Int(2))),
                    ("Allowed", Instance::Scalar(Value::Bool(true))),
                    ("BlockedEcho", Instance::Scalar(Value::Bool(false))),
                ]),
            ]),
        ),
    ]);
    for (index, current) in projects.iter().enumerate() {
        assert_eq!(
            evaluate(
                &dir.0,
                &format!("positions-{index}"),
                current,
                &input(false, false)
            ),
            Ok(expected.clone())
        );
        let selected = group(vec![(
            "Item",
            Instance::Repeated(vec![item(10, false, "unused"), item(200, false, "unused")]),
        )]);
        assert_eq!(
            evaluate(
                &dir.0,
                &format!("position-selected-{index}"),
                current,
                &selected
            ),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("2".into())
            })
        );
    }
}

fn divide_target(mut project: Project) -> Project {
    project.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(0),
        },
    );
    project.graph.nodes.insert(
        11,
        Node::Call {
            function: "divide".into(),
            args: vec![0, 10],
        },
    );
    project.root.children[0].bindings[0].node = 11;
    project
}

#[test]
fn earlier_division_cannot_supplant_later_global_failure_in_strict_native_export() {
    let dir = Directory::new("observed-counterexample");
    for when_true in [false, true] {
        let original = divide_target(project(when_true));
        let actual = evaluate(
            &dir.0,
            &format!("counterexample-{when_true}"),
            &original,
            &input(false, true),
        );
        assert_eq!(
            actual,
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("late-selected".into())
            })
        );
        reject_native(&dir, &format!("counterexample-{when_true}"), &original);
        let mut projects = vec![original.clone()];
        projects.extend(cycles(
            &dir,
            &format!("counterexample-{when_true}-extensions"),
            &original,
            false,
        ));
        for (index, current) in projects.iter().enumerate() {
            assert_eq!(
                evaluate(
                    &dir.0,
                    &format!("counterexample-cycle-{when_true}-{index}"),
                    current,
                    &input(false, true)
                ),
                Err(EngineError::MappingFailure {
                    rule: 1,
                    message: Some("late-selected".into())
                })
            );
            let unselected = evaluate(
                &dir.0,
                &format!("counterexample-unselected-{when_true}-{index}"),
                current,
                &input(false, false),
            );
            assert!(
                matches!(unselected, Err(EngineError::Function(source)) if source.to_string() == "division by zero")
            );
        }
    }
}

#[test]
fn multiple_rule_priority_and_lazy_fallible_messages_stay_global_in_extensions() {
    let dir = Directory::new("rules-message");
    let mut multiple = project(false);
    multiple.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::String("second-rule".into()),
        },
    );
    // Rule 1 selects the later blocked item; rule 2 would select the first
    // unblocked item if the rules were incorrectly interleaved by item.
    multiple.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Item".into()],
        },
        selection: FailureSelection::WhenFalse { predicate: 2 },
        message: Some(10),
    });
    multiple.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("RowA", vec![SchemaNode::scalar("Result", ScalarType::Int)])
                .repeating(),
            SchemaNode::group("RowB", vec![SchemaNode::scalar("Result", ScalarType::Int)])
                .repeating(),
        ],
    );
    multiple.root.children[0].target_field = "RowA".into();
    multiple.root.children.push(Scope {
        target_field: "RowB".into(),
        iteration: ScopeIteration::Source(vec!["Item".into()]),
        filter: Some(2),
        bindings: vec![Binding {
            target_field: "Result".into(),
            node: 0,
        }],
        ..Scope::default()
    });
    reject_native(&dir, "multiple", &multiple);
    for (index, current) in cycles(&dir, "multiple-extensions", &multiple, false)
        .iter()
        .enumerate()
    {
        assert_eq!(
            evaluate(
                &dir.0,
                &format!("multiple-cycle-{index}"),
                current,
                &input(false, true)
            ),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("late-selected".into())
            })
        );
        assert_eq!(
            evaluate(
                &dir.0,
                &format!("multiple-second-rule-{index}"),
                current,
                &input(false, false)
            ),
            Err(EngineError::MappingFailure {
                rule: 2,
                message: Some("second-rule".into())
            })
        );
    }
    let mut lazy = divide_target(project(true));
    lazy.root.children[0].bindings[0].node = 0;
    lazy.failure_rules[0].message = Some(11);
    reject_native(&dir, "message", &lazy);
    let mut projects = vec![lazy.clone()];
    projects.extend(cycles(&dir, "message-extensions", &lazy, false));
    for (index, current) in projects.iter().enumerate() {
        assert_eq!(
            evaluate(
                &dir.0,
                &format!("lazy-unselected-{index}"),
                current,
                &input(false, false)
            ),
            Ok(expected_rows(&[10, 20]))
        );
        assert!(
            matches!(evaluate(&dir.0, &format!("lazy-selected-{index}"), current, &input(false, true)), Err(EngineError::Function(source)) if source.to_string() == "division by zero")
        );
    }
}

#[test]
fn whole_target_roots_boundaries_and_prepass_controls_require_proof() {
    let dir = Directory::new("whole-target");
    let mut root = divide_target(project(false));
    root.root.children[0].bindings[0].node = 0;
    root.graph.nodes.insert(
        12,
        Node::Const {
            value: Value::Int(1),
        },
    );
    root.graph.nodes.insert(
        14,
        Node::Const {
            value: Value::Int(10),
        },
    );
    root.graph.nodes.insert(
        11,
        Node::Call {
            function: "divide".into(),
            args: vec![14, 10],
        },
    );
    root.graph.nodes.insert(
        13,
        Node::Call {
            function: "greater_than".into(),
            args: vec![11, 12],
        },
    );
    let SchemaNode {
        kind: ir::SchemaKind::Group { children, .. },
        ..
    } = &mut root.target
    else {
        panic!()
    };
    children.insert(0, SchemaNode::scalar("Marker", ScalarType::Bool));
    root.root.bindings.push(Binding {
        target_field: "Marker".into(),
        node: 13,
    });
    reject_native(&dir, "root-before-rows", &root);
    assert_eq!(
        evaluate(&dir.0, "root-global-first", &root, &input(false, true)),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: Some("late-selected".into())
        })
    );
    let mut named = project(false);
    named.extra_targets.push(NamedTarget {
        name: "audit".into(),
        schema: named.target.clone(),
        path: Some("audit.xml".into()),
        options: Default::default(),
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    });
    reject_native(&dir, "extra-target", &named);
    let mut sort = project(false);
    sort.root.children[0].sort_by = Some(0);
    sort.root.children[0].sort_filter_order = SortFilterOrder::FilterThenSort;
    reject_native(&dir, "sort", &sort);
    let mut window = project(false);
    window.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(1),
        },
    );
    window.root.children[0]
        .windows
        .push(SequenceWindow::First { count: 10 });
    reject_native(&dir, "window", &window);
    let mut unused = divide_target(project(false));
    unused.root.children[0].bindings[0].node = 0;
    reject_native(&dir, "disconnected-fallible", &unused);
}

#[test]
fn presence_numeric_adaptation_and_unknown_predicates_are_not_assumed_total() {
    let dir = Directory::new("types");
    let mut absent = project(false);
    let ir::SchemaKind::Group { children, .. } = &mut absent.source.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    assert!(children[1].set_xml_optional(true));
    reject_native(&dir, "optional-predicate", &absent);
    let mut float = project(false);
    let ir::SchemaKind::Group { children, .. } = &mut float.target.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children[0] = SchemaNode::scalar("Result", ScalarType::Float);
    reject_native(&dir, "numeric-adaptation", &float);
    let mut predicate = divide_target(project(false));
    predicate.root.children[0].bindings[0].node = 0;
    predicate.graph.nodes.insert(
        12,
        Node::Const {
            value: Value::Int(1),
        },
    );
    predicate.graph.nodes.insert(
        2,
        Node::Call {
            function: "greater_than".into(),
            args: vec![11, 12],
        },
    );
    reject_native(&dir, "fallible-predicate", &predicate);
    assert!(
        matches!(evaluate(&dir.0, "predicate-original-cause", &predicate, &input(false, true)), Err(EngineError::Function(source)) if source.to_string() == "division by zero")
    );
    let mut no_rules = divide_target(project(false));
    no_rules.failure_rules.clear();
    no_rules.root.children[0].filter = None;
    let report = mfd::preflight_export(&no_rules, &dir.0.join("no-rule.mfd"));
    retain(&dir.0, "no-rule-preflight", &report);
    let report = report.unwrap();
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.feature == Feature::GlobalFailureOrdering)
    );
    assert!(report.is_native_compatible(), "{report:?}");
}

#[test]
fn connected_stage_report_reconstruction_cannot_drop_global_ordering_refusal() {
    let dir = Directory::new("pipeline");
    let mut second = project(false);
    let mut first = second.clone();
    first.failure_rules.clear();
    first.graph.nodes.remove(&4); // The copy stage no longer owns the old filter.
    first.target = first.source.clone();
    first.target.name = "Bridge".into();
    first.target_path = Some("intermediate.xml".into());
    second.source = first.target.clone();
    second.source_path = Some("intermediate.xml".into());
    first.root.children[0].target_field = "Item".into();
    first.root.children[0].filter = None;
    first.root.children[0].bindings = vec![
        Binding {
            target_field: "Value".into(),
            node: 0,
        },
        Binding {
            target_field: "Blocked".into(),
            node: 2,
        },
        Binding {
            target_field: "Message".into(),
            node: 3,
        },
    ];
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![
            PipelineStage {
                id: "copy".into(),
                mapping_path: None,
                project: first,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "guarded".into(),
                mapping_path: None,
                project: second,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    let validation = engine::validate_pipeline(&pipeline);
    retain(&dir.0, "pipeline-validation", &validation);
    assert!(validation.is_empty(), "{validation:?}");
    let fresh = dir.child("strict");
    let before = directory_bytes(&fresh);
    let report = mfd::preflight_pipeline_export(&pipeline, &fresh.join("design.mfd"));
    retain(&dir.0, "pipeline-preflight", &report);
    let report = report.unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.feature == Feature::GlobalFailureOrdering
                && issue.component.contains("guarded"))
    );
    let native = mfd::export_pipeline_with_profile(
        &pipeline,
        &fresh.join("design.mfd"),
        ExportProfile::NativeMfd,
    );
    retain(&dir.0, "pipeline-native-result", &native);
    let mfd::MfdError::IncompatibleExport(actual) = native.unwrap_err() else {
        panic!()
    };
    assert_eq!(*actual, report);
    assert_eq!(directory_bytes(&fresh), before);
    let ordinary = dir.child("extensions");
    let exported = mfd::export_pipeline(&pipeline, &ordinary.join("design.mfd"));
    retain(&dir.0, "pipeline-extension-export", &exported);
    assert!(exported.unwrap().is_empty());
    let imported = mfd::import_pipeline(&ordinary.join("design.mfd"));
    match &imported {
        Ok(outcome) => {
            retain(&dir.0, "pipeline-extension-warnings", &outcome.warnings);
            std::fs::write(
                dir.0.join("pipeline-imported.json"),
                serde_json::to_vec_pretty(&outcome.pipeline).unwrap(),
            )
            .unwrap();
        }
        Err(error) => retain(&dir.0, "pipeline-extension-import-error", error),
    }
    let imported = imported.unwrap();
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    assert_eq!(
        imported
            .pipeline
            .stages
            .iter()
            .map(|stage| stage.project.failure_rules.len())
            .sum::<usize>(),
        1
    );
}

#[test]
fn oversized_and_deep_total_graphs_refuse_strict_native_without_publication() {
    let dir = Directory::new("proof-budgets");
    let mut oversized = project(true);
    for node in 10_000..14_097 {
        oversized.graph.nodes.insert(
            node,
            Node::Const {
                value: Value::Bool(true),
            },
        );
    }
    reject_native(&dir, "graph-count", &oversized);

    let mut deep = project(true);
    let mut previous = 2;
    for node in 100..280 {
        deep.graph.nodes.insert(
            node,
            Node::Call {
                function: "not".into(),
                args: vec![previous],
            },
        );
        previous = node;
    }
    let ir::SchemaKind::Group { children, .. } = &mut deep.target.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        panic!()
    };
    children.push(SchemaNode::scalar("Echo", ScalarType::Bool));
    deep.root.children[0].bindings.push(Binding {
        target_field: "Echo".into(),
        node: previous,
    });
    reject_native(&dir, "dependency-depth", &deep);
}
