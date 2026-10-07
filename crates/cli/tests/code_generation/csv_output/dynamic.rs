use super::*;

// Loader adapters add no transport or JSON input boundary.
use super::super::typed_dynamic_document_sources::typed_snapshot as dynamic_csv_snapshot;

const DYNAMIC_CSV_MARKER: &str = "dynamic-csv-original-second-loader-failure";

fn dynamic_csv_project(mixed: bool) -> Project {
    let mut project = Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Driver",
                    vec![
                        SchemaNode::scalar("Path", ScalarType::String)
                            .nullable()
                            .unwrap(),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Row",
            vec![string("Path"), string("File"), string("Value"), int("Rate")],
        ),
        source_path: None,
        target_path: Some("rows.csv".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "catalog".into(),
            path: String::new(),
            schema: SchemaNode::group(
                "Catalog",
                vec![
                    SchemaNode::group(
                        "Row",
                        vec![
                            string("File"),
                            SchemaNode::scalar("Value", ScalarType::String)
                                .nullable()
                                .unwrap(),
                        ],
                    )
                    .repeating(),
                ],
            ),
            options: Default::default(),
            dynamic_path: Some(mapping::DynamicSourcePath {
                node: 0,
                iteration: vec!["Driver".into()],
            }),
        }],
        extra_targets: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Path".into()],
                        frame: Some(vec!["Driver".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["File".into()],
                        frame: Some(vec!["catalog".into(), "Row".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["catalog".into(), "Row".into()]),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
                (
                    8,
                    Node::RuntimeParameterDefault {
                        name: "Override".into(),
                        ty: ScalarType::Int,
                        default: 3,
                        preview: None,
                    },
                ),
                (
                    10,
                    Node::RuntimeParameterDefault {
                        name: "Abort".into(),
                        ty: ScalarType::Bool,
                        default: 4,
                        preview: None,
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::String("blocked".into()),
                    },
                ),
                (
                    12,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    13,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    14,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![12, 13],
                    },
                ),
                (
                    15,
                    Node::RuntimeParameterDefault {
                        name: "MessageFail".into(),
                        ty: ScalarType::Bool,
                        default: 4,
                        preview: None,
                    },
                ),
                (
                    16,
                    Node::If {
                        condition: 15,
                        then: 14,
                        else_: 11,
                    },
                ),
            ]),
        },
        failure_rules: vec![mapping::FailureRule {
            iteration: mapping::FailureIteration::Source {
                collection: vec!["Driver".into()],
            },
            selection: mapping::FailureSelection::WhenTrue { predicate: 10 },
            message: Some(16),
        }],
        root: Scope {
            iteration: ScopeIteration::Source(vec!["catalog".into(), "Row".into()]),
            bindings: vec![
                Binding {
                    target_field: "Rate".into(),
                    node: 8,
                },
                Binding {
                    target_field: "Value".into(),
                    node: 2,
                },
                Binding {
                    target_field: "File".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Path".into(),
                    node: 0,
                },
            ],
            ..Scope::default()
        },
    };
    if mixed {
        project.extra_sources.push(mapping::NamedSource {
            name: "Rates".into(),
            path: "rates.json".into(),
            schema: SchemaNode::group("Rates", vec![int("Rate")]),
            options: Default::default(),
            dynamic_path: None,
        });
        project.graph.nodes.insert(
            3,
            Node::SourceField {
                path: vec!["Rates".into(), "Rate".into()],
                frame: None,
            },
        );
    }
    project
}

