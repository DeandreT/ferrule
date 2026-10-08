use std::error::Error;

use super::*;

fn required(mut schema: SchemaNode, names: &[&str]) -> SchemaNode {
    let SchemaKind::Group { required, .. } = &mut schema.kind else {
        panic!("test group expected");
    };
    *required = names.iter().map(|name| (*name).to_owned()).collect();
    schema
}

fn node(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}
fn constant(id: u32, value: Value) -> ExpressionNode {
    node(id, Expression::Const { value })
}
fn source(id: u32, name: &str) -> ExpressionNode {
    node(
        id,
        Expression::SourceField {
            frame: None,
            path: vec![name.into()],
        },
    )
}
fn equal(id: u32, left: u32, right: u32) -> ExpressionNode {
    node(
        id,
        Expression::Call {
            function: ScalarFunction::Equal,
            args: vec![left, right],
        },
    )
}
fn choice(id: u32, condition: u32, then: u32, else_: u32) -> ExpressionNode {
    node(
        id,
        Expression::If {
            condition,
            then,
            else_,
        },
    )
}
fn binding(name: &str, id: u32, ty: ScalarType) -> Binding {
    Binding {
        target_field: name.into(),
        expression: id,
        target_domain: ScalarTargetDomain::Single(ty),
        repeating: false,
    }
}

fn prototype() -> Program {
    let mut nil = SchemaNode::scalar("Nil", ScalarType::String);
    nil.nullable = true;
    Program {
        xml_boundary: None,
        source: required(
            SchemaNode::group(
                "Source",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Fail", ScalarType::Bool),
                    SchemaNode::scalar("Mode", ScalarType::Int),
                    SchemaNode::scalar("Numeric", ScalarType::Float),
                    required(
                        SchemaNode::group("Info", vec![SchemaNode::scalar("n", ScalarType::Int)]),
                        &["n"],
                    ),
                    nil.clone(),
                ],
            ),
            &["Name", "Fail", "Mode", "Numeric", "Info"],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Copied", ScalarType::String),
                SchemaNode::scalar("Selected", ScalarType::String),
                SchemaNode::scalar("Number", ScalarType::Float),
                nil,
                SchemaNode::group(
                    "Nested",
                    vec![SchemaNode::scalar("constant", ScalarType::String)],
                ),
            ],
        ),
        expressions: vec![
            source(1, "Name"),
            source(2, "Fail"),
            source(3, "Mode"),
            constant(4, Value::Int(1)),
            equal(5, 3, 4),
            node(
                6,
                Expression::RuntimeParameter {
                    name: "label".into(),
                    ty: ScalarType::String,
                },
            ),
            choice(7, 5, 6, 1),
            constant(8, Value::Null),
            node(9, Expression::Raise { message: Some(8) }),
            choice(10, 2, 33, 7),
            source(11, "Numeric"),
            node(
                12,
                Expression::RuntimeValue {
                    value: RuntimeValue::MappingFilePath,
                },
            ),
            constant(13, Value::Int(2)),
            equal(14, 3, 13),
            choice(15, 14, 12, 10),
            node(16, Expression::Raise { message: None }),
            constant(17, Value::String(String::new())),
            node(18, Expression::Raise { message: Some(17) }),
            constant(19, Value::String("selected".into())),
            node(20, Expression::Raise { message: Some(19) }),
            constant(21, Value::Int(3)),
            equal(22, 3, 21),
            choice(23, 22, 16, 9),
            constant(24, Value::Int(4)),
            equal(25, 3, 24),
            choice(26, 25, 18, 23),
            constant(27, Value::Int(5)),
            equal(28, 3, 27),
            choice(29, 28, 20, 26),
            constant(30, Value::Int(0)),
            node(
                31,
                Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![4, 30],
                },
            ),
            node(32, Expression::Raise { message: Some(31) }),
            constant(34, Value::Int(6)),
            equal(36, 3, 34),
            choice(33, 36, 32, 29),
            source(37, "Nil"),
            constant(38, Value::String("nested".into())),
            constant(40, Value::Int(8)),
            equal(41, 3, 40),
            constant(42, Value::String("global".into())),
            constant(44, Value::Int(9)),
            equal(45, 3, 44),
            choice(46, 45, 49, 15),
            node(
                49,
                Expression::UserFunctionCall {
                    function: FunctionId::new(50),
                    args: Vec::new(),
                },
            ),
        ],
        user_functions: vec![UserFunctionProgram {
            id: FunctionId::new(50),
            library: "self".into(),
            name: "constant".into(),
            parameters: Vec::new(),
            output_type: ScalarType::String,
            expressions: vec![constant(1, Value::String("from function".into()))],
            output: 1,
        }],
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
            selection: FailureSelection::WhenTrue(41),
            message: Some(42),
        }],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![
                binding("Copied", 1, ScalarType::String),
                binding("Selected", 46, ScalarType::String),
                binding("Number", 11, ScalarType::Float),
                binding("Nil", 37, ScalarType::String),
            ],
            children: vec![TargetScope {
                target_field: "Nested".into(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![binding("constant", 38, ScalarType::String)],
                children: Vec::new(),
            }],
        },
        extra_targets: Vec::new(),
    }
}

fn options(package: &str) -> Options {
    Options {
        package_name: package.into(),
        runtime_dependency: RuntimeDependency::Path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("codegen-runtime")
                .display()
                .to_string(),
        ),
    }
}

