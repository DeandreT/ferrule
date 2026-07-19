use std::path::{Path, PathBuf};

use engine::EngineError;
use ir::{Instance, Value};
use mapping::{FailureIteration, FailureSelection};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_{tag}_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn expense(allowed: bool, description: &str) -> Instance {
    Instance::Group(vec![
        ("Allowed".into(), Instance::Scalar(Value::Bool(allowed))),
        (
            "Description".into(),
            Instance::Scalar(Value::String(description.into())),
        ),
    ])
}

fn expenses(items: Vec<Instance>) -> Instance {
    Instance::Group(vec![("Expense".into(), Instance::Repeated(items))])
}

fn import_variant(tag: &str, transform: impl FnOnce(String) -> String) -> mfd::Imported {
    let source = std::fs::read_to_string(fixture("exception.mfd")).unwrap();
    let temp = TempDir::new(tag);
    std::fs::write(temp.0.join("exception.mfd"), transform(source)).unwrap();
    std::fs::copy(
        fixture("exception-source.xsd"),
        temp.0.join("exception-source.xsd"),
    )
    .unwrap();
    std::fs::copy(
        fixture("exception-target.xsd"),
        temp.0.join("exception-target.xsd"),
    )
    .unwrap();
    mfd::import(&temp.0.join("exception.mfd")).unwrap()
}

fn add_generated_sequence(source: String) -> String {
    source
        .replace(
            "        <component name=\"reject-expense\"",
            r#"        <component name="constant" library="core" kind="2" uid="6"><targets><datapoint pos="0" key="50"/></targets><data><constant value="1" datatype="integer"/></data></component>
        <component name="constant" library="core" kind="2" uid="7"><targets><datapoint pos="0" key="51"/></targets><data><constant value="2" datatype="integer"/></data></component>
        <component name="generate-sequence" library="core" kind="5" uid="8"><sources><datapoint pos="0" key="52"/><datapoint pos="1" key="53"/></sources><targets><datapoint pos="0" key="54"/></targets></component>
        <component name="reject-expense""#,
        )
        .replace(
            "      <edge from=\"23\" to=\"40\"/>",
            "      <edge from=\"50\" to=\"52\"/>\n      <edge from=\"51\" to=\"53\"/>\n      <edge from=\"54\" to=\"40\"/>",
        )
        .replace("      <edge from=\"12\" to=\"41\"/>\n", "")
}

fn add_secondary_exception_source(source: String, dynamic: bool) -> String {
    let file_input = if dynamic { " inpkey=\"69\"" } else { "" };
    let input_instance = if dynamic {
        ""
    } else {
        " inputinstance=\"secondary.xml\""
    };
    let component = format!(
        r#"        <component name="Secondary" library="xml" kind="14" uid="6">
          <data>
            <root><entry name="FileInstance"{file_input}><entry name="document"><entry name="Expenses"><entry name="Expense" outkey="70"><entry name="Allowed" outkey="71"/><entry name="Description" outkey="72"/></entry></entry></entry></entry></root>
            <document schema="exception-source.xsd"{input_instance} instanceroot="{{}}Expenses"/>
          </data>
        </component>
"#
    );
    let source = source
        .replace(
            "        <component name=\"Accepted\"",
            &format!("{component}        <component name=\"Accepted\""),
        )
        .replace(
            "      <edge from=\"23\" to=\"40\"/>",
            "      <edge from=\"70\" to=\"40\"/>",
        )
        .replace(
            "      <edge from=\"12\" to=\"41\"/>",
            "      <edge from=\"72\" to=\"41\"/>",
        );
    if dynamic {
        source.replace(
            "      <edge from=\"70\" to=\"40\"/>",
            "      <edge from=\"12\" to=\"69\"/>\n      <edge from=\"70\" to=\"40\"/>",
        )
    } else {
        source
    }
}