fn dynamic_csv_group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn dynamic_csv_scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}
fn dynamic_csv_text(value: &str) -> Value {
    Value::String(value.into())
}
fn dynamic_csv_document(file: &str, value: Value, empty: bool) -> Instance {
    dynamic_csv_group(vec![(
        "Row",
        Instance::Repeated(if empty {
            Vec::new()
        } else {
            vec![dynamic_csv_group(vec![
                ("File", dynamic_csv_scalar(dynamic_csv_text(file))),
                ("Value", dynamic_csv_scalar(value)),
            ])]
        }),
    )])
}
fn dynamic_csv_primary(paths: &[Value]) -> Instance {
    dynamic_csv_group(vec![(
        "Driver",
        Instance::Repeated(
            paths
                .iter()
                .map(|path| dynamic_csv_group(vec![("Path", dynamic_csv_scalar(path.clone()))]))
                .collect(),
        ),
    )])
}
fn dynamic_csv_expected(paths: &[Value], first: &Value, empty: bool, rate: i64) -> Instance {
    Instance::Repeated(
        paths
            .iter()
            .filter_map(|path| {
                let Value::String(path) = path else {
                    return None;
                };
                let (file, value) = match path.as_str() {
                    "a.xml" if !empty => ("same.xml", first.clone()),
                    "b.xml" => ("../雪😀.xml", dynamic_csv_text("  B  ")),
                    _ => return None,
                };
                Some(dynamic_csv_group(vec![
                    ("Rate", dynamic_csv_scalar(Value::Int(rate))),
                    ("Value", dynamic_csv_scalar(value)),
                    ("File", dynamic_csv_scalar(dynamic_csv_text(file))),
                    ("Path", dynamic_csv_scalar(dynamic_csv_text(path))),
                ]))
            })
            .collect(),
    )
}

struct DynamicCsvNativeLoader<'a> {
    first: &'a Instance,
    second: &'a Instance,
    fail_second: bool,
    directory: PathBuf,
    calls: std::cell::RefCell<Vec<serde_json::Value>>,
}
impl engine::DynamicSourceLoader for DynamicCsvNativeLoader<'_> {
    fn load(&self, source: &str, path: &str) -> Result<std::sync::Arc<Instance>, String> {
        use serde_json::json;
        let result = match (source, path) {
            ("catalog", "a.xml") => Ok(std::sync::Arc::new(self.first.clone())),
            ("catalog", "b.xml") if self.fail_second => Err(DYNAMIC_CSV_MARKER.to_string()),
            ("catalog", "b.xml") => Ok(std::sync::Arc::new(self.second.clone())),
            _ => Err("dynamic-csv-unexpected-path".to_string()),
        };
        let ordinal = self.calls.borrow().len() + 1;
        let outcome = match &result {
            Ok(value) => {
                let file = format!("callback-{ordinal}-document.txt");
                use std::io::Write as _;
                if let Err(error) = std::fs::File::create(self.directory.join(&file))
                    .and_then(|mut file| writeln!(file, "{value:#?}"))
                {
                    return Err(format!("raw callback evidence failed: {error}"));
                }
                json!({"kind":"returned","document":file})
            }
            Err(message) => json!({"kind":"error","message":message}),
        };
        self.calls
            .borrow_mut()
            .push(json!({"ordinal":ordinal,"source":source,"path":path,"outcome":outcome}));
        result
    }
}
fn dynamic_csv_native_error(error: &engine::EngineError) -> serde_json::Value {
    use serde_json::json;
    match error {
        engine::EngineError::MissingDynamicSourceLoader { source_name } => {
            json!({"kind":"missing-loader","source":source_name})
        }
        engine::EngineError::DynamicSourcePath { source_name, found } => {
            json!({"kind":"path","source":source_name,"node":0,"found":found})
        }
        engine::EngineError::DynamicSourceLoad {
            source_name,
            path,
            message,
        } => json!({"kind":"load","source":source_name,"path":path,"message":message}),
        engine::EngineError::MappingFailure { rule, message } => {
            json!({"kind":"failure","rule":rule,"message":message})
        }
        engine::EngineError::Function(codegen_runtime::FunctionError::DivideByZero) => {
            json!({"kind":"divide"})
        }
        engine::EngineError::RuntimeParameterType {
            node,
            name,
            expected,
            found,
        } => {
            json!({"kind":"parameter","node":node,"name":name,"expected":format!("{expected:?}"),"found":found})
        }
        _ => panic!("unexpected native mapping error: {error:?}"),
    }
}

