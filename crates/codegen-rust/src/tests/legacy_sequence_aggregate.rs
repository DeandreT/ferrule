use super::*;
use codegen::{AggregateFunction, ScalarTargetDomain};
use serde_json::Value as Json;

const CASES: &str = include_str!("legacy_sequence_aggregate/cases.json");
const HOST: &str = include_str!("legacy_sequence_aggregate/host.rs.txt");

fn expression(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}

fn constant(id: u32, value: Value) -> ExpressionNode {
    expression(id, Expression::Const { value })
}

fn program(case: &Json) -> Program {
    let mut expressions = vec![
        constant(1, Value::Int(1)),
        constant(2, Value::Int(3)),
        expression(
            10,
            Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        ),
    ];
    let predicate = match case["predicate_kind"].as_str().unwrap() {
        "greater_than_one" => {
            expressions.push(constant(3, Value::Int(1)));
            Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![10, 3],
            }
        }
        "false" => Expression::Const {
            value: Value::Bool(false),
        },
        "int" => Expression::Const {
            value: Value::Int(9),
        },
        "float" => Expression::Const {
            value: Value::Float(f64::from_bits(0x3ff4000000000000)),
        },
        "string" => Expression::Const {
            value: Value::String("bad λ".into()),
        },
        "null" => Expression::Const { value: Value::Null },
        "raise" => {
            expressions.push(constant(13, Value::String("predicate λ".into())));
            Expression::Raise { message: Some(13) }
        }
        other => panic!("unknown frozen predicate {other}"),
    };
    expressions.push(expression(4, predicate));
    let value_expression = match case["expression_kind"].as_str().unwrap() {
        "item" => None,
        "position" => {
            expressions.push(expression(
                5,
                Expression::Position {
                    collection: Vec::new(),
                },
            ));
            Some(5)
        }
        "lazy_raise" => {
            // The value for item one must never execute after its predicate is false.
            if !expressions.iter().any(|node| node.id == 3) {
                expressions.push(constant(3, Value::Int(1)));
            }
            expressions.extend([
                constant(6, Value::String("expression λ".into())),
                expression(7, Expression::Raise { message: Some(6) }),
                expression(
                    8,
                    Expression::Call {
                        function: ScalarFunction::Equal,
                        args: vec![10, 3],
                    },
                ),
                expression(
                    9,
                    Expression::If {
                        condition: 8,
                        then: 7,
                        else_: 10,
                    },
                ),
            ]);
            Some(9)
        }
        other => panic!("unknown frozen expression {other}"),
    };
    let argument = match case["argument_kind"].as_str() {
        None => None,
        Some("comma") => {
            expressions.push(constant(11, Value::String(",".into())));
            Some(11)
        }
        Some("first") => {
            expressions.push(constant(12, Value::Int(1)));
            Some(12)
        }
        Some("raise") => Some(7),
        Some(other) => panic!("unknown frozen argument {other}"),
    };
    let function = match case["aggregate"].as_str().unwrap() {
        "Count" => AggregateFunction::Count,
        "Sum" => AggregateFunction::Sum,
        "Avg" => AggregateFunction::Avg,
        "Min" => AggregateFunction::Min,
        "Max" => AggregateFunction::Max,
        "Join" => AggregateFunction::Join,
        "ItemAt" => AggregateFunction::ItemAt,
        other => panic!("unknown frozen aggregate {other}"),
    };
    expressions.push(expression(
        50,
        Expression::SequenceAggregate {
            function,
            sequence: GeneratedSequence::Range {
                from: Some(1),
                to: 2,
                item: 10,
            },
            predicate: Some(4),
            expression: value_expression,
            arg: argument,
        },
    ));
    expressions.sort_by_key(|node| node.id);
    let ty = match case["aggregate"].as_str().unwrap() {
        "Avg" => ScalarType::Float,
        "Join" => ScalarType::String,
        _ => ScalarType::Int,
    };
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", Vec::new()),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Result", ty)]),
        expressions,
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Result".into(),
                expression: 50,
                target_domain: ScalarTargetDomain::Single(ty),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

