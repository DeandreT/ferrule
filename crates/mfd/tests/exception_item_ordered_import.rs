use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::EngineError;
use ir::{Instance, Value};
use mapping::{Node, NodeId, Project};
use mfd::{ImportIssueKind, ImportOptions, ImportProfile};

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_item_exception_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn case(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir(&path).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!(
                "retained item-ordered import originals: {}",
                self.0.display()
            );
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn retain(path: &Path, name: &str, value: &impl Debug) {
    std::fs::write(
        path.join(format!("{name}.debug.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn design(throw_true: bool) -> String {
    let original = std::fs::read_to_string(fixture("exception.mfd")).unwrap();
    if throw_true {
        original
            .replace("from=\"22\" to=\"30\"", "from=\"23\" to=\"30\"")
            .replace("from=\"23\" to=\"40\"", "from=\"22\" to=\"40\"")
    } else {
        original
    }
}

fn stage(path: &Path, design: &str, source_extra: &str, target_extra: &str) -> PathBuf {
    let source = std::fs::read_to_string(fixture("exception-source.xsd"))
        .unwrap()
        .replace(
            "              <xs:element name=\"Description\" type=\"xs:string\"/>",
            &format!(
                "              <xs:element name=\"Description\" type=\"xs:string\"/>{source_extra}"
            ),
        );
    let target = std::fs::read_to_string(fixture("exception-target.xsd"))
        .unwrap()
        .replace(
            "              <xs:element name=\"Description\" type=\"xs:string\"/>",
            &format!(
                "              <xs:element name=\"Description\" type=\"xs:string\"/>{target_extra}"
            ),
        );
    std::fs::write(path.join("exception-source.xsd"), source).unwrap();
    std::fs::write(path.join("exception-target.xsd"), target).unwrap();
    let value = if source_extra.contains("name=\"Value\"") {
        "<Value>10</Value>"
    } else {
        ""
    };
    std::fs::write(path.join("expenses.xml"), format!("<Expenses><Expense><Allowed>true</Allowed><Description>fixture</Description>{value}</Expense></Expenses>")).unwrap();
    let mapping = path.join("design.mfd");
    std::fs::write(&mapping, design).unwrap();
    mapping
}

fn files(path: &Path) -> BTreeMap<String, Vec<u8>> {
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

fn imported(path: &Path, mapping: &Path, item_order: bool) -> Project {
    let options = ImportOptions::default().with_package_root(path);
    let options = if item_order {
        options.with_item_ordered_exceptions()
    } else {
        options
    };
    let result = mfd::import_with_profile(mapping, &options, ImportProfile::Executable);
    match &result {
        Ok(outcome) => {
            retain(
                path,
                if item_order {
                    "item-project-full"
                } else {
                    "global-project-full"
                },
                &outcome.imported.project,
            );
            retain(
                path,
                if item_order {
                    "item-report"
                } else {
                    "global-report"
                },
                &outcome.report,
            );
            retain(
                path,
                if item_order {
                    "item-warnings"
                } else {
                    "global-warnings"
                },
                &outcome.imported.warnings,
            );
            std::fs::write(
                path.join(if item_order {
                    "item-project.json"
                } else {
                    "global-project.json"
                }),
                serde_json::to_vec_pretty(&outcome.imported.project).unwrap(),
            )
            .unwrap();
        }
        Err(error) => retain(path, "import-error", error),
    }
    let outcome = result.unwrap();
    assert!(outcome.report.executable && outcome.report.issues.is_empty());
    assert!(outcome.imported.warnings.is_empty());
    let validation = engine::validate(&outcome.imported.project);
    retain(
        path,
        if item_order {
            "item-validation"
        } else {
            "global-validation"
        },
        &validation,
    );
    assert!(validation.is_empty(), "{validation:?}");
    outcome.imported.project
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn expense(allowed: Value, description: Value, value: i64) -> Instance {
    group(vec![
        ("Allowed", Instance::Scalar(allowed)),
        ("Description", Instance::Scalar(description)),
        ("Value", Instance::Scalar(Value::Int(value))),
    ])
}

fn input(items: Vec<Instance>) -> Instance {
    group(vec![("Expense", Instance::Repeated(items))])
}

fn evaluate(
    path: &Path,
    label: &str,
    project: &Project,
    input: &Instance,
) -> Result<Instance, EngineError> {
    std::fs::write(
        path.join(format!("{label}-input.json")),
        serde_json::to_vec_pretty(input).unwrap(),
    )
    .unwrap();
    let result = engine::run(project, input);
    retain(path, label, &result);
    result
}

fn assert_divide_zero(actual: Result<Instance, EngineError>) {
    match actual {
        Err(EngineError::Function(source)) => {
            assert_eq!(format!("{source:?}"), "DivideByZero");
            assert_eq!(source.to_string(), "division by zero");
        }
        actual => panic!("original typed function cause required: {actual:?}"),
    }
}

fn guard(project: &Project, throw_true: bool) -> (NodeId, NodeId, Option<NodeId>) {
    let owner = &project.root.children[0];
    let Some(Node::If {
        condition,
        then,
        else_,
    }) = owner.filter.and_then(|id| project.graph.nodes.get(&id))
    else {
        panic!("owner must have its lazy guard");
    };
    let (raise, yes) = if throw_true {
        (*then, *else_)
    } else {
        (*else_, *then)
    };
    assert!(matches!(
        project.graph.nodes.get(&yes),
        Some(Node::Const {
            value: Value::Bool(true)
        })
    ));
    let Some(Node::Raise { message }) = project.graph.nodes.get(&raise) else {
        panic!("selected branch must own Raise");
    };
    assert_eq!(
        project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Raise { .. }))
            .count(),
        1
    );
    assert!(project.failure_rules.is_empty());
    (*condition, raise, *message)
}

fn source_predicate(project: &Project) -> NodeId {
    *project.graph.nodes.iter().find(|(_, node)| matches!(node, Node::SourceField { path, frame } if path.as_slice() == ["Allowed"] && frame.as_deref() == Some(&["Expense".to_string()][..]))).unwrap().0
}

fn division_design(source: String, message: bool) -> String {
    let source = source.replace("<entry name=\"Description\" outkey=\"12\"/>", "<entry name=\"Description\" outkey=\"12\"/><entry name=\"Value\" outkey=\"13\"/>")
        .replace("<entry name=\"Description\" inpkey=\"31\"/>", "<entry name=\"Description\" inpkey=\"31\"/><entry name=\"Result\" inpkey=\"32\"/>")
        .replace("        <component name=\"reject-expense\"", r#"        <component name="constant" library="core" kind="2" uid="6"><targets><datapoint pos="0" key="50"/></targets><data><constant value="0" datatype="integer"/></data></component>
        <component name="divide" library="core" kind="5" uid="7"><sources><datapoint pos="0" key="51"/><datapoint pos="1" key="52"/></sources><targets><datapoint pos="0" key="53"/></targets></component>
        <component name="reject-expense""#)
        .replace("      <edge from=\"12\" to=\"31\"/>", "      <edge from=\"12\" to=\"31\"/>\n      <edge from=\"13\" to=\"51\"/>\n      <edge from=\"50\" to=\"52\"/>");
    if message {
        source.replace("from=\"12\" to=\"41\"", "from=\"53\" to=\"41\"")
    } else {
        source.replace(
            "      <edge from=\"12\" to=\"41\"/>",
            "      <edge from=\"53\" to=\"32\"/>\n      <edge from=\"12\" to=\"41\"/>",
        )
    }
}

#[test]
fn explicit_item_order_keeps_default_global_rules_and_exact_predicate_identity_for_both_branches() {
    let dir = Directory::new("default-and-polarity");
    for throw_true in [false, true] {
        let path = dir.case(&format!("branch-{throw_true}"));
        let mapping = stage(&path, &design(throw_true), "", "");
        let original_design = std::fs::read(&mapping).unwrap();
        let default = imported(&path, &mapping, false);
        let opted = imported(&path, &mapping, true);
        let (predicate, raise, message) = guard(&opted, throw_true);
        assert_eq!(predicate, source_predicate(&default));
        assert_eq!(
            serde_json::to_value(opted.graph.nodes.get(&predicate)).unwrap(),
            serde_json::to_value(default.graph.nodes.get(&predicate)).unwrap()
        );
        assert!(message.is_some());
        assert_eq!(default.failure_rules.len(), 1);
        assert!(
            !default
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Raise { .. }))
        );
        let source = input(vec![
            expense(Value::Bool(!throw_true), Value::String("one".into()), 10),
            expense(Value::Bool(!throw_true), Value::String("two".into()), 20),
        ]);
        let expected = group(vec![(
            "Expense",
            Instance::Repeated(vec![
                group(vec![(
                    "Description",
                    Instance::Scalar(Value::String("one".into())),
                )]),
                group(vec![(
                    "Description",
                    Instance::Scalar(Value::String("two".into())),
                )]),
            ]),
        )]);
        assert_eq!(
            evaluate(&path, "unselected", &opted, &source),
            Ok(expected.clone())
        );
        assert_eq!(
            evaluate(&path, "default-unselected", &default, &source),
            Ok(expected.clone())
        );
        let xml = format_xml::to_string(&opted.target, &expected);
        retain(&path, "full-output-xml", &xml);
        let xml = xml.unwrap();
        assert_eq!(
            xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Accepted>\n  <Expense>\n    <Description>one</Description>\n  </Expense>\n  <Expense>\n    <Description>two</Description>\n  </Expense>\n</Accepted>"
        );
        std::fs::write(path.join("expected-output.xml"), &xml).unwrap();
        let decoded = format_xml::from_str(&xml, &opted.target);
        retain(&path, "decoded-output", &decoded);
        assert_eq!(decoded.unwrap(), expected);
        let selected = input(vec![
            expense(Value::Bool(!throw_true), Value::String("first".into()), 10),
            expense(Value::Bool(throw_true), Value::String("late 😀".into()), 20),
        ]);
        assert_eq!(
            evaluate(&path, "selected", &opted, &selected),
            Err(EngineError::MappingException {
                node: raise,
                message: Some("late 😀".into())
            })
        );
        assert_eq!(
            evaluate(&path, "global-selected", &default, &selected),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("late 😀".into())
            })
        );
        let malformed_typed = input(vec![expense(
            Value::Int(1),
            Value::String("not read".into()),
            10,
        )]);
        assert_eq!(
            evaluate(&path, "typed-predicate-error", &opted, &malformed_typed),
            Err(EngineError::NotABool {
                node: predicate,
                found: "int"
            })
        );
        assert_eq!(std::fs::read(&mapping).unwrap(), original_design);
    }
}

#[test]
fn earlier_target_error_precedes_later_item_exception_while_default_rule_remains_pretarget() {
    let dir = Directory::new("ordering");
    for throw_true in [false, true] {
        let path = dir.case(&format!("branch-{throw_true}"));
        let mapping = stage(
            &path,
            &division_design(design(throw_true), false),
            "<xs:element name=\"Value\" type=\"xs:integer\"/>",
            "<xs:element name=\"Result\" type=\"xs:integer\"/>",
        );
        let default = imported(&path, &mapping, false);
        let opted = imported(&path, &mapping, true);
        let (_, raise, _) = guard(&opted, throw_true);
        let early = expense(
            Value::Bool(!throw_true),
            Value::String("earlier-unselected".into()),
            10,
        );
        let late = expense(
            Value::Bool(throw_true),
            Value::String("late-selected".into()),
            20,
        );
        let ordered = input(vec![early.clone(), late.clone()]);
        assert_divide_zero(evaluate(&path, "early-target", &opted, &ordered));
        assert_eq!(
            evaluate(&path, "default-global", &default, &ordered),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("late-selected".into())
            })
        );
        assert_eq!(
            evaluate(
                &path,
                "reverse-selected-first",
                &opted,
                &input(vec![late, early])
            ),
            Err(EngineError::MappingException {
                node: raise,
                message: Some("late-selected".into())
            })
        );
        assert!(!path.join("accepted.xml").exists());
    }
}