struct DynamicCsvCase {
    name: &'static str,
    paths: Vec<Value>,
    first: Value,
    empty_first: bool,
    fail_second: bool,
    loader: bool,
    parameters: Vec<(String, Value)>,
    context_only: bool,
    generated_only: bool,
    static_names: &'static str,
    large: bool,
}
fn dynamic_csv_large_value_length() -> usize {
    // Default header + one context row without Value is exactly 40 UTF-8 bytes.
    let overhead = "Path,File,Value,Rate\na.xml,same.xml,,19\n".len();
    assert_eq!(overhead, 40);
    MAXIMUM + 1 - overhead
}
fn dynamic_csv_cases(mixed: bool, large: bool) -> Vec<DynamicCsvCase> {
    let base = || DynamicCsvCase {
        name: "happy",
        paths: vec![dynamic_csv_text("a.xml"), dynamic_csv_text("b.xml")],
        first: dynamic_csv_text("A"),
        empty_first: false,
        fail_second: false,
        loader: true,
        parameters: vec![("Override".into(), Value::Int(19))],
        context_only: false,
        generated_only: false,
        static_names: "valid",
        large: false,
    };
    if large {
        return ["limit-plus-one", "large-before-late-host"]
            .into_iter()
            .map(|name| {
                let mut case = base();
                case.name = name;
                case.large = true;
                case.context_only = true;
                case.paths = if name == "limit-plus-one" {
                    vec![dynamic_csv_text("a.xml")]
                } else {
                    case.paths
                };
                // The large typed value is created only while preparing this one case.
                case.fail_second = name == "large-before-late-host";
                case
            })
            .collect();
    }
    let mut cases = Vec::new();
    for name in [
        "happy",
        "null-paths",
        "zero-drivers",
        "bad-path",
        "late-bad-path",
        "late-host",
        "bad-first-row",
        "bad-row-before-late-host",
        "null-value",
        "json-null-value",
        "empty-loaded-first",
        "quoted-loaded",
        "lazy-message",
        "selected-failure",
        "selected-message-failure",
        "wrong-host-type",
        "missing-loader",
        "empty-path",
        "path-byte-limit",
    ] {
        let mut case = base();
        case.name = name;
        match name {
            "null-paths" => {
                case.paths = vec![Value::Null, Value::json_null(), dynamic_csv_text("b.xml")]
            }
            "zero-drivers" => case.paths.clear(),
            "bad-path" => case.paths = vec![Value::Int(7)],
            "late-bad-path" => case.paths = vec![dynamic_csv_text("a.xml"), Value::Int(7)],
            "late-host" => case.fail_second = true,
            "bad-first-row" => case.first = Value::xml_nil(),
            "bad-row-before-late-host" => {
                case.first = Value::xml_nil();
                case.fail_second = true;
            }
            "null-value" => case.first = Value::Null,
            "json-null-value" => case.first = Value::json_null(),
            "empty-loaded-first" => case.empty_first = true,
            "quoted-loaded" => case.first = dynamic_csv_text("A,\"雪😀\"\nline"),
            "lazy-message" => {
                case.context_only = true;
                case.parameters
                    .push(("MessageFail".into(), Value::Bool(true)));
            }
            "selected-failure" => {
                case.context_only = true;
                case.parameters.push(("Abort".into(), Value::Bool(true)));
            }
            "selected-message-failure" => {
                case.context_only = true;
                case.parameters.extend([
                    ("Abort".into(), Value::Bool(true)),
                    ("MessageFail".into(), Value::Bool(true)),
                ]);
            }
            "wrong-host-type" => {
                case.context_only = true;
                case.parameters = vec![("Override".into(), dynamic_csv_text("bad"))];
            }
            "missing-loader" => case.loader = false,
            "empty-path" => case.paths = vec![dynamic_csv_text("")],
            "path-byte-limit" => {
                case.paths = vec![Value::String("é".repeat(2049))];
                case.generated_only = true;
            }
            _ => {}
        }
        cases.push(case);
    }
    if mixed {
        for names in [
            "missing",
            "duplicate",
            "unexpected",
            "dynamic-name",
            "loader-only",
        ] {
            let mut case = base();
            case.name = match names {
                "missing" => "static-missing-before-loader",
                "duplicate" => "static-duplicate-before-loader",
                "unexpected" => "static-unexpected-before-loader",
                "dynamic-name" => "dynamic-name-not-static-input",
                _ => "mixed-loader-only",
            };
            case.static_names = names;
            case.generated_only = true;
            case.paths = vec![Value::Int(7)];
            case.parameters.push(("Abort".into(), Value::Bool(true)));
            cases.push(case);
        }
    }
    cases
}

