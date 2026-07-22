use super::*;

fn collection_find_program(named: bool) -> Program {
    let people = SchemaNode::group(
        "People",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Selected", ScalarType::Bool),
        ],
    )
    .repeating();
    let source = if named {
        SchemaNode::group("Source", Vec::new())
    } else {
        SchemaNode::group("Source", vec![people.clone()])
    };
    let collection = if named {
        vec!["Catalog".into(), "People".into()]
    } else {
        vec!["People".into()]
    };
    Program {
        source,
        extra_sources: named
            .then(|| NamedSourceProgram {
                name: "Catalog".into(),
                source: SchemaNode::group("Catalog", vec![people]),
            })
            .into_iter()
            .collect(),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Found", ScalarType::String)],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: Some(collection.clone()),
                    path: vec!["Selected".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: Some(collection.clone()),
                    path: vec!["Name".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::CollectionFind {
                    collection,
                    predicate: 1,
                    value: 2,
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Found".into(),
                expression: 3,
                target_type: ScalarType::String,
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn emits_nullable_short_circuiting_collection_find() {
    let artifacts = emit(
        &collection_find_program(false),
        &Options {
            package_name: "collection-find".into(),
            runtime_dependency: RuntimeDependency::Version("1".into()),
        },
    )
    .expect("collection-find program emits");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated Rust source is UTF-8");

    assert!(source.contains("context.collection_find_items(&[\"People\"])?;"));
    assert!(source.contains("collection_find_selected(1, predicate)?"));
    assert!(source.contains("return expression_2(&item_context);"));
    assert!(source.contains("Ok(Value::Null)"));
}

#[test]
fn generated_package_finds_first_true_named_item_and_preserves_errors() {
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap();
    let output = TempDir::new("rust_collection_find_codegen");
    let artifacts = emit(
        &collection_find_program(true),
        &Options {
            package_name: "collection-find-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .unwrap();
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{RuntimeError, Value, boolean, field, group, repeated, scalar, string};
use collection_find_map::NamedInput;

fn row(name: &str, selected: Value) -> codegen_runtime::Instance {
    group([
        field("Name", scalar(string(name))),
        field("Selected", scalar(selected)),
    ])
}

fn selected_only(selected: Value) -> codegen_runtime::Instance {
    group([field("Selected", scalar(selected))])
}

fn found(output: &codegen_runtime::Instance) -> Option<&Value> {
    output.field("Found").and_then(codegen_runtime::Instance::as_scalar)
}

fn main() {
    let source = group(Vec::new());
    let catalog = group([field(
        "People",
        repeated([
            selected_only(Value::Null),
            row("first", boolean(true)),
            row("must stay lazy", string("not a bool")),
        ]),
    )]);
    let inputs = [NamedInput { name: "Catalog", instance: &catalog }];
    let output = collection_find_map::execute_with_sources(&source, &inputs).unwrap();
    assert_eq!(found(&output), Some(&string("first")));

    let none = group([field(
        "People",
        repeated([selected_only(Value::Null), selected_only(boolean(false))]),
    )]);
    let inputs = [NamedInput { name: "Catalog", instance: &none }];
    let output = collection_find_map::execute_with_sources(&source, &inputs).unwrap();
    assert_eq!(found(&output), Some(&Value::Null));

    let invalid = group([field("People", repeated([row("bad", string("no"))]))]);
    let inputs = [NamedInput { name: "Catalog", instance: &invalid }];
    assert_eq!(
        collection_find_map::execute_with_sources(&source, &inputs),
        Err(RuntimeError::NotABool { node: 1, found: "string" }),
    );

}
"#,
    )
    .unwrap();

    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "generated Rust collection-find project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