#[test]
fn selected_message_is_lazy_and_absent_null_and_function_causes_remain_distinct() {
    let dir = Directory::new("messages");
    for throw_true in [false, true] {
        for mode in ["absent", "null", "divide"] {
            let path = dir.case(&format!("{throw_true}-{mode}"));
            let mut source = design(throw_true);
            if mode == "absent" {
                source = source.replace("      <edge from=\"12\" to=\"41\"/>\n", "");
            }
            if mode == "divide" {
                source = division_design(source, true);
            }
            let mapping = stage(
                &path,
                &source,
                if mode == "divide" {
                    "<xs:element name=\"Value\" type=\"xs:integer\"/>"
                } else {
                    ""
                },
                if mode == "divide" {
                    "<xs:element name=\"Result\" type=\"xs:integer\" minOccurs=\"0\"/>"
                } else {
                    ""
                },
            );
            let opted = imported(&path, &mapping, true);
            let (_, raise, message) = guard(&opted, throw_true);
            assert_eq!(message.is_none(), mode == "absent");
            let ordinary = input(vec![expense(
                Value::Bool(!throw_true),
                Value::String("not selected".into()),
                10,
            )]);
            let unselected = evaluate(&path, "unselected-message", &opted, &ordinary);
            assert!(unselected.is_ok());
            let selected = input(vec![expense(
                Value::Bool(throw_true),
                if mode == "null" {
                    Value::Null
                } else {
                    Value::String("ignored".into())
                },
                10,
            )]);
            let actual = evaluate(&path, "selected-message", &opted, &selected);
            match mode {
                "absent" => assert_eq!(
                    actual,
                    Err(EngineError::MappingException {
                        node: raise,
                        message: None
                    })
                ),
                "null" => assert_eq!(
                    actual,
                    Err(EngineError::MappingException {
                        node: raise,
                        message: Some(String::new())
                    })
                ),
                _ => assert_divide_zero(actual),
            }
        }
    }
}