fn add_earlier_exception(source: String) -> String {
    source
        .replace(
            "        <component name=\"reject-expense\"",
            r#"        <component name="constant" library="core" kind="2" uid="6"><targets><datapoint pos="0" key="60"/></targets><data><constant value="first rule" datatype="string"/></data></component>
        <component name="first-exception" library="core" kind="18" uid="7"><sources><datapoint pos="0" key="61"/><datapoint pos="1" key="62"/></sources><data><exception/></data></component>
        <component name="reject-expense""#,
        )
        .replace(
            "      <edge from=\"23\" to=\"40\"/>",
            "      <edge from=\"10\" to=\"61\"/>\n      <edge from=\"60\" to=\"62\"/>\n      <edge from=\"23\" to=\"40\"/>",
        )
}

#[test]
fn imports_filter_false_exception_with_item_scoped_message() {
    let imported = mfd::import(&fixture("exception.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let [rule] = imported.project.failure_rules.as_slice() else {
        panic!("expected one imported failure rule");
    };
    assert_eq!(
        rule.iteration,
        FailureIteration::Source {
            collection: vec!["Expense".into()],
        }
    );
    assert!(matches!(rule.selection, FailureSelection::WhenFalse { .. }));
    assert!(rule.message.is_some());

    let output = engine::run(
        &imported.project,
        &expenses(vec![expense(true, "train"), expense(true, "meal")]),
    )
    .unwrap();
    assert_eq!(
        output
            .field("Expense")
            .and_then(Instance::as_repeated)
            .map(|items| items.len()),
        Some(2)
    );

    let error = engine::run(
        &imported.project,
        &expenses(vec![expense(true, "train"), expense(false, "private jet")]),
    )
    .unwrap_err();
    assert_eq!(
        error,
        EngineError::MappingFailure {
            rule: 1,
            message: Some("private jet".into()),
        }
    );
}

#[test]
fn disconnected_opposite_filter_branch_does_not_become_a_live_exception() {
    let imported = import_variant("exception_unconnected_branch", |source| {
        source.replace("<edge from=\"22\" to=\"30\"/>\n", "")
    });
    assert!(imported.project.failure_rules.is_empty());
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("distinct connected filter branches"));
}

#[test]
fn duplicate_filter_branch_keys_do_not_guess_polarity() {
    let imported = import_variant("exception_duplicate_filter_branches", |source| {
        source
            .replace(
                "<datapoint pos=\"1\" key=\"23\"/>",
                "<datapoint pos=\"1\" key=\"22\"/>",
            )
            .replace(
                "<edge from=\"23\" to=\"40\"/>",
                "<edge from=\"22\" to=\"40\"/>",
            )
    });
    assert!(imported.project.failure_rules.is_empty());
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("distinct connected filter branches"));
    assert!(engine::validate(&imported.project).is_empty());
}

#[test]
fn imports_filter_true_exception_without_inverting_its_predicate() {
    let imported = import_variant("exception_true_branch", |source| {
        source
            .replace(
                "<edge from=\"22\" to=\"30\"/>",
                "<edge from=\"23\" to=\"30\"/>",
            )
            .replace(
                "<edge from=\"23\" to=\"40\"/>",
                "<edge from=\"22\" to=\"40\"/>",
            )
    });
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.failure_rules[0].selection,
        FailureSelection::WhenTrue { .. }
    ));
    assert_eq!(
        engine::run(
            &imported.project,
            &expenses(vec![
                expense(false, "allowed branch"),
                expense(true, "stop")
            ]),
        )
        .unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: Some("stop".into()),
        }
    );
}

#[test]
fn omitted_optional_error_text_stays_absent() {
    let imported = import_variant("exception_no_message", |source| {
        source.replace("<edge from=\"12\" to=\"41\"/>\n", "")
    });
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.failure_rules[0].message, None);
    assert_eq!(
        engine::run(
            &imported.project,
            &expenses(vec![expense(false, "not surfaced")]),
        )
        .unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: None,
        }
    );
}

