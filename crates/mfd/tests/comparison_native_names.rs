use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};
use mfd::{ExportProfile, ImportOptions, ImportProfile};

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_comparison_names_{label}_{}_{}",
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
            eprintln!("retained comparison name originals: {}", self.0.display());
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
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn input() -> Instance {
    group(vec![(
        "Item",
        Instance::Repeated(
            [
                (9_007_199_254_740_992, 9_007_199_254_740_993),
                (i64::MAX, i64::MAX),
                (i64::MIN, i64::MIN + 1),
                (i64::MAX, i64::MAX - 1),
            ]
            .into_iter()
            .map(|(a, b)| {
                group(vec![
                    ("A", Instance::Scalar(Value::Int(a))),
                    ("B", Instance::Scalar(Value::Int(b))),
                ])
            })
            .collect(),
        ),
    )])
}

fn output(expected: &[bool]) -> Instance {
    group(vec![(
        "Row",
        Instance::Repeated(
            expected
                .iter()
                .map(|value| group(vec![("Result", Instance::Scalar(Value::Bool(*value)))]))
                .collect(),
        ),
    )])
}

fn project(function: &str) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("A", ScalarType::Int),
                        SchemaNode::scalar("B", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Result", ScalarType::Bool)])
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
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["A".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["B".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: function.into(),
                        args: vec![0, 1],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn capture(path: &Path, label: &str, project: &Project) -> Result<Instance, engine::EngineError> {
    std::fs::write(
        path.join(format!("{label}-project.json")),
        serde_json::to_vec_pretty(project).unwrap(),
    )
    .unwrap();
    std::fs::write(
        path.join(format!("{label}-input.json")),
        serde_json::to_vec_pretty(&input()).unwrap(),
    )
    .unwrap();
    let actual = engine::run(project, &input());
    retain(path, label, &actual);
    actual
}

fn export(path: &Path, project: &Project, expected_name: &str) -> String {
    let input_xml = format_xml::to_string(&project.source, &input());
    retain(path, "input-xml-original", &input_xml);
    std::fs::write(path.join("input.xml"), input_xml.unwrap()).unwrap();
    let result =
        mfd::export_with_profile(project, &path.join("design.mfd"), ExportProfile::NativeMfd);
    retain(path, "native-export-original", &result);
    let report = result.unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(report.warnings.is_empty(), "{report:?}");
    let xml = std::fs::read_to_string(path.join("design.mfd")).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let comparisons: Vec<_> = doc
        .descendants()
        .filter(|node| {
            node.has_tag_name("component")
                && node.attribute("kind") == Some("5")
                && node.attribute("library") == Some("core")
        })
        .collect();
    assert_eq!(comparisons.len(), 1, "{xml}");
    assert_eq!(comparisons[0].attribute("name"), Some(expected_name));
    assert_eq!(
        comparisons[0]
            .descendants()
            .filter(|node| node.has_tag_name("datapoint")
                && node
                    .parent()
                    .is_some_and(|parent| parent.has_tag_name("sources")))
            .count(),
        2
    );
    xml
}

fn import(path: &Path, name: &str) -> Project {
    let original = mfd::import_with_profile(
        &path.join(name),
        &ImportOptions::default().with_package_root(path),
        ImportProfile::Executable,
    );
    match &original {
        Ok(outcome) => {
            retain(path, "import-report-original", &outcome.report);
            retain(path, "import-warnings-original", &outcome.imported.warnings);
            std::fs::write(
                path.join("imported-project-original.json"),
                serde_json::to_vec_pretty(&outcome.imported.project).unwrap(),
            )
            .unwrap();
        }
        Err(error) => retain(path, "import-error-original", error),
    }
    let outcome = original.unwrap();
    assert!(outcome.report.executable && outcome.report.issues.is_empty());
    assert!(outcome.imported.warnings.is_empty());
    let validation = engine::validate(&outcome.imported.project);
    retain(path, "import-validation-original", &validation);
    assert!(validation.is_empty());
    outcome.imported.project
}

#[test]
fn six_comparison_names_keep_complete_integer_results_and_two_strict_cycles() {
    let dir = Directory::new("six");
    // Whole four-row boolean oracles are literal, independent of comparison dispatch.
    for (local, native, values) in [
        ("equal", "equal", [false, true, false, false]),
        ("not_equal", "not-equal", [true, false, true, true]),
        ("less_than", "less", [true, false, true, false]),
        ("greater_than", "greater", [false, false, false, true]),
        ("less_or_equal", "equal-or-less", [true, true, true, false]),
        (
            "greater_or_equal",
            "equal-or-greater",
            [false, true, false, true],
        ),
    ] {
        let mut current = project(local);
        let expected = output(&values);
        assert_eq!(
            capture(&dir.0, &format!("{local}-original"), &current),
            Ok(expected.clone())
        );
        for cycle in 0..2 {
            let path = dir.child(&format!("{local}-{cycle}"));
            export(&path, &current, native);
            current = import(&path, "design.mfd");
            assert_eq!(capture(&path, "reimported", &current), Ok(expected.clone()));
        }
    }
}

#[test]
fn historical_inclusive_aliases_import_then_reexport_with_current_component_names() {
    let dir = Directory::new("historical");
    for (local, canonical, alias, expected) in [
        (
            "less_or_equal",
            "equal-or-less",
            "less-equal",
            [true, true, true, false],
        ),
        (
            "less_or_equal",
            "equal-or-less",
            "less-or-equal",
            [true, true, true, false],
        ),
        (
            "greater_or_equal",
            "equal-or-greater",
            "greater-equal",
            [false, true, false, true],
        ),
        (
            "greater_or_equal",
            "equal-or-greater",
            "greater-or-equal",
            [false, true, false, true],
        ),
    ] {
        let path = dir.child(alias);
        let xml = export(&path, &project(local), canonical);
        let marker = format!("name=\"{canonical}\"");
        let replacement = format!("name=\"{alias}\"");
        assert_eq!(xml.matches(&marker).count(), 1);
        let historical = xml.replacen(&marker, &replacement, 1);
        std::fs::write(path.join("historical.mfd"), &historical).unwrap();
        let imported = import(&path, "historical.mfd");
        let calls: Vec<_> = imported
            .graph
            .nodes
            .values()
            .filter_map(|node| {
                if let Node::Call { function, .. } = node {
                    Some(function.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(calls, vec![local]);
        assert_eq!(
            capture(&path, "historical-result", &imported),
            Ok(output(&expected))
        );
        export(
            &dir.child(&format!("{alias}-canonical")),
            &imported,
            canonical,
        );
    }
}
