//! Direct Program retention controls, independent of project lowering.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use codegen::{
    DynamicSourceProgram, ExpressionNode, FailureIteration, FailureRule, FailureSelection,
    Json5BoundaryPolicyError, NamedSourceProgram, NamedTargetProgram, ScalarFunction,
    SourceIteration, UserFunctionParameter,
};
use ir::SchemaNode;

use super::*;

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
    }
}

fn field(id: u32, path: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::SourceField {
            frame: None,
            path: path.iter().map(|part| (*part).to_owned()).collect(),
        },
    }
}

fn binding(name: &str, expression: u32, ty: ScalarType) -> Binding {
    Binding {
        target_field: name.into(),
        expression,
        target_domain: ScalarTargetDomain::Single(ty),
        repeating: false,
    }
}

fn root(bindings: Vec<Binding>) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings,
        children: Vec::new(),
    }
}

fn witness() -> Program {
    Program {
        xml_boundary: None,
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Unused", ScalarType::String)],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Answer", ScalarType::Int)],
        ),
        expressions: vec![field(1, &["Unused"]), constant(2, Value::Int(7))],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: root(vec![binding("Answer", 2, ScalarType::Int)]),
        extra_targets: Vec::new(),
    }
}

fn options(name: &str) -> Options {
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    Options {
        package_name: name.into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime.canonicalize().unwrap().display().to_string(),
        ),
    }
}

fn library(artifacts: &ArtifactSet) -> &str {
    let file = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .unwrap();
    std::str::from_utf8(&file.contents).unwrap()
}

#[test]
fn direct_program_unused_source_field_builds_warning_free_in_ordinary_and_json5_profiles() {
    let program = witness();
    let original = program.clone();
    let mut evidence = Evidence::new("direct_program_witness");
    let ordinary = emit(&program, &options("direct-retention-map"));
    let selected = emit_with_json5(&program, &options("direct-retention-map"));
    evidence.write(
        "ORIGINAL_EMISSION_RESULTS.debug.txt",
        format!("ordinary={ordinary:#?}\nselected={selected:#?}\n"),
    );
    let ordinary = ordinary.unwrap();
    let selected = selected.unwrap();
    // Compile first so the baseline retains the actual dead-code diagnostic.
    exercise(&mut evidence, "ordinary", &ordinary, ORDINARY_HOST);
    exercise(&mut evidence, "json5", &selected, JSON5_HOST);
    assert!(!library(&ordinary).contains("fn expression_1("));
    assert!(library(&ordinary).contains("fn expression_2("));
    assert!(library(&selected).starts_with(library(&ordinary)));
    assert_eq!(ordinary.files()[0].contents, selected.files()[0].contents);
    assert_eq!(program, original);
}

#[test]
fn unused_invalid_expressions_keep_full_validation_and_package_error_precedence() {
    let cases = [
        (
            Expression::Call {
                function: ScalarFunction::Add,
                args: vec![99, 2],
            },
            ProgramValidationError::MissingDependency {
                node: 1,
                dependency: 99,
            },
        ),
        (
            Expression::FunctionParameter {
                parameter: FunctionParameterId::new(7),
            },
            ProgramValidationError::FunctionParameterInMain {
                node: 1,
                parameter: FunctionParameterId::new(7),
            },
        ),
        (
            Expression::UserFunctionCall {
                function: FunctionId::new(9),
                args: Vec::new(),
            },
            ProgramValidationError::MissingUserFunction {
                owner: None,
                node: 1,
                function: FunctionId::new(9),
            },
        ),
        (
            Expression::Aggregate {
                function: AggregateFunction::Count,
                collection: vec!["Missing".into()],
                value: AggregateValue::Path(vec!["value".into()]),
                arg: None,
            },
            ProgramValidationError::InvalidAggregateCollection {
                node: 1,
                collection: vec!["Missing".into()],
            },
        ),
    ];
    let evidence = Evidence::new("direct_program_invalid");
    for (index, (expression, expected)) in cases.into_iter().enumerate() {
        let mut program = witness();
        program.expressions[0].expression = expression;
        let original = program.clone();
        let ordinary = emit(&program, &options("9-invalid"));
        let selected = emit_with_json5(&program, &options("9-invalid"));
        evidence.write(
            &format!("CASE_{index}_ORIGINAL_RESULTS.debug.txt"),
            format!("program={program:#?}\nordinary={ordinary:#?}\nselected={selected:#?}\n"),
        );
        assert!(matches!(ordinary, Err(EmitError::InvalidProgram(actual)) if actual == expected));
        assert!(
            matches!(selected, Err(Json5EmitError::Policy(Json5BoundaryPolicyError::Validation(actual))) if actual == expected)
        );
        assert_eq!(program, original);
    }
    let valid = witness();
    let ordinary = emit(&valid, &options("9-invalid"));
    let selected = emit_with_json5(&valid, &options("9-invalid"));
    evidence.write(
        "VALID_PROGRAM_BAD_PACKAGE_ORIGINAL_RESULTS.debug.txt",
        format!("ordinary={ordinary:#?}\nselected={selected:#?}\n"),
    );
    assert!(matches!(ordinary, Err(EmitError::InvalidPackageName(name)) if name == "9-invalid"));
    assert!(
        matches!(selected, Err(Json5EmitError::Ordinary(EmitError::InvalidPackageName(name))) if name == "9-invalid")
    );
}