fn dynamic_csv_expected_error(case: &DynamicCsvCase, context: bool) -> Option<serde_json::Value> {
    use serde_json::json;
    let name = case.name;
    if case.static_names != "valid" {
        return Some(match case.static_names {
            "missing" | "loader-only" => json!({"kind":"missing-name","source":"Rates"}),
            "duplicate" => json!({"kind":"duplicate-name","source":"Rates"}),
            "dynamic-name" => json!({"kind":"unexpected-name","source":"catalog"}),
            _ => json!({"kind":"unexpected-name","source":"Other"}),
        });
    }
    Some(match name {
        "missing-loader" => json!({"kind":"missing-loader","source":"catalog"}),
        "bad-path" | "late-bad-path" => {
            json!({"kind":"path","source":"catalog","node":0,"found":"int"})
        }
        "late-host" | "bad-row-before-late-host" | "large-before-late-host" => {
            json!({"kind":"load","source":"catalog","path":"b.xml","message":DYNAMIC_CSV_MARKER})
        }
        "empty-path" => {
            json!({"kind":"load","source":"catalog","path":"","message":"dynamic-csv-unexpected-path"})
        }
        "path-byte-limit" => json!({"kind":"path-limit","source":"catalog","maximum":4096}),
        "selected-failure" if context => json!({"kind":"failure","rule":1,"message":"blocked"}),
        "selected-message-failure" if context => json!({"kind":"divide"}),
        "wrong-host-type" if context => {
            json!({"kind":"parameter","node":8,"name":"Override","expected":"Int","found":"string"})
        }
        _ => return None,
    })
}
fn dynamic_csv_expected_callbacks(case: &DynamicCsvCase, context: bool) -> Vec<serde_json::Value> {
    use serde_json::json;
    if !case.loader
        || case.static_names != "valid"
        || case.name == "path-byte-limit"
        || (context && matches!(case.name, "selected-failure" | "selected-message-failure"))
    {
        return Vec::new();
    }
    let mut callbacks = Vec::new();
    for path in &case.paths {
        match path {
            Value::Null | Value::JsonNull(_) => continue,
            Value::String(path) => {
                let message = if path == "b.xml" && case.fail_second {
                    Some(DYNAMIC_CSV_MARKER)
                } else if path != "a.xml" && path != "b.xml" {
                    Some("dynamic-csv-unexpected-path")
                } else {
                    None
                };
                callbacks.push(json!({"ordinal":callbacks.len()+1,"source":"catalog","path":path,
                    "outcome":match message { Some(message) => json!({"kind":"error","message":message}),
                        None => json!({"kind":"returned","document":format!("callback-{}-document.txt",callbacks.len()+1)}) }}));
                if message.is_some() {
                    break;
                }
            }
            _ => break,
        }
    }
    callbacks
}

