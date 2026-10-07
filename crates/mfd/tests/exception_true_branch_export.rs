use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use engine::EngineError;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, IterationOutput, NamedSource,
    Node, Project, Scope, ScopeConstruction, ScopeIteration, SequenceExpr, SequenceWindow,
};
use mfd::{ExportProfile, ImportOptions, ImportProfile};

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_true_exception_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
            eprintln!("retained true-exception originals: {}", self.0.display());
            return;
        }
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn retain(path: &Path, label: &str, value: &impl Debug) {
    let text = format!("{value:#?}\n");
    std::fs::write(path.join(format!("{label}.debug.txt")), &text).unwrap();
    eprintln!("{label}: {text}");
}

fn optional(mut schema: SchemaNode) -> SchemaNode {
    assert!(schema.set_xml_optional(true));
    schema
}

fn project(message: Option<u32>) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::Int),
                        SchemaNode::scalar("Blocked", ScalarType::Bool),
                        optional(SchemaNode::scalar("Message", ScalarType::String)),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Result", ScalarType::Int),
                        optional(SchemaNode::scalar("Allowed", ScalarType::Bool)),
                        optional(SchemaNode::scalar("Rank", ScalarType::Int)),
                    ],
                )
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
            selection: FailureSelection::WhenTrue { predicate: 2 },
            message,
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

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn item(value: i64, blocked: Value, message: &str) -> Instance {
    group(vec![
        ("Value", Instance::Scalar(Value::Int(value))),
        ("Blocked", Instance::Scalar(blocked)),
        ("Message", Instance::Scalar(Value::String(message.into()))),
    ])
}

fn input(items: Vec<Instance>) -> Instance {
    group(vec![("Item", Instance::Repeated(items))])
}