fn position_design(source: String) -> String {
    source.replace("<entry name=\"Description\" inpkey=\"31\"/>", "<entry name=\"Description\" inpkey=\"31\"/><entry name=\"Rank\" inpkey=\"32\"/><entry name=\"AllowedEcho\" inpkey=\"33\"/>")
        .replace("        <component name=\"reject-expense\"", r#"        <component name="auto-number" library="core" kind="5" uid="6"><sources><datapoint/></sources><targets><datapoint pos="0" key="51"/></targets></component>
        <component name="auto-number" library="core" kind="5" uid="7"><sources><datapoint/></sources><targets><datapoint pos="0" key="53"/></targets></component>
        <component name="logical-not" library="core" kind="5" uid="8"><sources><datapoint pos="0" key="54"/></sources><targets><datapoint pos="0" key="55"/></targets></component>
        <component name="reject-expense""#)
        .replace("      <edge from=\"12\" to=\"41\"/>", "      <edge from=\"51\" to=\"41\"/>\n      <edge from=\"53\" to=\"32\"/>\n      <edge from=\"11\" to=\"54\"/>\n      <edge from=\"55\" to=\"33\"/>")
}

#[test]
fn raw_message_position_and_existing_target_nodes_survive_project_save_and_reopen() {
    let dir = Directory::new("position-and-persistence");
    let path = dir.case("false-branch");
    // Distinct native auto-number controls retain raw-message and compact
    // target ownership even though both lower to the same collection identity.
    let source = position_design(design(false));
    let mapping = stage(
        &path,
        &source,
        "",
        "<xs:element name=\"Rank\" type=\"xs:integer\"/><xs:element name=\"AllowedEcho\" type=\"xs:boolean\"/>",
    );
    let default = imported(&path, &mapping, false);
    let opted = imported(&path, &mapping, true);
    let (_, raise, message) = guard(&opted, false);
    let message = message.unwrap();
    let rank = opted.root.children[0]
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Rank")
        .unwrap()
        .node;
    assert_ne!(message, rank);
    assert!(
        matches!(opted.graph.nodes.get(&message), Some(Node::Call { function, .. }) if function == "add")
    );
    assert!(
        matches!(opted.graph.nodes.get(&rank), Some(Node::Call { function, .. }) if function == "add")
    );
    assert_eq!(
        opted
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Position { collection } if collection.is_empty()))
            .count(),
        2
    );
    for binding in &default.root.children[0].bindings {
        let same = opted.root.children[0]
            .bindings
            .iter()
            .find(|candidate| candidate.target_field == binding.target_field)
            .unwrap();
        assert_eq!(same.node, binding.node);
        assert_eq!(
            serde_json::to_value(opted.graph.nodes.get(&same.node)).unwrap(),
            serde_json::to_value(default.graph.nodes.get(&binding.node)).unwrap()
        );
    }
    let source = input(vec![
        expense(Value::Bool(true), Value::String("a".into()), 10),
        expense(Value::Bool(true), Value::String("b".into()), 20),
    ]);
    let expected = group(vec![(
        "Expense",
        Instance::Repeated(vec![
            group(vec![
                ("Description", Instance::Scalar(Value::String("a".into()))),
                ("Rank", Instance::Scalar(Value::Int(1))),
                ("AllowedEcho", Instance::Scalar(Value::Bool(false))),
            ]),
            group(vec![
                ("Description", Instance::Scalar(Value::String("b".into()))),
                ("Rank", Instance::Scalar(Value::Int(2))),
                ("AllowedEcho", Instance::Scalar(Value::Bool(false))),
            ]),
        ]),
    )]);
    let original_files = [
        "design.mfd",
        "exception-source.xsd",
        "exception-target.xsd",
        "expenses.xml",
    ]
    .map(|name| (name, std::fs::read(path.join(name)).unwrap()));
    let mut current = opted;
    for cycle in 0..2 {
        let encoded = serde_json::to_vec_pretty(&current).unwrap();
        std::fs::write(path.join(format!("saved-{cycle}.json")), &encoded).unwrap();
        let reopened: Project = serde_json::from_slice(
            &std::fs::read(path.join(format!("saved-{cycle}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(serde_json::to_vec_pretty(&reopened).unwrap(), encoded);
        assert!(engine::validate(&reopened).is_empty());
        assert_eq!(
            evaluate(&path, &format!("reopen-{cycle}"), &reopened, &source),
            Ok(expected.clone())
        );
        let late = input(vec![
            expense(Value::Bool(true), Value::String("unselected".into()), 10),
            expense(Value::Bool(false), Value::String("selected".into()), 20),
        ]);
        assert_eq!(
            evaluate(&path, &format!("raw-rank-{cycle}"), &reopened, &late),
            Err(EngineError::MappingException {
                node: raise,
                message: Some("2".into())
            })
        );
        current = reopened;
    }
    for (name, bytes) in original_files {
        assert_eq!(std::fs::read(path.join(name)).unwrap(), bytes);
    }
}

fn refused(path: &Path, design: &str) {
    let mapping = stage(path, design, "", "");
    let before = files(path);
    let options = ImportOptions::default()
        .with_package_root(path)
        .with_item_ordered_exceptions();
    let partial = mfd::import_with_options(&mapping, &options);
    match &partial {
        Ok(imported) => {
            retain(path, "partial-project-full", &imported.project);
            retain(path, "partial-warnings", &imported.warnings);
            std::fs::write(
                path.join("partial-project.json"),
                serde_json::to_vec_pretty(&imported.project).unwrap(),
            )
            .unwrap();
        }
        Err(error) => retain(path, "partial-error", error),
    }
    let partial = partial.unwrap();
    assert!(
        partial
            .warnings
            .iter()
            .any(|warning| warning.contains("item-ordered exception"))
    );
    assert!(partial.project.failure_rules.is_empty());
    assert!(
        !partial
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Raise { .. }))
    );
    let strict = mfd::import_with_profile(&mapping, &options, ImportProfile::Executable);
    match &strict {
        Ok(outcome) => retain(path, "unexpected-report", &outcome.report),
        Err(error) => retain(path, "strict-error", error),
    }
    let report = match strict {
        Err(mfd::MfdError::IncompatibleImport(report)) => report,
        Err(error) => panic!("typed strict refusal required: {error}"),
        Ok(_) => panic!("unsupported opt-in must refuse strict import"),
    };
    assert!(!report.executable);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::ImportWarning
                && issue.message.contains("item-ordered exception"))
    );
    for (name, bytes) in before {
        assert_eq!(std::fs::read(path.join(name)).unwrap(), bytes);
    }
    assert!(!path.join("accepted.xml").exists());
    assert!(!path.join("project.json").exists());
}