fn prepare_dynamic_csv_cases(
    output: &Path,
    project: &Project,
    mixed: bool,
    large: bool,
) -> TestResult<()> {
    use serde_json::json;
    use std::io::Write as _;
    let mut records = Vec::new();
    for case in dynamic_csv_cases(mixed, large) {
        let owned = output.join(format!("native-{}", case.name));
        std::fs::create_dir(&owned)?;
        let primary = dynamic_csv_primary(&case.paths);
        let first_value = if case.large {
            Value::String("x".repeat(dynamic_csv_large_value_length()))
        } else {
            case.first.clone()
        };
        let first = dynamic_csv_document("same.xml", first_value, case.empty_first);
        let second = dynamic_csv_document("../雪😀.xml", dynamic_csv_text("  B  "), false);
        let extras = if mixed {
            vec![(
                "Rates".into(),
                dynamic_csv_group(vec![("Rate", dynamic_csv_scalar(Value::Int(7)))]),
            )]
        } else {
            Vec::new()
        };
        writeln!(
            std::fs::File::create(owned.join("prepared-inputs.txt"))?,
            "primary={primary:#?}\nfirst={first:#?}\nsecond={second:#?}\nstatic={extras:#?}\nparameters={:#?}",
            case.parameters
        )?;
        let mut wanted = Vec::new();
        for context in [false, true] {
            if case.large && !context {
                wanted.push(json!({"not_invoked":true}));
                continue;
            }
            let label = if context { "context" } else { "default" };
            let callbacks = dynamic_csv_expected_callbacks(&case, context);
            let error = dynamic_csv_expected_error(&case, context);
            let mut expected_mapped = None;
            let csv_error = if case.name == "bad-first-row" {
                Some(
                    json!({"kind":"value-type","row":0,"field":"Value","expected":"String","got":"xml nil"}),
                )
            } else if case.name == "limit-plus-one" {
                Some(json!({"kind":"output-limit","maximum":MAXIMUM}))
            } else {
                None
            };
            if !case.generated_only && (!case.large || context) {
                let current = owned.join(label);
                std::fs::create_dir(&current)?;
                let loader = DynamicCsvNativeLoader {
                    first: &first,
                    second: &second,
                    fail_second: case.fail_second,
                    directory: current.clone(),
                    calls: Default::default(),
                };
                let mut parameters = engine::RuntimeParameters::new();
                if context {
                    for (name, value) in &case.parameters {
                        parameters.insert(name, value.clone())?;
                    }
                }
                let mut execution = engine::ExecutionContext::new(Path::new("dynamic-csv.json"))
                    .with_parameters(&parameters);
                if case.loader {
                    execution = execution.with_dynamic_source_loader(&loader);
                }
                let mapped = engine::run_with_sources_and_context(
                    project,
                    &primary,
                    extras.clone(),
                    &execution,
                );
                // Unconditional full Debug and complete actual callback originals precede every oracle assertion.
                writeln!(
                    std::fs::File::create(current.join("mapping-original.txt"))?,
                    "{mapped:#?}"
                )?;
                std::fs::write(
                    current.join("callbacks.json"),
                    serde_json::to_vec_pretty(&*loader.calls.borrow())?,
                )?;
                assert_eq!(
                    *loader.calls.borrow(),
                    callbacks,
                    "{} native {label}",
                    case.name
                );
                if let Some(wanted_error) = &error {
                    assert_eq!(
                        dynamic_csv_native_error(mapped.as_ref().expect_err("mapping must fail")),
                        *wanted_error
                    );
                } else {
                    let mapped = mapped?;
                    let rate = if context { 19 } else { 7 };
                    if !case.large {
                        let independent =
                            dynamic_csv_expected(&case.paths, &case.first, case.empty_first, rate);
                        assert_eq!(mapped, independent, "{} native {label}", case.name);
                        expected_mapped = Some(dynamic_csv_snapshot::snapshot(&independent));
                    }
                    let serialized = native_bytes(project, &mapped);
                    record_native_bytes(&current, "csv-original", &serialized)?;
                    if let Some(wanted_error) = &csv_error {
                        match &serialized {
                            Err(CsvBoundedError::Format(CsvFormatError::ValueType {
                                row,
                                field,
                                expected,
                                got,
                            })) => assert_eq!(
                                json!({"kind":"value-type","row":row,"field":field,"expected":format!("{expected:?}"),"got":got}),
                                *wanted_error
                            ),
                            Err(CsvBoundedError::OutputTooLarge { maximum }) => assert_eq!(
                                json!({"kind":"output-limit","maximum":maximum}),
                                *wanted_error
                            ),
                            _ => panic!(
                                "{} native {label} wrong CSV outcome: {serialized:?}",
                                case.name
                            ),
                        }
                    } else {
                        let bytes = serialized?;
                        if case.name == "happy" {
                            assert_eq!(bytes, format!("Path,File,Value,Rate\na.xml,same.xml,A,{rate}\nb.xml,../雪😀.xml,  B  ,{rate}\n").as_bytes());
                        }
                        std::fs::write(
                            output.join(format!("expected-{}-{label}.csv", case.name)),
                            bytes,
                        )?;
                    }
                }
            }
            wanted.push(json!({"runtime_error":error,"csv_error":csv_error,"callbacks":callbacks,"mapped":expected_mapped}));
        }
        records.push(json!({"name":case.name,"paths":case.paths.iter().map(|value| dynamic_csv_snapshot::snapshot(&dynamic_csv_scalar(value.clone()))).collect::<Vec<_>>(),
            "first":if case.large { json!({"repeat_x":dynamic_csv_large_value_length()}) } else { dynamic_csv_snapshot::snapshot(&dynamic_csv_scalar(case.first.clone())) },
            "empty_first":case.empty_first,"fail_second":case.fail_second,"loader":case.loader,
            "parameters":case.parameters.iter().map(|(name,value)|json!({"name":name,"value":dynamic_csv_snapshot::snapshot(&dynamic_csv_scalar(value.clone()))})).collect::<Vec<_>>(),
            "context_only":case.context_only,"static_names":case.static_names,"large":case.large,"expected":wanted}));
    }
    std::fs::write(
        output.join(if large {
            "dynamic-large-cases.json"
        } else {
            "dynamic-cases.json"
        }),
        serde_json::to_vec_pretty(&records)?,
    )?;
    Ok(())
}