fn all_false() -> Instance {
    input(vec![
        item(10, Value::Bool(false), "unused-first"),
        item(20, Value::Bool(false), "unused-second"),
    ])
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

fn capture_run(
    path: &Path,
    label: &str,
    project: &Project,
    input: &Instance,
) -> Result<Instance, EngineError> {
    std::fs::write(
        path.join(format!("{label}-project.json")),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    std::fs::write(
        path.join(format!("{label}-input.json")),
        serde_json::to_vec_pretty(input).unwrap(),
    )
    .unwrap();
    let xml = format_xml::to_string(&project.source, input);
    retain(path, &format!("{label}-input-xml-result"), &xml);
    if let Ok(xml) = xml {
        std::fs::write(path.join(format!("{label}-input.xml")), xml).unwrap();
    }
    let result = engine::run(project, input);
    retain(path, label, &result);
    if let Ok(output) = &result {
        std::fs::write(
            path.join(format!("{label}-output.json")),
            serde_json::to_vec_pretty(output).unwrap(),
        )
        .unwrap();
        let xml = format_xml::to_string(&project.target, output);
        retain(path, &format!("{label}-output-xml-result"), &xml);
        if let Ok(xml) = xml {
            std::fs::write(path.join(format!("{label}-output.xml")), xml).unwrap();
        }
    }
    result
}

fn assert_identity(project: &Project) -> u32 {
    let [rule] = project.failure_rules.as_slice() else {
        panic!("expected one rule")
    };
    let FailureSelection::WhenTrue { predicate } = rule.selection else {
        panic!("original true polarity lost")
    };
    assert!(
        matches!(&rule.iteration, FailureIteration::Source { collection } if collection == &["Item"])
    );
    let filter = project.root.children[0].filter.unwrap();
    assert!(
        matches!(project.graph.nodes.get(&filter), Some(Node::Call { function, args }) if function == "not" && args.as_slice() == std::slice::from_ref(&predicate))
    );
    predicate
}

fn component<'a>(document: &'a roxmltree::Document<'a>, name: &str) -> roxmltree::Node<'a, 'a> {
    document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
        .unwrap()
}

fn pin(component: roxmltree::Node<'_, '_>, side: &str, position: usize) -> String {
    component
        .children()
        .find(|node| node.has_tag_name(side))
        .unwrap()
        .children()
        .filter(|node| node.has_tag_name("datapoint"))
        .enumerate()
        .find(|(index, node)| {
            node.attribute("pos")
                .and_then(|pos| pos.parse::<usize>().ok())
                .unwrap_or(*index)
                == position
        })
        .and_then(|(_, node)| node.attribute("key"))
        .unwrap()
        .to_string()
}

fn edge(document: &roxmltree::Document<'_>, from: &str, to: &str) -> bool {
    document.descendants().any(|vertex| {
        vertex.has_tag_name("vertex")
            && vertex.attribute("vertexkey") == Some(from)
            && vertex
                .descendants()
                .any(|node| node.has_tag_name("edge") && node.attribute("vertexkey") == Some(to))
    })
}

fn branches(design: &Path) {
    let xml = std::fs::read_to_string(design).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let filter = component(&document, "filter");
    let exception = component(&document, "exception");
    let true_output = pin(filter, "targets", 0);
    let false_output = pin(filter, "targets", 1);
    let throw = pin(exception, "sources", 0);
    assert!(edge(&document, &true_output, &throw));
    assert!(!edge(&document, &false_output, &throw));
    assert!(
        document
            .descendants()
            .any(|vertex| vertex.has_tag_name("vertex")
                && vertex.attribute("vertexkey") == Some(false_output.as_str())
                && vertex.descendants().any(|node| node.has_tag_name("edge")))
    );
    for ordinary_not in document.descendants().filter(|node| {
        node.has_tag_name("component") && node.attribute("name") == Some("logical-not")
    }) {
        let not_output = pin(ordinary_not, "targets", 0);
        assert!(
            !edge(&document, &not_output, &pin(filter, "sources", 1)),
            "filter must evaluate original P rather than not(P)"
        );
    }
}

fn cycle_projects(dir: &Directory, label: &str, original: &Project) -> Vec<(PathBuf, Project)> {
    let validation = engine::validate(original);
    retain(&dir.0, &format!("{label}-original-validation"), &validation);
    assert!(validation.is_empty(), "{validation:?}");
    std::fs::write(
        dir.0.join(format!("{label}-original-project.json")),
        serde_json::to_vec_pretty(original).unwrap(),
    )
    .unwrap();
    let mut results = Vec::new();
    for native in [false, true] {
        let mut current = original.clone();
        for cycle in 0..2 {
            let path = dir.child(&format!(
                "{label}-{}-{cycle}",
                if native { "native" } else { "default" }
            ));
            let design = path.join("design.mfd");
            std::fs::write(
                path.join("project-before.json"),
                serde_json::to_vec_pretty(&current).unwrap(),
            )
            .unwrap();
            let xml = format_xml::to_string(&current.source, &all_false());
            retain(&path, "schema-valid-input-xml-result", &xml);
            std::fs::write(path.join("input.xml"), xml.unwrap()).unwrap();
            if native {
                let result = mfd::export_with_profile(&current, &design, ExportProfile::NativeMfd);
                retain(&path, "native-export-result", &result);
                match result {
                    Ok(report) => assert!(report.is_native_compatible(), "{report:?}"),
                    Err(mfd::MfdError::IncompatibleExport(report)) => {
                        assert!(report.issues.iter().any(|issue| issue.feature
                            == mfd::ExportCompatibilityFeature::GlobalFailureOrdering));
                        assert!(!design.exists());
                        assert!(!path.join("design-source.xsd").exists());
                        assert!(!path.join("design-target.xsd").exists());
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            } else {
                let result = mfd::export(&current, &design);
                retain(&path, "default-export-result", &result);
                assert!(result.as_ref().is_ok_and(Vec::is_empty), "{result:?}");
            }
            branches(&design);
            let outcome = mfd::import_with_profile(
                &design,
                &ImportOptions::default().with_package_root(&dir.0),
                ImportProfile::Executable,
            );
            match &outcome {
                Ok(outcome) => {
                    retain(&path, "strict-import-report", &outcome.report);
                    retain(&path, "strict-import-warnings", &outcome.imported.warnings);
                    std::fs::write(
                        path.join("imported-project.json"),
                        serde_json::to_vec_pretty(&outcome.imported.project).unwrap(),
                    )
                    .unwrap();
                }
                Err(error) => retain(&path, "strict-import-error", error),
            }
            let outcome = outcome.unwrap();
            assert!(outcome.report.executable && outcome.report.issues.is_empty());
            assert!(outcome.imported.warnings.is_empty());
            current = outcome.imported.project;
            assert_identity(&current);
            results.push((path, current.clone()));
        }
    }
    results
}

#[test]
fn complementary_true_branch_roundtrips_both_profiles_with_full_literals_and_first_late_failure() {
    let dir = Directory::new("branches");
    let original = project(Some(3));
    let expected = expected_rows(&[10, 20]);
    let literal = b"<Output><Row><Result>10</Result></Row><Row><Result>20</Result></Row></Output>";
    std::fs::write(dir.0.join("expected-output.xml"), literal).unwrap();
    std::fs::write(
        dir.0.join("expected-output.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    let mut projects = vec![(dir.child("original"), original.clone())];
    projects.extend(cycle_projects(&dir, "branches", &original));
    for (path, current) in projects {
        assert_identity(&current);
        let actual = capture_run(&path, "all-false", &current, &all_false());
        assert_eq!(actual, Ok(expected.clone()));
        let xml = format_xml::to_string_with_options(
            &current.target,
            actual.as_ref().unwrap(),
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                ..Default::default()
            },
        );
        retain(&path, "compact-output-result", &xml);
        assert_eq!(xml.unwrap().as_bytes(), literal);
        for (label, rows, message) in [
            (
                "first",
                vec![
                    item(1, Value::Bool(true), "first"),
                    item(2, Value::Bool(true), "second"),
                ],
                "first",
            ),
            (
                "late",
                vec![
                    item(1, Value::Bool(false), "unselected"),
                    item(2, Value::Bool(true), "late"),
                    item(3, Value::Bool(true), "later"),
                ],
                "late",
            ),
        ] {
            let actual = capture_run(&path, label, &current, &input(rows));
            assert_eq!(
                actual,
                Err(EngineError::MappingFailure {
                    rule: 1,
                    message: Some(message.into())
                })
            );
        }
        let mut throwing_target = current.clone();
        let value = throwing_target.root.children[0].bindings[0].node;
        assert!(!throwing_target.graph.nodes.contains_key(&1000));
        assert!(!throwing_target.graph.nodes.contains_key(&1001));
        throwing_target.graph.nodes.insert(
            1000,
            Node::Const {
                value: Value::Int(0),
            },
        );
        throwing_target.graph.nodes.insert(
            1001,
            Node::Call {
                function: "divide".into(),
                args: vec![value, 1000],
            },
        );
        throwing_target.root.children[0].bindings[0].node = 1001;
        let actual = capture_run(
            &path,
            "failure-before-target",
            &throwing_target,
            &input(vec![item(10, Value::Bool(true), "before-target")]),
        );
        assert_eq!(
            actual,
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("before-target".into())
            })
        );
    }
}

#[test]
fn absent_message_and_unselected_throwing_message_preserve_lazy_selected_cause() {
    let dir = Directory::new("messages");
    for message in [None, Some(7)] {
        let mut original = project(message);
        if message.is_some() {
            original.graph.nodes.insert(
                6,
                Node::Const {
                    value: Value::Int(0),
                },
            );
            original.graph.nodes.insert(
                7,
                Node::Call {
                    function: "divide".into(),
                    args: vec![0, 6],
                },
            );
        }
        let expected_error = capture_run(
            &dir.0,
            &format!("original-selected-{message:?}"),
            &original,
            &input(vec![item(10, Value::Bool(true), "irrelevant")]),
        );
        if message.is_none() {
            assert_eq!(
                expected_error,
                Err(EngineError::MappingFailure {
                    rule: 1,
                    message: None
                })
            );
        } else {
            assert!(
                matches!(&expected_error, Err(EngineError::Function(source)) if source.to_string().to_lowercase().contains("zero"))
            );
        }
        let mut projects = vec![(
            dir.child(&format!("original-{message:?}")),
            original.clone(),
        )];
        projects.extend(cycle_projects(
            &dir,
            &format!("message-{message:?}"),
            &original,
        ));
        for (path, current) in projects {
            let actual = capture_run(&path, "unselected-throw", &current, &all_false());
            assert_eq!(actual, Ok(expected_rows(&[10, 20])));
            let actual = capture_run(
                &path,
                "selected-message",
                &current,
                &input(vec![item(10, Value::Bool(true), "irrelevant")]),
            );
            assert_eq!(actual, expected_error);
            if message.is_none() {
                assert_eq!(current.failure_rules[0].message, None);
                if path.join("design.mfd").exists() {
                    let xml = std::fs::read_to_string(path.join("design.mfd")).unwrap();
                    let document = roxmltree::Document::parse(&xml).unwrap();
                    let message_pin = pin(component(&document, "exception"), "sources", 1);
                    assert!(!document.descendants().any(|node| node.has_tag_name("edge")
                        && node.attribute("vertexkey") == Some(message_pin.as_str())));
                }
            }
        }
    }
}

#[test]
fn position_roots_and_an_unrelated_not_consumer_keep_original_predicate_and_wires() {
    let dir = Directory::new("positions");
    let mut original = project(Some(5));
    for id in [5, 7] {
        original.graph.nodes.insert(
            id,
            Node::Position {
                collection: vec!["Item".into()],
            },
        );
    }
    original.root.children[0].bindings.extend([
        Binding {
            target_field: "Allowed".into(),
            node: 4,
        },
        Binding {
            target_field: "Rank".into(),
            node: 7,
        },
    ]);
    let mut projects = vec![(dir.child("original"), original.clone())];
    projects.extend(cycle_projects(&dir, "positions", &original));
    for (path, current) in projects {
        let actual = capture_run(&path, "all-false-position", &current, &all_false());
        let expected = group(vec![(
            "Row",
            Instance::Repeated(vec![
                group(vec![
                    ("Result", Instance::Scalar(Value::Int(10))),
                    ("Allowed", Instance::Scalar(Value::Bool(true))),
                    ("Rank", Instance::Scalar(Value::Int(1))),
                ]),
                group(vec![
                    ("Result", Instance::Scalar(Value::Int(20))),
                    ("Allowed", Instance::Scalar(Value::Bool(true))),
                    ("Rank", Instance::Scalar(Value::Int(2))),
                ]),
            ]),
        )]);
        assert_eq!(actual, Ok(expected));
        let actual = capture_run(
            &path,
            "second-position-message",
            &current,
            &input(vec![
                item(10, Value::Bool(false), ""),
                item(20, Value::Bool(true), ""),
            ]),
        );
        assert_eq!(
            actual,
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("2".into())
            })
        );
        let predicate = assert_identity(&current);
        let allowed = current.root.children[0]
            .bindings
            .iter()
            .find(|binding| binding.target_field == "Allowed")
            .unwrap()
            .node;
        assert!(
            matches!(current.graph.nodes.get(&allowed), Some(Node::Call { function, args }) if function == "not" && args.as_slice() == std::slice::from_ref(&predicate))
        );
    }
    let mut position_predicate = project(Some(3));
    position_predicate.graph.nodes.insert(
        5,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    position_predicate.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::Int(1),
        },
    );
    position_predicate.graph.nodes.insert(
        8,
        Node::Call {
            function: "greater_than".into(),
            args: vec![5, 6],
        },
    );
    position_predicate.graph.nodes.insert(
        4,
        Node::Call {
            function: "not".into(),
            args: vec![8],
        },
    );
    position_predicate.failure_rules[0].selection = FailureSelection::WhenTrue { predicate: 8 };
    let mut projects = vec![(
        dir.child("original-position-predicate"),
        position_predicate.clone(),
    )];
    projects.extend(cycle_projects(
        &dir,
        "position-predicate",
        &position_predicate,
    ));
    for (path, current) in projects {
        let actual = capture_run(
            &path,
            "one-position",
            &current,
            &input(vec![item(10, Value::Bool(false), "unused")]),
        );
        assert_eq!(actual, Ok(expected_rows(&[10])));
        let actual = capture_run(
            &path,
            "second-position-predicate",
            &current,
            &input(vec![
                item(10, Value::Bool(false), "unused"),
                item(20, Value::Bool(false), "second-picked"),
            ]),
        );
        assert_eq!(
            actual,
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("second-picked".into())
            })
        );
    }
}

