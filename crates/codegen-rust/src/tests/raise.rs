use super::*;

include!("../../../codegen/tests/fixtures/raise_project.rs");

#[test]
fn generated_raise_preserves_lazy_messages_and_item_error_order() {
    let project = raise_project();
    let lowered = codegen::lower(&project).expect("lazy Raise project lowers");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let output = TempDir::new("rust_lazy_raise");
    let directory = output.path().to_path_buf();
    // Keep all originals after infrastructure/runtime failures as well as semantic panics.
    std::mem::forget(output);
    let artifacts = emit(
        &lowered,
        &Options {
            package_name: "raise-map".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .expect("static Raise project emits");
    write_artifacts(&directory, &artifacts);
    fs::write(directory.join("src/main.rs"), HOST).unwrap();
    let launched = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(&directory)
        .generated_host_output(&directory);
    fs::write(
        directory.join("HOST_FULL_RESULT.debug.txt"),
        format!("{launched:#?}\n"),
    )
    .unwrap();
    let original = launched.expect("generated host launch failed; complete result retained");
    fs::write(directory.join("HOST_STDOUT.bin"), &original.stdout).unwrap();
    fs::write(directory.join("HOST_STDERR.bin"), &original.stderr).unwrap();
    assert!(
        original.status.success(),
        "generated Raise host failed; originals at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&original.stdout),
        String::from_utf8_lossy(&original.stderr)
    );
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
        fs::remove_dir_all(&directory).unwrap();
    }
}

const HOST: &str = r#"use std::fmt::Debug;
use std::path::Path;
use codegen_runtime::{ExecutionContext, FunctionError, Instance, RuntimeError, RuntimeValue, Value, field, group, repeated, scalar};

fn source(mode: i64, global: bool, rows: &[(i64, i64, bool, &str)]) -> Instance {
    group([
        field("Mode", scalar(Value::Int(mode))), field("Global", scalar(Value::Bool(global))),
        field("Item", repeated(rows.iter().map(|(value, denominator, blocked, message)| group([
            field("Value", scalar(Value::Int(*value))), field("Denominator", scalar(Value::Int(*denominator))),
            field("Blocked", scalar(Value::Bool(*blocked))), field("Message", scalar(Value::String((*message).into()))),
        ])))),
    ])
}
fn original<T: Debug>(label: &str, actual: Result<T, RuntimeError>) -> Result<T, RuntimeError> {
    println!("{label}: {actual:#?}");
    actual
}
fn failure<T: Debug>(label: &str, actual: Result<T, RuntimeError>, expected: RuntimeError) {
    assert_eq!(original(label, actual).unwrap_err(), expected);
}
fn exception(node: u32, message: Option<&str>) -> RuntimeError {
    RuntimeError::MappingException { node, message: message.map(str::to_owned) }
}
fn main() {
    let success = source(3, false, &[(10, 2, false, "unused"), (20, 2, false, "unused")]);
    let expected = group([field("Row", repeated([
        group([field("Result", scalar(Value::Float(5.0)))]),
        group([field("Result", scalar(Value::Float(10.0)))]),
    ]))]);
    assert_eq!(original("unselected failing message", raise_map::execute(&success)).unwrap(), expected);
    assert_eq!(original("empty collection", raise_map::execute(&source(3, false, &[]))).unwrap(), group([field("Row", repeated([]))]));
    failure("absent message", raise_map::execute(&source(1, false, &[(10, 0, true, "unused")])), exception(41, None));
    failure("explicit Null message", raise_map::execute(&source(2, false, &[(10, 0, true, "unused")])), exception(42, Some("")));
    failure("explicit empty string", raise_map::execute(&source(0, false, &[(10, 0, true, "")])), exception(40, Some("")));
    failure("selected message cause", raise_map::execute(&source(3, false, &[(10, 2, true, "unused")])), RuntimeError::Function(FunctionError::DivideByZero));
    failure("missing message context", raise_map::execute(&source(4, false, &[(10, 2, true, "unused")])), RuntimeError::MissingRuntimeValue { value: RuntimeValue::MappingFilePath });
    let execution = ExecutionContext::new(Path::new("active.ferrule"));
    failure("supplied message context", raise_map::execute_with_context(&source(4, false, &[(10, 2, true, "unused")]), &execution), exception(44, Some("active.ferrule")));
    failure("isolated function Raise", raise_map::execute(&source(5, false, &[(10, 2, true, "雪 udf")])), exception(8, Some("雪 udf")));
    failure("earlier target before later guard", raise_map::execute(&source(0, false, &[(10, 0, false, "earlier"), (20, 2, true, "late-selected")])), RuntimeError::Function(FunctionError::DivideByZero));
    failure("earlier guard before later target", raise_map::execute_outputs(&source(0, false, &[(10, 2, true, "first-selected"), (20, 0, false, "later")])), exception(40, Some("first-selected")));
    failure("late guard retains item message", raise_map::execute(&source(0, false, &[(10, 2, false, "unselected"), (20, 2, true, "late-selected")])), exception(40, Some("late-selected")));
    failure("legacy global rule precedes targets", raise_map::execute(&source(3, true, &[(10, 0, false, "global-first"), (20, 2, true, "late")])), RuntimeError::MappingFailure { rule: 1, message: Some("global-first".into()) });
}
"#;