#[test]
fn direct_collection_feed_imports_as_an_unconditional_failure() {
    let imported = import_variant("exception_direct_collection", |source| {
        source.replace(
            "<edge from=\"23\" to=\"40\"/>",
            "<edge from=\"10\" to=\"40\"/>",
        )
    });
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.failure_rules[0].selection,
        FailureSelection::All
    );
    assert_eq!(
        engine::run(
            &imported.project,
            &expenses(vec![expense(true, "first expense")]),
        )
        .unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: Some("first expense".into()),
        }
    );
}

#[test]
fn direct_generated_sequence_imports_as_an_unconditional_failure() {
    let imported = import_variant("exception_generated_sequence", add_generated_sequence);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.failure_rules[0].iteration,
        FailureIteration::Sequence { .. }
    ));
    assert_eq!(
        engine::run(&imported.project, &expenses(Vec::new())).unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: None,
        }
    );
}

#[test]
fn source_dependent_generated_sequence_exception_is_skipped() {
    let imported = import_variant("exception_source_dependent_sequence", |source| {
        add_generated_sequence(source).replace(
            "      <edge from=\"51\" to=\"53\"/>",
            "      <edge from=\"11\" to=\"53\"/>",
        )
    });
    assert!(imported.project.failure_rules.is_empty());
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("depend on a repeated source context"));
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        engine::run(
            &imported.project,
            &expenses(vec![expense(true, "still mapped")]),
        )
        .is_ok()
    );
}

#[test]
fn filter_target_owned_generated_sequence_exception_is_skipped() {
    let imported = import_variant("exception_shared_target_sequence", |source| {
        add_generated_sequence(source).replace(
            "      <edge from=\"10\" to=\"20\"/>",
            "      <edge from=\"54\" to=\"20\"/>",
        )
    });
    assert!(imported.project.failure_rules.is_empty());
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("another target or sequence consumer"));
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        engine::run(
            &imported.project,
            &expenses(vec![expense(true, "still mapped")]),
        )
        .is_ok()
    );
}

#[test]
fn static_secondary_source_exception_uses_its_named_context() {
    let imported = import_variant("exception_static_secondary", |source| {
        add_secondary_exception_source(source, false)
    });
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let [secondary] = imported.project.extra_sources.as_slice() else {
        panic!("expected one secondary source");
    };
    assert_eq!(
        imported.project.failure_rules[0].iteration,
        FailureIteration::Source {
            collection: vec![secondary.name.clone(), "Expense".into()],
        }
    );
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        engine::run_with_sources(
            &imported.project,
            &expenses(vec![expense(true, "primary")]),
            vec![(
                secondary.name.clone(),
                expenses(vec![expense(true, "secondary failure")]),
            )],
        )
        .unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: Some("secondary failure".into()),
        }
    );
}

#[test]
fn dynamic_secondary_source_exception_warns_and_is_skipped() {
    let imported = import_variant("exception_dynamic_secondary", |source| {
        add_secondary_exception_source(source, true)
    });
    assert!(imported.project.failure_rules.is_empty());
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("per-item dynamic secondary source"));
    assert!(imported.project.extra_sources[0].dynamic_path.is_some());
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        engine::run(
            &imported.project,
            &expenses(vec![expense(true, "secondary.xml")]),
        )
        .is_ok()
    );
}

#[test]
fn failure_rules_execute_in_component_declaration_order() {
    let imported = import_variant("exception_rule_order", add_earlier_exception);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.failure_rules.len(), 2);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        engine::run(
            &imported.project,
            &expenses(vec![expense(false, "second rule")]),
        )
        .unwrap_err(),
        EngineError::MappingFailure {
            rule: 1,
            message: Some("first rule".into()),
        }
    );
}

#[test]
fn missing_throw_pin_warns_once_and_does_not_create_a_rule() {
    let imported = import_variant("exception_missing_throw_pin", |source| {
        source.replace(
            "<datapoint pos=\"0\" key=\"40\"/>",
            "<datapoint pos=\"0\"/>",
        )
    });
    assert!(imported.project.failure_rules.is_empty());
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("missing its `throw` input pin"));
    assert!(engine::validate(&imported.project).is_empty());
}