// The native graph is authored independently of the generated Program constructor.
// Both routes compare the same pre-existing literals; neither result is an oracle.
fn native_project(case: &Json) -> mapping::Project {
    use mapping::{AggregateOp, Node, SequenceExpr};
    let mut nodes = vec![
        (
            1,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            10,
            Node::SourceField {
                frame: None,
                path: Vec::new(),
            },
        ),
    ];
    let predicate = match case["predicate_kind"].as_str().unwrap() {
        "greater_than_one" => {
            nodes.push((
                3,
                Node::Const {
                    value: Value::Int(1),
                },
            ));
            Node::Call {
                function: "greater_than".into(),
                args: vec![10, 3],
            }
        }
        "false" => Node::Const {
            value: Value::Bool(false),
        },
        "int" => Node::Const {
            value: Value::Int(9),
        },
        "float" => Node::Const {
            value: Value::Float(f64::from_bits(0x3ff4000000000000)),
        },
        "string" => Node::Const {
            value: Value::String("bad λ".into()),
        },
        "null" => Node::Const { value: Value::Null },
        "raise" => {
            nodes.push((
                13,
                Node::Const {
                    value: Value::String("predicate λ".into()),
                },
            ));
            Node::Raise { message: Some(13) }
        }
        other => panic!("unknown frozen native predicate {other}"),
    };
    nodes.push((4, predicate));
    let value_expression = match case["expression_kind"].as_str().unwrap() {
        "item" => None,
        "position" => {
            nodes.push((
                5,
                Node::Position {
                    collection: Vec::new(),
                },
            ));
            Some(5)
        }
        "lazy_raise" => {
            if !nodes.iter().any(|(id, _)| *id == 3) {
                nodes.push((
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ));
            }
            nodes.extend([
                (
                    6,
                    Node::Const {
                        value: Value::String("expression λ".into()),
                    },
                ),
                (7, Node::Raise { message: Some(6) }),
                (
                    8,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![10, 3],
                    },
                ),
                (
                    9,
                    Node::If {
                        condition: 8,
                        then: 7,
                        else_: 10,
                    },
                ),
            ]);
            Some(9)
        }
        other => panic!("unknown frozen native expression {other}"),
    };
    let arg = match case["argument_kind"].as_str() {
        None => None,
        Some("comma") => {
            nodes.push((
                11,
                Node::Const {
                    value: Value::String(",".into()),
                },
            ));
            Some(11)
        }
        Some("first") => {
            nodes.push((
                12,
                Node::Const {
                    value: Value::Int(1),
                },
            ));
            Some(12)
        }
        Some("raise") => Some(7),
        Some(other) => panic!("unknown frozen native argument {other}"),
    };
    let function = match case["aggregate"].as_str().unwrap() {
        "Count" => AggregateOp::Count,
        "Sum" => AggregateOp::Sum,
        "Avg" => AggregateOp::Avg,
        "Min" => AggregateOp::Min,
        "Max" => AggregateOp::Max,
        "Join" => AggregateOp::Join,
        "ItemAt" => AggregateOp::ItemAt,
        other => panic!("unknown frozen native aggregate {other}"),
    };
    nodes.push((
        50,
        Node::SequenceAggregate {
            function,
            sequence: SequenceExpr::Generate {
                from: Some(1),
                to: 2,
                item: 10,
            },
            predicate: Some(4),
            expression: value_expression,
            arg,
        },
    ));
    let ty = match case["aggregate"].as_str().unwrap() {
        "Avg" => ScalarType::Float,
        "Join" => ScalarType::String,
        _ => ScalarType::Int,
    };
    mapping::Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Result", ty)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: mapping::Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Result".into(),
                node: 50,
            }],
            ..Default::default()
        },
    }
}

fn native_value_original(value: &Value, path: &str, text: &mut String) {
    use std::fmt::Write as _;
    writeln!(text, "path={path:?};complete-value={value:#?}").unwrap();
    if let Value::Float(value) = value {
        writeln!(text, "path={path:?};binary64={:016x}", value.to_bits()).unwrap();
    }
}