fn roots_program() -> Program {
    let mut program = witness();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Unused", ScalarType::String),
            SchemaNode::scalar("Input", ScalarType::Int),
            SchemaNode::scalar("Fail", ScalarType::Bool),
            SchemaNode::group(
                "Files",
                vec![SchemaNode::scalar("path", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let loaded = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    program.extra_sources = vec![
        NamedSourceProgram {
            name: "Settings".into(),
            source: SchemaNode::group(
                "SettingsDocument",
                vec![SchemaNode::scalar("Value", ScalarType::Int)],
            ),
            dynamic: None,
        },
        NamedSourceProgram {
            name: "Live".into(),
            source: loaded.clone(),
            dynamic: Some(DynamicSourceProgram {
                path: 10,
                driver: SourceIteration::new(vec!["Files".into()]),
            }),
        },
        NamedSourceProgram {
            name: "Unused".into(),
            source: loaded,
            dynamic: Some(DynamicSourceProgram {
                path: 20,
                driver: SourceIteration::new(vec!["Files".into()]),
            }),
        },
    ];
    program.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("Result", ScalarType::Int),
            SchemaNode::group("Sequence", vec![SchemaNode::scalar("n", ScalarType::Int)])
                .repeating(),
            SchemaNode::group(
                "Loaded",
                vec![SchemaNode::scalar("value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    program.expressions = vec![
        field(1, &["Unused"]),
        field(2, &["Input"]),
        constant(3, Value::Int(1)),
        constant(4, Value::Int(1)),
        constant(5, Value::Int(2)),
        field(6, &[]),
        ExpressionNode {
            id: 8,
            expression: Expression::Call {
                function: ScalarFunction::Add,
                args: vec![2, 3],
            },
        },
        field(9, &["path"]),
        ExpressionNode {
            id: 10,
            expression: Expression::If {
                condition: 18,
                then: 9,
                else_: 19,
            },
        },
        field(11, &["Fail"]),
        constant(12, Value::String("stop".into())),
        field(13, &["Settings", "Value"]),
        field(14, &["value"]),
        ExpressionNode {
            id: 16,
            expression: Expression::UserFunctionCall {
                function: FunctionId::new(50),
                args: vec![2],
            },
        },
        constant(18, Value::Bool(true)),
        constant(19, Value::String("fallback.json".into())),
        constant(20, Value::String("unused.json".into())),
        field(30, &[]),
        ExpressionNode {
            id: 35,
            expression: Expression::SequenceExists {
                sequence: GeneratedSequence::Range {
                    from: Some(4),
                    to: 5,
                    item: 36,
                },
                predicate: 37,
            },
        },
        field(36, &[]),
        constant(37, Value::Bool(true)),
    ];
    program.user_functions = vec![UserFunctionProgram {
        id: FunctionId::new(50),
        library: "self".into(),
        name: "increment".into(),
        parameters: vec![UserFunctionParameter {
            id: FunctionParameterId::new(1),
            ty: ScalarType::Int,
        }],
        output_type: ScalarType::Int,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::FunctionParameter {
                    parameter: FunctionParameterId::new(1),
                },
            },
            constant(2, Value::Int(1)),
            ExpressionNode {
                id: 3,
                expression: Expression::Call {
                    function: ScalarFunction::Add,
                    args: vec![1, 2],
                },
            },
        ],
        output: 3,
    }];
    program.failure_rules = vec![FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::Range {
            from: Some(4),
            to: 5,
            item: 30,
        }),
        selection: FailureSelection::WhenTrue(11),
        message: Some(12),
    }];
    program.root = root(vec![binding("Result", 8, ScalarType::Int)]);
    let mut sequence = root(vec![binding("n", 4, ScalarType::Int)]);
    sequence.target_field = "Sequence".into();
    sequence.repeating = true;
    sequence.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: Some(4),
        to: 5,
        item: 6,
    }));
    let mut dynamic = root(vec![binding("value", 14, ScalarType::String)]);
    dynamic.target_field = "Loaded".into();
    dynamic.repeating = true;
    dynamic.iteration = Some(IterationPlan::source(vec!["Live".into(), "Rows".into()]));
    program.root.children = vec![sequence, dynamic];
    program.extra_targets = vec![NamedTargetProgram {
        name: "Audit".into(),
        target: SchemaNode::group(
            "AuditDocument",
            vec![
                SchemaNode::scalar("Static", ScalarType::Int),
                SchemaNode::scalar("Function", ScalarType::Int),
            ],
        ),
        root: root(vec![
            binding("Static", 13, ScalarType::Int),
            binding("Function", 16, ScalarType::Int),
        ]),
    }];
    program
}