fn dynamic_csv_host_source(template: &str, language: &str, mixed: bool) -> String {
    let expressions = match (language, mixed) {
        ("rust", false) => [
            "generated::execute_csv(source)",
            "generated::execute_csv_bytes(source)",
            "generated::execute_csv_with_context(source, context)",
            "generated::execute_csv_bytes_with_context(source, context)",
        ],
        ("rust", true) => [
            "generated::execute_csv_with_sources(source, inputs)",
            "generated::execute_csv_bytes_with_sources(source, inputs)",
            "generated::execute_csv_with_sources_and_context(source, inputs, context)",
            "generated::execute_csv_bytes_with_sources_and_context(source, inputs, context)",
        ],
        ("csharp", false) => [
            "GeneratedMapping.ExecuteCsv(source)",
            "GeneratedMapping.ExecuteCsvBytes(source)",
            "GeneratedMapping.ExecuteCsv(source,context)",
            "GeneratedMapping.ExecuteCsvBytes(source,context)",
        ],
        ("csharp", true) => [
            "GeneratedMapping.ExecuteCsvWithSources(source,inputs)",
            "GeneratedMapping.ExecuteCsvBytesWithSources(source,inputs)",
            "GeneratedMapping.ExecuteCsvWithSources(source,inputs,context)",
            "GeneratedMapping.ExecuteCsvBytesWithSources(source,inputs,context)",
        ],
        _ => unreachable!("two generated targets"),
    };
    let mut source = template.to_string();
    for (placeholder, expression) in [
        "__NO_LOADER_TEXT__",
        "__NO_LOADER_BYTES__",
        "__NO_LOADER_TEXT_CONTEXT__",
        "__NO_LOADER_BYTES_CONTEXT__",
    ]
    .into_iter()
    .zip(expressions)
    {
        assert_eq!(source.matches(placeholder).count(), 1);
        source = source.replace(placeholder, expression);
    }
    assert!(!source.contains("__NO_LOADER_"));
    source
}
fn verify_dynamic_csv_host_receipt(output: &Path, mode: &str) -> TestResult<()> {
    use serde_json::json;
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(
        output.join(format!("dynamic-{mode}-complete.json")),
    )?)?;
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(output.join(if mode == "large" {
            "dynamic-large-cases.json"
        } else {
            "dynamic-cases.json"
        }))?)?;
    let mut expected = Vec::new();
    for case in &cases {
        let apis: &[&str] = if mode == "large" {
            &["bytes-context"]
        } else if case["static_names"] == "loader-only" {
            &["text-loader", "bytes-loader"]
        } else if case["context_only"] == true {
            &["text-context", "bytes-context"]
        } else if mode == "mixed" {
            &[
                "text-sources",
                "bytes-sources",
                "text-context",
                "bytes-context",
            ]
        } else {
            &[
                "text-loader",
                "bytes-loader",
                "text-sources",
                "bytes-sources",
                "text-context",
                "bytes-context",
            ]
        };
        for api in apis {
            expected.push(json!({"case":case["name"],"api":api,"csv_calls":1,"ordinary_calls":usize::from(mode != "large")}));
        }
    }
    assert_eq!(
        (cases.len(), expected.len()),
        match mode {
            "pure" => (19, 98),
            "mixed" => (24, 86),
            "large" => (2, 2),
            _ => unreachable!(),
        }
    );
    assert_eq!(
        receipt,
        json!({"mode":mode,"cases":cases.len(),"csv_calls":expected.len(),"ordinary_calls":if mode == "large" { 0 } else { expected.len() },"calls":expected})
    );
    Ok(())
}