#[test]
fn unsupported_owner_fanout_controls_and_multiple_rules_refuse_without_global_fallback_or_publication()
 {
    let dir = Directory::new("ownership-refusals");
    let base = design(false);
    let variants = [
        ("shared-keep", base.replace("      <edge from=\"12\" to=\"31\"/>", "      <edge from=\"22\" to=\"31\"/>")),
        ("shared-throw", base.replace("      <edge from=\"12\" to=\"31\"/>", "      <edge from=\"23\" to=\"31\"/>")),
        ("raw-fanout", base.replace("      <edge from=\"12\" to=\"31\"/>", "      <edge from=\"10\" to=\"31\"/>")),
        ("unfiltered-throw", base.replace("from=\"23\" to=\"40\"", "from=\"10\" to=\"40\"")),
        ("duplicate-sink", base.replace("      <edge from=\"12\" to=\"31\"/>", "      <edge from=\"12\" to=\"31\"/>\n      <edge from=\"11\" to=\"21\"/>")),
        ("duplicate-port", base.replace("<entry name=\"Description\" outkey=\"12\"/>", "<entry name=\"Description\" outkey=\"12\"/><entry name=\"Description\" outkey=\"12\"/>")),
        ("multiple-exceptions", base.replace("        <component name=\"reject-expense\"", r#"        <component name="second-exception" library="core" kind="18" uid="6"><sources><datapoint pos="0" key="60"/></sources><data><exception/></data></component>
        <component name="reject-expense""#).replace("      <edge from=\"12\" to=\"41\"/>", "      <edge from=\"12\" to=\"41\"/>\n      <edge from=\"23\" to=\"60\"/>")),
        ("remote-input", base.replace("inputinstance=\"expenses.xml\"", "inputinstance=\"https://example.invalid/expenses.xml\"")),
        ("extra-source", base.replace("        <component name=\"Accepted\"", r#"        <component name="Secondary" library="xml" kind="14" uid="6"><data><root><entry name="FileInstance"><entry name="document"><entry name="Expenses"><entry name="Expense" outkey="70"><entry name="Description" outkey="71"/></entry></entry></entry></entry></root><document schema="exception-source.xsd" inputinstance="expenses.xml" instanceroot="{}Expenses"/></data></component>
        <component name="Accepted""#).replace("from=\"12\" to=\"41\"", "from=\"71\" to=\"41\"")),
        ("extra-target", base.replace("        <component name=\"reject-expense\"", r#"        <component name="Audit" library="xml" kind="14" uid="6"><data><root><entry name="FileInstance"><entry name="document"><entry name="Accepted"><entry name="Expense" inpkey="70"><entry name="Description" inpkey="71"/></entry></entry></entry></entry></root><document schema="exception-target.xsd" outputinstance="audit.xml" instanceroot="{}Accepted"/></data></component>
        <component name="reject-expense""#).replace("      <edge from=\"12\" to=\"41\"/>", "      <edge from=\"12\" to=\"41\"/>\n      <edge from=\"22\" to=\"70\"/>\n      <edge from=\"12\" to=\"71\"/>")),
        ("chained-filter", base.replace("        <component name=\"reject-expense\"", r#"        <component name="filter" library="core" kind="3" uid="6"><sources><datapoint pos="0" key="60"/><datapoint pos="1" key="61"/></sources><targets><datapoint pos="0" key="62"/><datapoint pos="1" key="63"/></targets></component>
        <component name="reject-expense""#).replace("from=\"23\" to=\"40\"", "from=\"63\" to=\"40\"").replace("      <edge from=\"12\" to=\"41\"/>", "      <edge from=\"12\" to=\"41\"/>\n      <edge from=\"22\" to=\"60\"/>\n      <edge from=\"11\" to=\"61\"/>\n      <edge from=\"62\" to=\"31\"/>")),
    ];
    for (label, variant) in variants {
        refused(&dir.case(label), &variant);
    }
}