#[test]
fn direct_program_retains_dependencies_named_outputs_failures_items_and_dynamic_paths() {
    let program = roots_program();
    let original = program.clone();
    let mut evidence = Evidence::new("direct_program_roots");
    let emitted = emit(&program, &options("direct-roots-map"));
    evidence.write(
        "ORIGINAL_EMISSION_RESULT.debug.txt",
        format!("program={program:#?}\nresult={emitted:#?}\n"),
    );
    let artifacts = emitted.unwrap();
    exercise(&mut evidence, "roots", &artifacts, ROOTS_HOST);
    let source = library(&artifacts);
    for id in [1, 20, 35, 36, 37] {
        assert!(
            !source.contains(&format!("fn expression_{id}(")),
            "unused expression {id}"
        );
    }
    for id in [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 16, 18, 19, 30] {
        assert!(
            source.contains(&format!("fn expression_{id}(")),
            "retained expression {id}"
        );
    }
    for id in [6, 30] {
        assert!(source.contains(&format!("#[allow(dead_code)]\nfn expression_{id}(")));
    }
    assert_eq!(source.matches("#[allow(dead_code)]").count(), 2);
    // The same live path graph remains reachable with just one declaration.
    let mut single_dynamic = program.clone();
    single_dynamic.extra_sources.pop();
    let single_original = single_dynamic.clone();
    let single = emit(&single_dynamic, &options("direct-roots-map"));
    evidence.write(
        "SINGLE_DYNAMIC_ORIGINAL_EMISSION_RESULT.debug.txt",
        format!("program={single_dynamic:#?}\nresult={single:#?}\n"),
    );
    let single = single.unwrap();
    for id in [1, 20, 35, 36, 37] {
        assert!(!library(&single).contains(&format!("fn expression_{id}(")));
    }
    for id in [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 16, 18, 19, 30] {
        assert!(library(&single).contains(&format!("fn expression_{id}(")));
    }
    assert_eq!(single_dynamic, single_original);
    assert_eq!(program, original);
}

struct Evidence {
    directory: PathBuf,
    preserve: bool,
}

impl Evidence {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("ferrule_{tag}_{}_{}", std::process::id(), nonce));
        fs::create_dir(&directory).unwrap();
        eprintln!("direct Program originals: {}", directory.display());
        Self {
            directory,
            preserve: false,
        }
    }
    fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        fs::write(self.directory.join(name), contents).unwrap();
    }
}