#[test]
fn predicate_error_identity_and_schema_valid_optional_empty_xml_are_preserved() {
    let dir = Directory::new("predicate-empty");
    let original = project(Some(3));
    std::fs::write(
        dir.0.join("independent-optional-empty-input.xml"),
        "<Input/>",
    )
    .unwrap();
    let mut projects = vec![(dir.child("original"), original.clone())];
    projects.extend(cycle_projects(&dir, "empty", &original));
    for (path, current) in projects {
        let predicate = assert_identity(&current);
        for (label, value, found) in [
            ("null", Value::Null, "null"),
            ("not-bool", Value::Int(5), "int"),
        ] {
            let actual = capture_run(
                &path,
                label,
                &current,
                &input(vec![item(10, value, "unselected")]),
            );
            assert_eq!(
                actual,
                Err(EngineError::NotABool {
                    node: predicate,
                    found
                })
            );
        }
        let typed_empty = capture_run(&path, "typed-empty", &current, &input(Vec::new()));
        assert_eq!(typed_empty, Ok(expected_rows(&[])));
        let parsed = format_xml::from_str("<Input/>", &current.source);
        retain(&path, "optional-empty-xml-parse", &parsed);
        let parsed = parsed.unwrap();
        let actual = capture_run(&path, "parsed-optional-empty", &current, &parsed);
        assert_eq!(actual, Ok(expected_rows(&[])));
        if path.join("design.mfd").exists() {
            let schema = std::fs::read_to_string(path.join("design-source.xsd")).unwrap();
            let document = roxmltree::Document::parse(&schema).unwrap();
            assert!(
                document
                    .descendants()
                    .any(|node| node.has_tag_name("element")
                        && node.attribute("name") == Some("Item")
                        && node.attribute("minOccurs") == Some("0"))
            );
        }
    }
}