#[test]
fn bounded_message_dependency_refusal_and_no_exception_import_preserve_original_modes() {
    let dir = Directory::new("bounded-and-ordinary");
    let mut components = String::new();
    let mut edges = String::new();
    let mut feed = 11;
    for index in 0..65 {
        let input = 100 + index * 2;
        let output = input + 1;
        components.push_str(&format!("<component name=\"logical-not\" library=\"core\" kind=\"5\" uid=\"{}\"><sources><datapoint pos=\"0\" key=\"{input}\"/></sources><targets><datapoint pos=\"0\" key=\"{output}\"/></targets></component>\n", index + 6));
        edges.push_str(&format!("<edge from=\"{feed}\" to=\"{input}\"/>\n"));
        feed = output;
    }
    let variant = design(false)
        .replace(
            "        <component name=\"reject-expense\"",
            &format!("{components}        <component name=\"reject-expense\""),
        )
        .replace(
            "      <edge from=\"12\" to=\"41\"/>",
            &format!("{edges}<edge from=\"{feed}\" to=\"41\"/>"),
        );
    refused(&dir.case("message-depth"), &variant);
    let path = dir.case("no-exception");
    let original = design(false);
    let component_start = original
        .find("        <component name=\"reject-expense\"")
        .unwrap();
    let component_end = original[component_start..].find("</component>").unwrap()
        + component_start
        + "</component>".len();
    let source = format!(
        "{}{}",
        &original[..component_start],
        &original[component_end..]
    )
    .replace("      <edge from=\"23\" to=\"40\"/>\n", "")
    .replace("      <edge from=\"12\" to=\"41\"/>\n", "");
    let mapping = stage(&path, &source, "", "");
    let ordinary = imported(&path, &mapping, false);
    let opted = imported(&path, &mapping, true);
    assert_eq!(
        serde_json::to_vec_pretty(&ordinary).unwrap(),
        serde_json::to_vec_pretty(&opted).unwrap()
    );
}