fn native_instance_original(
    value: &ir::Instance,
    path: &str,
    text: &mut String,
    floats: &mut Vec<(String, u64)>,
) {
    use ir::Instance;
    use std::fmt::Write as _;
    writeln!(text, "path={path:?};complete-instance={value:#?}").unwrap();
    match value {
        Instance::Scalar(value) => {
            native_value_original(value, path, text);
            if let Value::Float(value) = value {
                floats.push((path.into(), value.to_bits()));
            }
        }
        Instance::Group(fields) => {
            writeln!(
                text,
                "path={path:?};complete-origin={:?}",
                fields.xml_type_origin()
            )
            .unwrap();
            for (index, (name, child)) in fields.iter().enumerate() {
                native_instance_original(
                    child,
                    &format!("{path}/field[{index}]={name:?}"),
                    text,
                    floats,
                );
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            let kind = if matches!(value, Instance::Repeated(_)) {
                "repeated"
            } else {
                "mapped"
            };
            for (index, child) in items.iter().enumerate() {
                native_instance_original(child, &format!("{path}/{kind}[{index}]"), text, floats);
            }
        }
        Instance::DocumentSet(documents) => {
            for (index, document) in documents.iter().enumerate() {
                native_instance_original(
                    document.value(),
                    &format!("{path}/document[{index}]={:?}", document.path()),
                    text,
                    floats,
                );
            }
        }
    }
}

fn native_error_original(error: &engine::EngineError, path: &str, text: &mut String) {
    use std::fmt::Write as _;
    writeln!(
        text,
        "path={path:?};complete-error={error:#?};display={error}"
    )
    .unwrap();
    match error {
        engine::EngineError::FilterMapRuntime { source, .. } => {
            native_error_original(source, &format!("{path}/source"), text);
        }
        engine::EngineError::FilterMapValueType { found, .. } => {
            native_value_original(found, &format!("{path}/found"), text);
        }
        _ => {}
    }
}

fn native_error_chain(error: &engine::EngineError) -> Vec<String> {
    let mut result = Vec::new();
    let mut next = std::error::Error::source(error);
    while let Some(error) = next {
        result.push(error.to_string());
        next = error.source();
    }
    result
}

fn native_expectation(case: &Json) -> Result<ir::Instance, engine::EngineError> {
    let expected = &case["expected"];
    if expected["status"] == "ok" {
        let fields = expected["typed_result"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| {
                let value = &field[1];
                let value = match value["tag"].as_str().unwrap() {
                    "int" => Value::Int(value["value"].as_i64().unwrap()),
                    "float" => Value::Float(f64::from_bits(
                        u64::from_str_radix(value["ieee754_binary64_hex"].as_str().unwrap(), 16)
                            .unwrap(),
                    )),
                    "string" => Value::String(value["value"].as_str().unwrap().into()),
                    other => panic!("unknown frozen native expected tag {other}"),
                };
                (
                    field[0].as_str().unwrap().into(),
                    ir::Instance::Scalar(value),
                )
            })
            .collect::<Vec<_>>();
        Ok(ir::Instance::Group(fields.into()))
    } else {
        let diagnostic = &expected["typed_diagnostic"];
        Err(match diagnostic["category"].as_str().unwrap() {
            "NotABool" => engine::EngineError::NotABool {
                node: 4,
                found: match diagnostic["found"].as_str().unwrap() {
                    "int" => "int",
                    "float" => "float",
                    "string" => "string",
                    "null" => "null",
                    other => panic!("unknown frozen native bool diagnostic {other}"),
                },
            },
            "MappingException" => engine::EngineError::MappingException {
                node: 4,
                message: Some(diagnostic["message_payload"].as_str().unwrap().into()),
            },
            other => panic!("unknown frozen native diagnostic {other}"),
        })
    }
}

fn native_counterparts(cases: &[Json], directory: &Path) {
    let mut observations = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let project = native_project(case);
        let source = ir::Instance::Group(Vec::new().into());
        let encoded = mapping::project_file::encode_pretty(&project);
        let mut project_original =
            format!("complete-project={project:#?}\nproject-codec={encoded:#?}\n");
        for (node, expression) in &project.graph.nodes {
            if let mapping::Node::Const { value } = expression {
                native_value_original(value, &format!("graph/{node}"), &mut project_original);
            }
        }
        fs::write(
            directory.join(format!("{id}.NATIVE_PROJECT.original.txt")),
            project_original,
        )
        .unwrap();
        let input_text = case["source_json"].as_str().unwrap();
        let mut input_original = format!(
            "frozen-json-input={input_text:?}\ninput-bytes={:?}\n",
            input_text.as_bytes()
        );
        native_instance_original(&source, "source", &mut input_original, &mut Vec::new());
        fs::write(
            directory.join(format!("{id}.NATIVE_INPUT.original.txt")),
            input_original,
        )
        .unwrap();
        let actual = engine::run(&project, &source);
        let mut run_original = format!("complete-public-native-result={actual:#?}\n");
        let mut floats = Vec::new();
        let causes = match &actual {
            Ok(value) => {
                native_instance_original(value, "target", &mut run_original, &mut floats);
                Vec::new()
            }
            Err(error) => {
                native_error_original(error, "error", &mut run_original);
                native_error_chain(error)
            }
        };
        run_original.push_str(&format!("whole-original-cause-chain={causes:#?}\n"));
        fs::write(
            directory.join(format!("{id}.NATIVE_RUN.original.txt")),
            run_original,
        )
        .unwrap();
        observations.push((case, actual, floats, causes, encoded.is_ok()));
    }
    fs::write(
        directory.join("SEPARATE_NATIVE_ENGINE_CALL_COUNT.original.txt"),
        format!("SEPARATE_NATIVE_ENGINE_CALL_COUNT={}\n", observations.len()),
    )
    .unwrap();
    eprintln!("SEPARATE_NATIVE_ENGINE_CALL_COUNT={}", observations.len());
    // Every actual native outcome above is published before any comparison below.
    for (case, actual, actual_floats, actual_causes, codec_ok) in observations {
        let id = case["id"].as_str().unwrap();
        let expected = native_expectation(case);
        let mut expected_original = format!(
            "complete-frozen-literal={}\ncomplete-native-expectation={expected:#?}\n",
            case["expected"]
        );
        let mut expected_floats = Vec::new();
        match &expected {
            Ok(value) => native_instance_original(
                value,
                "target",
                &mut expected_original,
                &mut expected_floats,
            ),
            Err(error) => native_error_original(error, "error", &mut expected_original),
        }
        fs::write(
            directory.join(format!("{id}.NATIVE_EXPECTED.original.txt")),
            expected_original,
        )
        .unwrap();
        assert!(
            codec_ok,
            "native Project codec failed; original retained for {id}"
        );
        assert_eq!(
            case["source_json"], "{}",
            "native input is the frozen empty group for {id}"
        );
        assert_eq!(
            actual_floats, expected_floats,
            "native exact binary64 payloads: {id}"
        );
        assert_eq!(&actual, &expected, "native full typed Instance/error: {id}");
        if let Err(error) = &actual {
            let diagnostic = &case["expected"]["typed_diagnostic"];
            assert_eq!(
                error.to_string(),
                diagnostic["message"].as_str().unwrap(),
                "native error message: {id}"
            );
            let expected_causes = diagnostic["source_chain"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                actual_causes, expected_causes,
                "native whole error cause chain: {id}"
            );
        } else {
            assert_eq!(case["expected"]["typed_result"]["kind"], "group");
            assert_eq!(case["expected"]["typed_result"]["origin"], "Unknown");
        }
    }
    assert_eq!(cases.len(), 15);
}

