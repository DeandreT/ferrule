use std::path::{Path, PathBuf};

use ir::{Instance, Value};
use mapping::Node;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_schemas_scopes_and_functions() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let project = &imported.project;

    // Schemas come from the referenced XSDs (typed, repeating).
    assert_eq!(project.source.name, "Company");
    assert!(project.source.child("Staff").unwrap().repeating);
    assert_eq!(project.target.name, "People");
    assert!(project.target.child("Person").unwrap().repeating);

    // The Staff -> Person repeating connection becomes a scope.
    assert_eq!(project.root.children.len(), 1);
    let person = &project.root.children[0];
    assert_eq!(person.target_field, "Person");
    assert_eq!(person.source, Some(vec!["Staff".to_string()]));

    // Name <- concat(First, " ", Last); Age <- Age.
    assert_eq!(person.bindings.len(), 2);
    let name_binding = person
        .bindings
        .iter()
        .find(|b| b.target_field == "Name")
        .unwrap();
    let Node::Call { function, args } = &project.graph.nodes[&name_binding.node] else {
        panic!("Name should be bound to a call");
    };
    assert_eq!(function, "concat");
    assert_eq!(args.len(), 3);
    assert!(matches!(
        &project.graph.nodes[&args[0]],
        Node::SourceField { path } if path == &["First"]
    ));
    assert!(matches!(
        &project.graph.nodes[&args[1]],
        Node::Const { value: Value::String(s) } if s == " "
    ));

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
}

#[test]
fn imported_project_runs() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let source = format_xml::read(&fixture("people.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();

    let people = target
        .field("Person")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        people[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Alice Carter".into()))
    );
    assert_eq!(
        people[1].field("Age").and_then(Instance::as_scalar),
        Some(&Value::Int(41))
    );
}

#[test]
fn export_then_import_roundtrips_semantically() {
    let imported = mfd::import(&fixture("people.mfd")).unwrap();
    let dir = std::env::temp_dir().join(format!("ferrule_mfd_roundtrip_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("people.mfd");

    let warnings = mfd::export(&imported.project, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&out).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let a = &imported.project;
    let b = &reimported.project;
    assert_eq!(a.source, b.source);
    assert_eq!(a.target, b.target);
    // Scope shape survives.
    assert_eq!(b.root.children.len(), 1);
    assert_eq!(b.root.children[0].source, a.root.children[0].source);
    assert_eq!(
        b.root.children[0].bindings.len(),
        a.root.children[0].bindings.len()
    );
    // The reimported project must still run and produce the same output.
    let source = format_xml::read(&fixture("people.xml"), &b.source).unwrap();
    let out_a = engine::run(a, &source).unwrap();
    let out_b = engine::run(b, &source).unwrap();
    assert_eq!(out_a, out_b);
}