fn projection_refusal(path: &Path, mapping: &Path, default_executable: bool) {
    let ordinary = if default_executable {
        Some(imported(path, mapping, false))
    } else {
        None
    };
    let before = files(path);
    let options = ImportOptions::default()
        .with_package_root(path)
        .with_item_ordered_exceptions();
    let partial = mfd::import_with_options(mapping, &options);
    match &partial {
        Ok(outcome) => {
            retain(path, "projection-partial-project", &outcome.project);
            retain(path, "projection-partial-warnings", &outcome.warnings);
        }
        Err(error) => retain(path, "projection-partial-error", error),
    }
    let partial = partial.unwrap();
    assert!(
        partial.warnings.iter().any(|warning| warning
            .contains("scalar expressions must read the exact owner item or its current position")),
        "{:?}",
        partial.warnings
    );
    assert!(partial.project.failure_rules.is_empty());
    assert!(
        !partial
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Raise { .. }))
    );
    if let Some(ordinary) = ordinary {
        assert_eq!(
            serde_json::to_value(&partial.project.root).unwrap(),
            serde_json::to_value(&ordinary.root).unwrap()
        );
    }
    let strict = mfd::import_with_profile(mapping, &options, ImportProfile::Executable);
    match &strict {
        Ok(outcome) => retain(path, "projection-unexpected-report", &outcome.report),
        Err(error) => retain(path, "projection-strict-error", error),
    }
    let report = match strict {
        Err(mfd::MfdError::IncompatibleImport(report)) => report,
        Err(error) => panic!("typed projection refusal required: {error}"),
        Ok(_) => panic!("off-owner opt-in must refuse"),
    };
    assert!(!report.executable);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::ImportWarning
                && issue.message.contains(
                    "scalar expressions must read the exact owner item or its current position"
                ))
    );
    for (name, bytes) in before {
        assert_eq!(std::fs::read(path.join(name)).unwrap(), bytes);
    }
    assert!(!path.join("accepted.xml").exists());
    assert!(!path.join("project.json").exists());
}