impl Drop for Evidence {
    fn drop(&mut self) {
        if self.preserve
            || std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!(
                "retained direct Program originals: {}",
                self.directory.display()
            );
        } else {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

// Resolve existing ancestors without creating/copying a shared build cache.
fn identity(path: &Path) -> PathBuf {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(ancestor.file_name().unwrap());
        ancestor = ancestor.parent().unwrap();
    }
    let mut result = ancestor.canonicalize().unwrap();
    for part in suffix.into_iter().rev() {
        result.push(part);
    }
    result
}

fn exercise(evidence: &mut Evidence, label: &str, artifacts: &ArtifactSet, host: &str) {
    let directory = evidence.directory.join(label);
    fs::create_dir(&directory).unwrap();
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    fs::write(directory.join("src/main.rs"), host).unwrap();
    let cwd = std::env::current_dir().unwrap();
    let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(value) => {
            assert!(!value.is_empty(), "host target must be nonempty");
            let path = PathBuf::from(value);
            identity(&if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            })
        }
        None => evidence.directory.join("host-target"),
    };
    if let Some(outer) = std::env::var_os("CARGO_TARGET_DIR") {
        let outer = PathBuf::from(outer);
        assert_ne!(
            identity(&if outer.is_absolute() {
                outer
            } else {
                cwd.join(outer)
            }),
            target
        );
    }
    assert!(
        !std::env::current_exe()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(&target)
    );
    // This leaf's hosts are serial; external shared-cache users need one lease.
    // The outer test supervisor owns the process deadline and disk headroom.
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet", "--offline", "--jobs", "1"])
        .current_dir(&directory)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings");
    fs::write(
        directory.join("ORIGINAL_COMMAND.debug.txt"),
        format!("{command:#?}\n"),
    )
    .unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let launched = command.output();
        fs::write(
            directory.join("ORIGINAL_PROCESS_RESULT.debug.txt"),
            format!("{launched:#?}\n"),
        )
        .unwrap();
        if let Ok(output) = &launched {
            fs::write(directory.join("ORIGINAL_STDOUT.bin"), &output.stdout).unwrap();
            fs::write(directory.join("ORIGINAL_STDERR.bin"), &output.stderr).unwrap();
        }
        let output = launched.unwrap();
        assert!(
            output.status.success(),
            "host {label} failed; originals: {}",
            directory.display()
        );
    }));
    let after = artifacts
        .files()
        .iter()
        .map(|file| {
            (
                file.path.as_str(),
                fs::read(directory.join(file.path.as_str())),
            )
        })
        .collect::<Vec<_>>();
    let host_after = fs::read(directory.join("src/main.rs"));
    fs::write(
        directory.join("COMPLETE_SOURCE_AND_HOST_AFTER.debug.txt"),
        format!("artifacts={after:#?}\nhost={host_after:#?}\n"),
    )
    .unwrap();
    if let Err(payload) = &outcome {
        evidence.preserve = true;
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|value| (*value).into()))
            .unwrap_or_else(|| "opaque panic payload".into());
        fs::write(directory.join("ORIGINAL_ASSERTION_UNWIND.txt"), message).unwrap();
    }
    for (file, (_, actual)) in artifacts.files().iter().zip(after) {
        assert_eq!(actual.unwrap(), file.contents);
    }
    assert_eq!(host_after.unwrap(), host.as_bytes());
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

const ORDINARY_HOST: &str = r#"use codegen_runtime::{field, group, scalar, Value};
fn main() {
    let source = group([field("Unused", scalar(Value::String("ignored".into())))]);
    let actual = direct_retention_map::execute(&source);
    let text = direct_retention_map::execute_json("{\"Unused\":\"ignored\"}");
    let bytes = direct_retention_map::execute_json_bytes(b"{\"Unused\":\"ignored\"}");
    std::fs::write("ORIGINAL_TYPED_AND_JSON_RESULTS.debug.txt", format!("typed={actual:#?}\ntext={text:#?}\nbytes={bytes:#?}\n")).unwrap();
    assert_eq!(actual.unwrap(), group([field("Answer", scalar(Value::Int(7)))]));
    assert_eq!(text.unwrap(), "{\n  \"Answer\": 7\n}\n");
    assert_eq!(bytes.unwrap(), b"{\n  \"Answer\": 7\n}\n");
}
"#;