fn expectation(case: &Json) -> String {
    let expected = &case["expected"];
    if expected["status"] == "ok" {
        let value = &expected["typed_result"]["fields"][0][1];
        let value = match value["tag"].as_str().unwrap() {
            "int" => format!("Value::Int({})", value["value"].as_i64().unwrap()),
            "float" => format!(
                "Value::Float(f64::from_bits(0x{}))",
                value["ieee754_binary64_hex"].as_str().unwrap()
            ),
            "string" => format!(
                "Value::String({:?}.into())",
                value["value"].as_str().unwrap()
            ),
            other => panic!("unknown frozen result {other}"),
        };
        format!(
            "Expected::Value {{ value: {value}, json: {:?} }}",
            expected["complete_expected_utf8"].as_str().unwrap()
        )
    } else {
        let diagnostic = &expected["typed_diagnostic"];
        let error = match diagnostic["category"].as_str().unwrap() {
            "NotABool" => format!(
                "RuntimeError::NotABool {{ node: 4, found: {:?} }}",
                diagnostic["found"].as_str().unwrap()
            ),
            "MappingException" => format!(
                "RuntimeError::MappingException {{ node: 4, message: Some({:?}.into()) }}",
                diagnostic["message_payload"].as_str().unwrap()
            ),
            other => panic!("unknown frozen diagnostic {other}"),
        };
        format!(
            "Expected::Error {{ error: {error}, message: {:?} }}",
            diagnostic["message"].as_str().unwrap()
        )
    }
}