#[test]
fn explicit_json5_selection_preserves_complete_ordinary_artifacts_manifest_and_source_prefix() {
    let program = prototype();
    let before_program = program.clone();
    let options = options("json5-map");
    let before = emit(&program, &options);
    eprintln!("original ordinary before={before:#?}");
    let selected = emit_with_json5(&program, &options);
    eprintln!("original selected={selected:#?}");
    let after = emit(&program, &options);
    eprintln!("original ordinary after={after:#?}");
    let before = before.unwrap();
    let selected = selected.unwrap();
    assert_eq!(after.unwrap(), before);
    assert_eq!(program, before_program);
    assert_eq!(before.files().len(), 2);
    assert_eq!(selected.files().len(), 2);
    for ordinary in before.files() {
        let selected = selected
            .files()
            .iter()
            .find(|file| file.path == ordinary.path)
            .unwrap();
        if ordinary.path.as_str() == "src/lib.rs" {
            assert!(selected.contents.starts_with(&ordinary.contents));
            let suffix =
                std::str::from_utf8(&selected.contents[ordinary.contents.len()..]).unwrap();
            for name in [
                "execute_json5(",
                "execute_json5_with_context(",
                "execute_json5_bytes(",
                "execute_json5_bytes_with_context(",
            ] {
                assert_eq!(suffix.matches(&format!("pub fn {name}")).count(), 1);
            }
            assert!(!suffix.contains("with_sources"));
            assert!(!suffix.contains("dynamic_source"));
            assert!(!String::from_utf8_lossy(&ordinary.contents).contains("pub fn execute_json5"));
        } else {
            assert_eq!(selected, ordinary);
        }
    }
}

#[test]
fn opt_in_policy_refuses_before_publication_and_keeps_ordinary_emitter_causes() {
    for source_side in [true, false] {
        let mut program = prototype();
        if source_side {
            program.source.repeating = true;
        } else {
            program.target.nullable = true;
        }
        let result = emit_with_json5(&program, &options("json5-map"));
        eprintln!("original refused schema={result:#?}");
        let error = result.unwrap_err();
        assert!(error.source().is_some());
        assert!(
            matches!(error, Json5EmitError::Policy(codegen::Json5BoundaryPolicyError::Schema { side, .. }) if side == if source_side { codegen::Json5BoundarySide::Source } else { codegen::Json5BoundarySide::Target })
        );
    }
    let mut extra = prototype();
    extra.extra_sources.push(NamedSourceProgram {
        name: "Other".into(),
        source: extra.source.clone(),
        dynamic: None,
    });
    let result = emit_with_json5(&extra, &options("json5-map"));
    eprintln!("original named-source refusal={result:#?}");
    assert!(matches!(
        result,
        Err(Json5EmitError::Policy(
            codegen::Json5BoundaryPolicyError::ProgramField {
                field: "extra_sources"
            }
        ))
    ));
    let result = emit_with_json5(&prototype(), &options("9-invalid"));
    eprintln!("original ordinary cause={result:#?}");
    let error = result.unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<EmitError>()
            .is_some()
    );
    assert!(
        matches!(error, Json5EmitError::Ordinary(EmitError::InvalidPackageName(name)) if name == "9-invalid")
    );
}

#[test]
fn compiled_selected_prototype_exercises_four_public_methods_typed_causes_lazy_messages_and_context()
 {
    let output = TempDir::new("rust_json5_companions");
    let directory = output.path().to_path_buf();
    std::mem::forget(output);
    let result = emit_with_json5(&prototype(), &options("json5-map"));
    eprintln!("original compile artifacts={result:#?}");
    let artifacts = result.unwrap();
    write_artifacts(&directory, &artifacts);
    fs::write(directory.join("src/main.rs"), HOST).unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let launched = Command::new("cargo")
            .args(["run", "--quiet", "--offline"])
            .current_dir(&directory)
            .generated_host_output(&directory);
        fs::write(
            directory.join("HOST_FULL_RESULT.debug.txt"),
            format!("{launched:#?}\n"),
        )
        .unwrap();
        eprintln!("original generated JSON5 host={launched:#?}");
        let original = launched.unwrap();
        fs::write(directory.join("HOST_STDOUT.bin"), &original.stdout).unwrap();
        fs::write(directory.join("HOST_STDERR.bin"), &original.stderr).unwrap();
        assert!(
            original.status.success(),
            "selected prototype failed; full originals at {}",
            directory.display()
        );
    }));
    // Verify whole generation inputs even when the host assertion unwinds.
    let observed = artifacts
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
        directory.join("WHOLE_SOURCE_AND_HOST_AFTER.debug.txt"),
        format!("artifacts={observed:#?}\nhost={host_after:#?}\n"),
    )
    .unwrap();
    if let Err(payload) = &outcome {
        fs::write(
            directory.join("ORIGINAL_ASSERTION_UNWIND.txt"),
            if let Some(text) = payload.downcast_ref::<String>() {
                text.clone()
            } else if let Some(text) = payload.downcast_ref::<&str>() {
                (*text).to_owned()
            } else {
                "opaque original panic payload; type not fabricated".into()
            },
        )
        .unwrap();
    }
    for (file, (_, observed)) in artifacts.files().iter().zip(observed) {
        assert_eq!(observed.unwrap(), file.contents);
    }
    assert_eq!(host_after.unwrap(), HOST.as_bytes());
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

const HOST: &str = include_str!("json5/host.rs");