fn run_dynamic_rust_fixture(output: &Path, mode: &str) -> TestResult<()> {
    let main = output.join("src/main.rs");
    let fixture = dynamic_csv_host_source(
        include_str!("../fixtures/csv_output_dynamic_rust_harness.rs.txt"),
        "rust",
        mode == "mixed",
    );
    if main.exists() {
        assert_eq!(std::fs::read_to_string(&main)?, fixture);
    } else {
        std::fs::write(main, &fixture)?;
    }
    let snapshot = include_str!("../fixtures/typed_document_driver_snapshot.rs.txt").replacen(
        "use ir::{",
        "use codegen_runtime::{",
        1,
    );
    let snapshot_path = output.join("src/typed_snapshot.rs");
    if snapshot_path.exists() {
        assert_eq!(std::fs::read_to_string(&snapshot_path)?, snapshot);
    } else {
        std::fs::write(snapshot_path, snapshot)?;
    }
    let manifest = output.join("Cargo.toml");
    let body = std::fs::read_to_string(&manifest)?;
    if !body.contains("serde_json =") {
        assert_eq!(body.matches("\n[workspace]\n").count(), 1);
        std::fs::write(&manifest, body.replacen("\n[workspace]\n", "\nserde_json = { version = \"1.0\", features = [\"preserve_order\"] }\n\n[workspace]\n",1))?;
    }
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet", "--", mode])
        .current_dir(output)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_BUILD_JOBS", "1");
    if let Some(target) = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        command.env("CARGO_TARGET_DIR", target);
    }
    let actual = recorded_csv_command(&mut command, output, &format!("rust-dynamic-{mode}"))?;
    assert!(
        actual.status.success(),
        "generated Rust dynamic CSV {mode} failed; originals {}\n{}\n{}",
        output.display(),
        String::from_utf8_lossy(&actual.stdout),
        String::from_utf8_lossy(&actual.stderr)
    );
    verify_dynamic_csv_host_receipt(output, mode)?;
    Ok(())
}
fn run_dynamic_csharp_fixture(output: &Path, mode: &str) -> TestResult<()> {
    if mode != "large" {
        let fixture = dynamic_csv_host_source(
            include_str!("../fixtures/csv_output_dynamic_csharp.cs.txt"),
            "csharp",
            mode == "mixed",
        );
        run_csharp(output, &fixture, mode)?;
    } else {
        let mut command = dotnet_command(output);
        command
            .args([
                "run",
                "--no-build",
                "--project",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
                "--",
                "large",
            ])
            .current_dir(output);
        let actual = recorded_csv_command(&mut command, output, "csharp-dynamic-large")?;
        assert!(
            actual.status.success(),
            "generated C# dynamic CSV large failed; originals {}\n{}\n{}",
            output.display(),
            String::from_utf8_lossy(&actual.stdout),
            String::from_utf8_lossy(&actual.stderr)
        );
    }
    verify_dynamic_csv_host_receipt(output, mode)?;
    Ok(())
}
fn dynamic_csv_matrix(
    language: &str,
    target: GenerateTarget,
    run_host: fn(&Path, &str) -> TestResult<()>,
) -> TestResult<()> {
    let mut directory = CsvDirectory::new(&format!("{language}_csv_dynamic"))?;
    for mixed in [false, true] {
        let mode = if mixed { "mixed" } else { "pure" };
        let project = dynamic_csv_project(mixed);
        let owned = directory.path.join(mode);
        std::fs::create_dir(&owned)?;
        let validation = engine::validate(&project);
        std::fs::write(
            owned.join("validation-original.txt"),
            format!("{validation:#?}"),
        )?;
        assert!(validation.is_empty());
        let path = write_mapping(&owned, &project)?;
        let output = owned.join("generated");
        let generated = generate_project_with_csv_output(&path, &output, target.clone());
        std::fs::write(
            owned.join("generation-original.txt"),
            format!("{generated:#?}"),
        )?;
        generated?;
        prepare_dynamic_csv_cases(&output, &project, mixed, false)?;
        run_host(&output, mode)?;
        if !mixed {
            prepare_dynamic_csv_cases(&output, &project, false, true)?;
            run_host(&output, "large")?;
        }
    }
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_rust_csv_dynamic_loaders_preserve_driver_context_callbacks_and_failures()
-> TestResult<()> {
    dynamic_csv_matrix("rust", rust_target(), run_dynamic_rust_fixture)
}
#[test]
fn generated_csharp_csv_dynamic_loaders_preserve_driver_context_callbacks_and_failures()
-> TestResult<()> {
    dynamic_csv_matrix("csharp", GenerateTarget::CSharp, run_dynamic_csharp_fixture)
}