#[test]
fn generated_legacy_sequence_aggregate_predicates_compile_and_preserve_complete_oracles() {
    let corpus: Json = serde_json::from_str(CASES).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let output = TempDir::new("legacy_sequence_aggregate_predicate");
    let directory = output.path().to_path_buf();
    std::mem::forget(output);
    fs::write(directory.join("FROZEN_COMPLETE_CASES.original.json"), CASES).unwrap();
    native_counterparts(cases, &directory);
    let mut host = String::new();
    let mut registrations = String::new();
    for (index, case) in cases.iter().enumerate() {
        let id = case["id"].as_str().unwrap();
        let program = program(case);
        let floats = program
            .expressions
            .iter()
            .filter_map(|node| match &node.expression {
                Expression::Const {
                    value: Value::Float(value),
                } => Some((node.id, value.to_bits())),
                _ => None,
            })
            .collect::<Vec<_>>();
        fs::write(
            directory.join(format!("{id}.PROGRAM.original.txt")),
            format!("{program:#?}\nexact Float bits={floats:#?}\n"),
        )
        .unwrap();
        let emitted = emit(
            &program,
            &Options {
                package_name: "legacy-aggregate-map".into(),
                runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
            },
        );
        fs::write(
            directory.join(format!("{id}.EMIT_RESULT.original.txt")),
            format!("{emitted:#?}\n"),
        )
        .unwrap();
        let artifacts = emitted.unwrap();
        write_artifacts(&directory.join("ordinary-artifacts").join(id), &artifacts);
        if index == 0 {
            write_artifacts(&directory, &artifacts);
        }
        let generated = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap();
        let module = directory.join("src/generated").join(format!("{id}.rs"));
        fs::create_dir_all(module.parent().unwrap()).unwrap();
        fs::write(module, &generated.contents).unwrap();
        host.push_str(&format!("#[path = \"generated/{id}.rs\"] pub mod {id};\n"));
        registrations.push_str(&format!("Case {{ id: {id:?}, execute: {id}::execute, text: {id}::execute_json, bytes: {id}::execute_json_bytes, expected: {} }},\n", expectation(case)));
    }
    host.push_str(HOST);
    host.push_str(&format!(
        "\nfn main() {{ let cases = [{registrations}]; run(&cases); }}\n"
    ));
    fs::write(directory.join("src/main.rs"), host).unwrap();
    let launched = Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(&directory)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-D warnings")
        .generated_host_output(&directory);
    fs::write(
        directory.join("HOST_FULL_LAUNCH_RESULT.original.txt"),
        format!("{launched:#?}\n"),
    )
    .unwrap();
    let original = launched.expect("generated host launch failed; original result retained");
    fs::write(directory.join("HOST_STDOUT.original.bin"), &original.stdout).unwrap();
    fs::write(directory.join("HOST_STDERR.original.bin"), &original.stderr).unwrap();
    eprintln!(
        "legacy aggregate full host status={} originals={}",
        original.status,
        directory.display()
    );
    assert_eq!(cases.len(), 15);
    assert!(
        original.status.success(),
        "generated legacy aggregate host failed; originals at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&original.stdout),
        String::from_utf8_lossy(&original.stderr)
    );
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
        fs::remove_dir_all(&directory).unwrap();
    }
}
