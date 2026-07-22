use std::path::{Path, PathBuf};

use ir::{Instance, SchemaKind, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_case_insensitive_external_source_and_executes_nested_records() {
    let imported = mfd::import(&fixture("flextext-source.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("flextext/source.flex")
    );
    assert!(engine::validate(&imported.project).is_empty());
    let layout = imported.project.source_options.flextext.as_ref().unwrap();
    let source = format_flextext::read(
        &fixture("flextext/source.flex"),
        &imported.project.source,
        layout,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let sections = output
        .field("Section")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(sections.len(), 1);
    assert_eq!(
        sections[0].field("Title").and_then(Instance::as_scalar),
        Some(&Value::String("Demo".into()))
    );
    let rows = sections[0]
        .field("Row")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Bob".into()))
    );
    assert_eq!(
        rows[1].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(3))
    );
}

#[test]
fn imports_external_target_and_renders_fixed_width_records() {
    let imported = mfd::import(&fixture("flextext-target.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("flextext/target.flex")
    );
    assert!(engine::validate(&imported.project).is_empty());
    let body = imported
        .project
        .target
        .child("Sections")
        .unwrap()
        .child("Body")
        .unwrap();
    assert!(body.repeating);
    assert!(matches!(body.kind, SchemaKind::Group { .. }));
    let source = format_xml::read(
        &fixture("flextext/target-source.xml"),
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let layout = imported.project.target_options.flextext.as_ref().unwrap();
    let rendered = format_flextext::to_string(&imported.project.target, &target, layout).unwrap();
    assert_eq!(
        rendered,
        "Quotes\nAda     1815Analytical  \nGrace   1906Compiler    "
    );
}

#[test]
fn imports_and_executes_regex_switch_conditions() {
    let imported = mfd::import(&fixture("flextext-regex-switch.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let layout = imported.project.source_options.flextext.as_ref().unwrap();
    let parsed = format_flextext::from_str(
        "prefix alert-42 suffix\nINFO ready\nplain\n",
        &imported.project.source,
        layout,
    )
    .unwrap();
    let lines = parsed
        .field("Lines")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(
        lines[0]
            .field("Classified")
            .and_then(|group| group.field("Alert"))
            .and_then(Instance::as_scalar),
        Some(&Value::String("prefix alert-42 suffix".into()))
    );
    assert!(
        lines[1]
            .field("Classified")
            .and_then(|group| group.field("Info"))
            .is_some()
    );
    assert!(
        lines[2]
            .field("Classified")
            .and_then(|group| group.field("Other"))
            .is_some()
    );

    let output = engine::run(&imported.project, &parsed).unwrap();
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("prefix alert-42 suffix".into()))
    );
}