fn refusal(path: &Path, label: &str, project: &Project, expected: &str) {
    std::fs::write(
        path.join("project.json"),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    for native in [false, true] {
        let subdir = path.join(if native { "native" } else { "default" });
        std::fs::create_dir(&subdir).unwrap();
        let design = subdir.join("design.mfd");
        let source = subdir.join("design-source.xsd");
        let target = subdir.join("design-target.xsd");
        std::fs::write(&design, b"sentinel design").unwrap();
        std::fs::write(&source, b"sentinel source").unwrap();
        std::fs::write(&target, b"sentinel target").unwrap();
        let profile = if native {
            ExportProfile::NativeMfd
        } else {
            ExportProfile::FerruleExtensions
        };
        let result = mfd::export_with_profile(project, &design, profile);
        retain(&subdir, label, &result);
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.to_string().contains(expected)),
            "{result:?}"
        );
        assert_eq!(std::fs::read(&design).unwrap(), b"sentinel design");
        assert_eq!(std::fs::read(&source).unwrap(), b"sentinel source");
        assert_eq!(std::fs::read(&target).unwrap(), b"sentinel target");
        let fresh = subdir.join("fresh.mfd");
        let result = mfd::export_with_profile(project, &fresh, profile);
        retain(&subdir, "fresh-publication-refusal", &result);
        assert!(result.is_err());
        assert!(!fresh.exists());
        assert!(!subdir.join("fresh-source.xsd").exists());
        assert!(!subdir.join("fresh-target.xsd").exists());
    }
}

