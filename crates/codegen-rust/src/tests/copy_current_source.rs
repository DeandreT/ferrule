use super::*;

#[test]
fn repeated_copy_current_source_emits_owned_groups_and_executes()
-> Result<(), Box<dyn std::error::Error>> {
    let row =
        SchemaNode::group("Row", vec![SchemaNode::scalar("Name", ScalarType::String)]).repeating();
    let program = Program {
        source: SchemaNode::group("Source", vec![row.clone()]),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", vec![row]),
        expressions: Vec::new(),
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: vec![TargetScope {
                target_field: "Row".into(),
                repeating: true,
                iteration: Some(IterationPlan::source(vec!["Row".into()])),
                construction: TargetConstruction::CopyCurrentSource,
                bindings: Vec::new(),
                children: Vec::new(),
            }],
        },
        extra_targets: Vec::new(),
    };
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("codegen runtime has a workspace parent")?
        .join("codegen-runtime");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "generated-copy-current".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains("let output = (&item_context).copy_current_group()?;"));

    let output = TempDir::new("rust_repeated_copy_current_source_codegen");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{field, group, repeated, scalar, Value};
use generated_copy_current::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rows = repeated([
        group([field("Name", scalar(Value::String("one".into())))]),
        group([field("Name", scalar(Value::String("two".into())))]),
    ]);
    let input = group([field("Row", rows.clone())]);
    let expected = group([field("Row", rows)]);
    assert_eq!(execute(&input)?, expected);
    Ok(())
}
"#,
    )?;
    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()?;
    assert!(
        result.status.success(),
        "generated Rust repeated copy failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