fn sibling_projection_design(source: String) -> String {
    source.replace("<entry name=\"Description\" outkey=\"12\"/></entry>", "<entry name=\"Description\" outkey=\"12\"/></entry><entry name=\"Other\" outkey=\"70\"><entry name=\"Text\" outkey=\"71\"/><entry name=\"Allowed\" outkey=\"72\"/></entry><entry name=\"Marker\" outkey=\"73\"/>")
}

fn sibling_projection_schema(path: &Path) {
    // This is a complete valid closed schema, with a sibling repeating group
    // and an explicitly singular root scalar. Neither is the selected Expense.
    std::fs::write(path.join("exception-source.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Expenses"><xs:complexType><xs:sequence><xs:element name="Expense" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Allowed" type="xs:boolean"/><xs:element name="Description" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Other" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/><xs:element name="Allowed" type="xs:boolean"/></xs:sequence></xs:complexType></xs:element><xs:element name="Marker" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
}

#[test]
fn off_owner_sibling_nested_repetition_and_foreign_position_refuse_without_global_fallback() {
    let dir = Directory::new("projection-refusals");
    let sibling = sibling_projection_design(design(false));
    assert_ne!(sibling, design(false));
    let position = sibling.replace("        <component name=\"reject-expense\"", r#"        <component name="position" library="core" kind="5" uid="6"><sources><datapoint pos="0" key="80"/></sources><targets><datapoint pos="0" key="81"/></targets></component>
        <component name="reject-expense""#).replace("from=\"12\" to=\"41\"", "from=\"81\" to=\"41\"").replace("      <edge from=\"12\" to=\"31\"/>", "      <edge from=\"12\" to=\"31\"/>\n      <edge from=\"70\" to=\"80\"/>");
    for (label, source, default_executable) in [
        (
            "sibling-message",
            sibling.replace("from=\"12\" to=\"41\"", "from=\"71\" to=\"41\""),
            true,
        ),
        (
            "sibling-predicate",
            sibling.replace("from=\"11\" to=\"21\"", "from=\"72\" to=\"21\""),
            true,
        ),
        (
            "sibling-target",
            sibling.replace("from=\"12\" to=\"31\"", "from=\"71\" to=\"31\""),
            true,
        ),
        (
            "singular-root-message",
            sibling.replace("from=\"12\" to=\"41\"", "from=\"73\" to=\"41\""),
            true,
        ),
        // Foreign Position is deliberately not claimed to be a valid legacy
        // executable context; the explicit projection warning must precede
        // guard publication rather than rely on final engine validation.
        ("foreign-position", position, false),
    ] {
        let path = dir.case(label);
        let mapping = stage(&path, &source, "", "");
        sibling_projection_schema(&path);
        projection_refusal(&path, &mapping, default_executable);
    }
    let nested_schema = r#"<xs:element name="Child" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"#;
    let nested = design(false).replace("<entry name=\"Description\" outkey=\"12\"/>", "<entry name=\"Description\" outkey=\"12\"/><entry name=\"Child\" outkey=\"70\"><entry name=\"Text\" outkey=\"71\"/></entry>");
    for (label, sink) in [("nested-message", 41), ("nested-target", 31)] {
        let path = dir.case(label);
        let source = nested.replace(
            &format!("from=\"12\" to=\"{sink}\""),
            &format!("from=\"71\" to=\"{sink}\""),
        );
        let mapping = stage(&path, &source, nested_schema, "");
        projection_refusal(&path, &mapping, true);
    }
}

#[test]
fn singular_owner_descendant_scalar_keeps_current_item_lazy_message_and_full_target() {
    let dir = Directory::new("owned-singular-descendant");
    let path = dir.case("details");
    let source = design(false).replace("<entry name=\"Description\" outkey=\"12\"/>", "<entry name=\"Description\" outkey=\"12\"/><entry name=\"Details\"><entry name=\"Text\" outkey=\"71\"/></entry>")
        .replace("from=\"12\" to=\"31\"", "from=\"71\" to=\"31\"").replace("from=\"12\" to=\"41\"", "from=\"71\" to=\"41\"");
    let details_schema = r#"<xs:element name="Details"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element>"#;
    let mapping = stage(&path, &source, details_schema, "");
    std::fs::write(path.join("expenses.xml"), "<Expenses><Expense><Allowed>true</Allowed><Description>outer</Description><Details><Text>one nested</Text></Details></Expense></Expenses>").unwrap();
    let ordinary = imported(&path, &mapping, false);
    let opted = imported(&path, &mapping, true);
    let (predicate, raise, message) = guard(&opted, false);
    assert_eq!(predicate, source_predicate(&ordinary));
    let message = message.unwrap();
    assert!(
        matches!(opted.graph.nodes.get(&message), Some(Node::SourceField { path, frame }) if path.as_slice() == ["Details", "Text"] && frame.as_deref() == Some(&["Expense".to_string()][..]))
    );
    assert_eq!(opted.root.children[0].bindings[0].node, message);
    let item = |allowed, text: &str| {
        group(vec![
            ("Allowed", Instance::Scalar(Value::Bool(allowed))),
            (
                "Description",
                Instance::Scalar(Value::String("outer ignored".into())),
            ),
            (
                "Details",
                group(vec![("Text", Instance::Scalar(Value::String(text.into())))]),
            ),
        ])
    };
    let accepted = input(vec![item(true, "one nested"), item(true, "two 😀 nested")]);
    let expected = group(vec![(
        "Expense",
        Instance::Repeated(vec![
            group(vec![(
                "Description",
                Instance::Scalar(Value::String("one nested".into())),
            )]),
            group(vec![(
                "Description",
                Instance::Scalar(Value::String("two 😀 nested".into())),
            )]),
        ]),
    )]);
    assert_eq!(
        evaluate(&path, "nested-owner-target", &opted, &accepted),
        Ok(expected)
    );
    let selected = input(vec![item(true, "unselected"), item(false, "late nested")]);
    assert_eq!(
        evaluate(&path, "nested-owner-message", &opted, &selected),
        Err(EngineError::MappingException {
            node: raise,
            message: Some("late nested".into())
        })
    );
}