#[test]
fn noncanonical_true_consumers_and_owners_refuse_before_any_artifact_publication() {
    let dir = Directory::new("refusals");
    for label in [
        "same-polarity",
        "wrong-argument",
        "wrong-arity",
        "ambiguous",
        "collection",
        "root-name-prefix",
        "empty-root",
        "nonrepeating",
        "dynamic",
        "sort",
        "group",
        "window",
        "post-filter",
        "first",
        "mapped",
        "copy",
        "multiple-rules",
        "named",
        "generated",
        "missing-filter",
        "missing-predicate",
        "missing-message",
    ] {
        let mut current = project(Some(3));
        let mut expected = "complementary false-branch";
        match label {
            "same-polarity" => current.root.children[0].filter = Some(2),
            "wrong-argument" => {
                current.graph.nodes.insert(
                    4,
                    Node::Call {
                        function: "not".into(),
                        args: vec![3],
                    },
                );
            }
            "wrong-arity" => {
                current.graph.nodes.insert(
                    4,
                    Node::Call {
                        function: "not".into(),
                        args: vec![2, 2],
                    },
                );
            }
            "ambiguous" => current.root.children.push(current.root.children[0].clone()),
            "collection" => {
                current.failure_rules[0].iteration = FailureIteration::Source {
                    collection: vec!["other".into()],
                };
                expected = "exact repeating primary-source schema path";
            }
            "root-name-prefix" => {
                current.failure_rules[0].iteration = FailureIteration::Source {
                    collection: vec!["Input".into(), "Item".into()],
                };
                current.root.children[0].set_source(Some(vec!["Input".into(), "Item".into()]));
                expected = "exact repeating primary-source schema path";
            }
            "empty-root" => {
                current.failure_rules[0].iteration = FailureIteration::Source {
                    collection: Vec::new(),
                };
                current.root.children[0].set_source(Some(Vec::new()));
                expected = "exact repeating primary-source schema path";
            }
            "nonrepeating" => {
                let mut item = current.source.child("Item").unwrap().clone();
                item.repeating = false;
                current.source = SchemaNode::group("Input", vec![item]);
                expected = "exact repeating primary-source schema path";
            }
            "dynamic" => {
                assert!(current.root.children[0].set_output_path(Some(3)));
            }
            "sort" => current.root.children[0].sort_by = Some(0),
            "group" => current.root.children[0].group_by = Some(0),
            "window" => current.root.children[0]
                .windows
                .push(SequenceWindow::First { count: 0 }),
            "post-filter" => current.root.children[0].post_group_filter = Some(2),
            "first" => current.root.children[0].iteration_output = IterationOutput::First,
            "mapped" => current.root.children[0].iteration_output = IterationOutput::MappedSequence,
            "copy" => current.root.children[0].construction = ScopeConstruction::CopyCurrentSource,
            "multiple-rules" => {
                current.failure_rules.push(current.failure_rules[0].clone());
                expected = "exactly one failure rule";
            }
            "named" => {
                current.extra_sources.push(NamedSource {
                    name: "other".into(),
                    path: "other.xml".into(),
                    schema: current.source.clone(),
                    options: Default::default(),
                    dynamic_path: None,
                });
                current.failure_rules[0].iteration = FailureIteration::Source {
                    collection: vec!["other".into(), "Item".into()],
                };
                expected = "secondary-source failures";
            }
            "generated" => {
                current.failure_rules[0].iteration = FailureIteration::Sequence {
                    sequence: SequenceExpr::Tokenize {
                        input: 3,
                        delimiter: 3,
                        item: 9,
                    },
                };
                expected = "generated-sequence failures";
            }
            "missing-filter" => {
                current.graph.nodes.remove(&4);
            }
            "missing-predicate" => {
                current.failure_rules[0].selection = FailureSelection::WhenTrue { predicate: 999 };
                expected = "missing predicate node 999";
            }
            "missing-message" => {
                current.failure_rules[0].message = Some(999);
                expected = "missing message node 999";
            }
            _ => unreachable!(),
        }
        refusal(&dir.child(label), label, &current, expected);
    }
}