const JSON5_HOST: &str = r#"use codegen_runtime::{field, group, scalar, ExecutionContext, Value};
fn main() {
    let source = group([field("Unused", scalar(Value::String("ignored".into())))]);
    let context = ExecutionContext::new(std::path::Path::new("direct.map"));
    let typed = direct_retention_map::execute(&source);
    let strict = direct_retention_map::execute_json("{\"Unused\":\"ignored\"}");
    let text = direct_retention_map::execute_json5("{Unused:'ignored',}");
    let bytes = direct_retention_map::execute_json5_bytes(b"{Unused:'ignored',}");
    let text_context = direct_retention_map::execute_json5_with_context("{Unused:'ignored',}", &context);
    let bytes_context = direct_retention_map::execute_json5_bytes_with_context(b"{Unused:'ignored',}", &context);
    std::fs::write("ORIGINAL_TYPED_STRICT_AND_JSON5_RESULTS.debug.txt", format!("typed={typed:#?}\nstrict={strict:#?}\ntext={text:#?}\nbytes={bytes:#?}\ntext_context={text_context:#?}\nbytes_context={bytes_context:#?}\n")).unwrap();
    assert_eq!(typed.unwrap(), group([field("Answer", scalar(Value::Int(7)))]));
    let expected = "{\n  \"Answer\": 7\n}\n";
    assert_eq!(strict.unwrap(), expected);
    assert_eq!(text.unwrap(), expected);
    assert_eq!(bytes.unwrap(), expected.as_bytes());
    assert_eq!(text_context.unwrap(), expected);
    assert_eq!(bytes_context.unwrap(), expected.as_bytes());
}
"#;

const ROOTS_HOST: &str = r#"use std::cell::RefCell;
use codegen_runtime::{field, group, repeated, scalar, DynamicSourceLoader, Instance, NamedInput, RuntimeError, Value};
struct Loader { calls: RefCell<Vec<(String, String)>> }
impl DynamicSourceLoader for Loader {
    fn load(&self, name: &str, path: &str) -> Result<Instance, String> {
        self.calls.borrow_mut().push((name.into(), path.into()));
        Ok(group([field("Rows", repeated([group([field("value", scalar(Value::String("seen".into())))])]))]))
    }
}
fn source(fail: bool) -> Instance {
    group([
        field("Unused", scalar(Value::String("ignored".into()))),
        field("Input", scalar(Value::Int(7))), field("Fail", scalar(Value::Bool(fail))),
        field("Files", repeated([group([field("path", scalar(Value::String("one.json".into())))])])),
    ])
}
fn main() {
    let settings = group([field("Value", scalar(Value::Int(9)))]);
    let inputs = [NamedInput { name: "Settings", instance: &settings }];
    let loader = Loader { calls: RefCell::new(Vec::new()) };
    let actual = direct_roots_map::execute_outputs_with_sources_and_dynamic_source_loader(&source(false), &inputs, &loader);
    let failed = direct_roots_map::execute_outputs_with_sources_and_dynamic_source_loader(&source(true), &inputs, &loader);
    std::fs::write("ORIGINAL_COMPLETE_OUTPUT_FAILURE_AND_LOADS.debug.txt", format!("actual={actual:#?}\nfailed={failed:#?}\nloads={:#?}\n", loader.calls.borrow())).unwrap();
    let actual = actual.unwrap();
    assert_eq!(actual.primary, group([
        field("Result", scalar(Value::Int(8))),
        field("Sequence", repeated([
            group([field("n", scalar(Value::Int(1)))]), group([field("n", scalar(Value::Int(1)))]),
        ])),
        field("Loaded", repeated([group([field("value", scalar(Value::String("seen".into())))])])),
    ]));
    assert_eq!(actual.extras.len(), 1);
    assert_eq!(actual.extras[0].name, "Audit");
    assert_eq!(actual.extras[0].instance, group([field("Static", scalar(Value::Int(9))), field("Function", scalar(Value::Int(8)))]));
    assert_eq!(failed, Err(RuntimeError::MappingFailure { rule: 1, message: Some("stop".into()) }));
    assert_eq!(*loader.calls.borrow(), vec![("Live".into(), "one.json".into())]);
}
"#;