#[test]
fn transformed_ancestors_and_ambiguous_failure_positions_remain_atomic_refusals() {
    let dir = Directory::new("ancestors");
    let mut shared_position = project(Some(5));
    shared_position.graph.nodes.insert(
        5,
        Node::Position {
            collection: vec!["Item".into()],
        },
    );
    shared_position.root.children[0].bindings.push(Binding {
        target_field: "Rank".into(),
        node: 5,
    });
    refusal(
        &dir.child("shared-raw-message-and-target-position"),
        "shared-raw-message-and-target-position",
        &shared_position,
        "no unambiguous failure-item",
    );
    let mut nested = project(Some(5));
    let source_item = nested.source.child("Item").unwrap().clone();
    let target_row = nested.target.child("Row").unwrap().clone();
    nested.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::group("Outer", vec![source_item]).repeating()],
    );
    nested.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("OuterRow", vec![target_row]).repeating()],
    );
    let inner = nested.root.children.remove(0);
    nested.root.children.push(Scope {
        target_field: "OuterRow".into(),
        iteration: ScopeIteration::Source(vec!["Outer".into()]),
        children: vec![inner],
        ..Scope::default()
    });
    nested.failure_rules[0].iteration = FailureIteration::Source {
        collection: vec!["Outer".into(), "Item".into()],
    };
    for id in [0, 2, 3] {
        if let Some(Node::SourceField { frame, .. }) = nested.graph.nodes.get_mut(&id) {
            *frame = Some(vec!["Outer".into(), "Item".into()]);
        }
    }
    nested.graph.nodes.insert(
        5,
        Node::Position {
            collection: vec!["Outer".into(), "Item".into()],
        },
    );
    let mut restricted = nested.clone();
    restricted.root.children[0].filter = Some(4);
    refusal(
        &dir.child("ancestor-filter"),
        "ancestor-filter",
        &restricted,
        "found 0",
    );
    nested.graph.nodes.insert(
        5,
        Node::Position {
            collection: vec!["Outer".into()],
        },
    );
    refusal(
        &dir.child("ambiguous-position"),
        "ambiguous-position",
        &nested,
        "no unambiguous failure-item",
    );
}

#[path = "exception_true_branch_export/negation.rs"]
mod negation;
