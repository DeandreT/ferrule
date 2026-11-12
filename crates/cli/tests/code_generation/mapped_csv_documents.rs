//! Generic constructed-row output, not filename acquisition or reference parity.
use super::*;
use codegen::{CsvMappedDocumentOutputError, CsvMappedDocumentOutputPolicy, CsvOutputError};
use ir::SchemaKind;
use mapping::{AggregateOp, RuntimeValue};

const CLOCK: &str = "2026-11-12T03:04:05Z";
const BP: &str = "out/B-2026-11-12T03:04:05Z.csv";
const AP: &str = "out/A-2026-11-12T03:04:05Z.csv";
const B: &[u8] =
    b"\xef\xbb\xbfClient;SKU;Total;Label\nB;x;5;\"\xc3\xa9;\"\"Q\"\"\r\nZ\"\nB;y;6;bee\n";
const A: &[u8] = b"\xef\xbb\xbfClient;SKU;Total;Label\nA;x;12;\"A\"\"x\"\nA;z;-2;\xe5\xb0\xbe\n";

fn binding(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn literal(value: Value) -> Node {
    Node::Const { value }
}
fn call(name: &str, args: &[u32]) -> Node {
    Node::Call {
        function: name.into(),
        args: args.to_vec(),
    }
}
fn parameter(name: &str, ty: ScalarType, default: u32) -> Node {
    Node::RuntimeParameterDefault {
        name: name.into(),
        ty,
        default,
        preview: None,
    }
}

fn project(supplied_clock: bool) -> Project {
    Project {
        execution_contract: None,
        source: SchemaNode::group(
            "InputRow",
            vec![
                text("Account"),
                text("Product"),
                SchemaNode::scalar("Qty", ScalarType::Int),
                text("Memo"),
                text("Unused"),
            ],
        )
        .repeating(),
        target: SchemaNode::group(
            "Shipment",
            vec![
                SchemaNode::group(
                    "DetailLines",
                    vec![
                        text("Client"),
                        text("SKU"),
                        SchemaNode::scalar("Total", ScalarType::Int),
                        text("Label"),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: mapping::FormatOptions {
            delimiter: Some(';'),
            csv_utf8_bom: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::SourceField {
                        path: vec!["Account".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Product".into()],
                        frame: None,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Memo".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Aggregate {
                        function: AggregateOp::Sum,
                        collection: vec![],
                        value: vec!["Qty".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    6,
                    if supplied_clock {
                        Node::RuntimeValue {
                            value: RuntimeValue::CurrentDateTime,
                        }
                    } else {
                        literal(Value::String(CLOCK.into()))
                    },
                ),
                (7, literal(Value::String("out/".into()))),
                (8, literal(Value::String("-".into()))),
                (9, literal(Value::String(".csv".into()))),
                (10, call("concat", &[7, 1, 8, 6, 9])),
                (11, literal(Value::Bool(false))),
                (12, literal(Value::Bool(true))),
                (13, parameter("KeepRows", ScalarType::Bool, 12)),
                (14, parameter("Tick", ScalarType::Bool, 11)),
                (15, literal(Value::Int(1))),
                (
                    16,
                    Node::InvocationCounter {
                        key: "map".into(),
                        start: 15,
                        step: 15,
                        restart: None,
                    },
                ),
                (
                    17,
                    Node::If {
                        condition: 14,
                        then: 16,
                        else_: 4,
                    },
                ),
                (18, call("concat", &[7, 1, 8, 16, 9])),
                (
                    19,
                    Node::If {
                        condition: 14,
                        then: 18,
                        else_: 10,
                    },
                ),
                (20, parameter("Path", ScalarType::String, 19)),
                (21, literal(Value::String("A".into()))),
                (22, call("equal", &[1, 21])),
                (23, parameter("LatePath", ScalarType::String, 20)),
                (
                    42,
                    Node::If {
                        condition: 22,
                        then: 23,
                        else_: 20,
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: 42,
            },
            group_by: Some(1),
            children: vec![Scope {
                target_field: "DetailLines".into(),
                iteration: ScopeIteration::Source(vec![]),
                group_by: Some(2),
                filter: Some(13),
                bindings: vec![
                    binding("Label", 17),
                    binding("Total", 5),
                    binding("SKU", 2),
                    binding("Client", 1),
                ],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

fn row(account: &str, product: &str, qty: Value, memo: &str, unused: &str) -> Instance {
    Instance::Group(
        vec![
            (
                "Account".into(),
                Instance::Scalar(Value::String(account.into())),
            ),
            (
                "Product".into(),
                Instance::Scalar(Value::String(product.into())),
            ),
            ("Qty".into(), Instance::Scalar(qty)),
            ("Memo".into(), Instance::Scalar(Value::String(memo.into()))),
            (
                "Unused".into(),
                Instance::Scalar(Value::String(unused.into())),
            ),
        ]
        .into(),
    )
}
fn source() -> Instance {
    Instance::Repeated(vec![
        row("B", "x", Value::Int(2), "é;\"Q\"\r\nZ", "discard-1"),
        row("A", "x", Value::Int(11), "A\"x", "discard-2"),
        row("B", "y", Value::Int(7), "bee", "discard-3"),
        row("B", "x", Value::Int(3), "not-first-Bx", "discard-4"),
        row("A", "z", Value::Int(-2), "尾", "discard-5"),
        row("A", "x", Value::Int(1), "not-first-Ax", "discard-6"),
        row("B", "y", Value::Int(-1), "not-first-By", "discard-7"),
    ])
}

#[test]
fn mapped_csv_documents_native_literals_empty_filtered_and_execution_failures() -> TestResult<()> {
    let project = project(true);
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let policy = CsvMappedDocumentOutputPolicy::from_project(&project, "DetailLines")?;
    let schema = policy.row_schema(&project.target)?;
    let options = format_csv::CsvWriteOptions {
        delimiter: Some(';'),
        utf8_bom: true,
        ..Default::default()
    };
    let context =
        engine::ExecutionContext::new(Path::new("mapped.csv.ferrule")).with_current_datetime(CLOCK);
    let output = engine::run_with_context(&project, &source(), &context)?;
    let bytes = format_csv::to_mapped_document_bytes(&schema, &output, "DetailLines", &options)?;
    assert_eq!(
        bytes
            .iter()
            .map(|d| (d.path.as_str(), d.bytes.as_slice()))
            .collect::<Vec<_>>(),
        [(BP, B), (AP, A)]
    );
    let texts = format_csv::to_mapped_document_text(&schema, &output, "DetailLines", &options)?;
    assert_eq!(
        texts[0].text,
        "\u{feff}Client;SKU;Total;Label\nB;x;5;\"é;\"\"Q\"\"\r\nZ\"\nB;y;6;bee\n"
    );
    assert_eq!(
        texts[1].text,
        "\u{feff}Client;SKU;Total;Label\nA;x;12;\"A\"\"x\"\nA;z;-2;尾\n"
    );
    assert_eq!(
        engine::run_with_context(&project, &Instance::Repeated(vec![]), &context)?,
        Instance::DocumentSet(vec![])
    );
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("KeepRows", Value::Bool(false))?;
    let filtered =
        engine::run_with_context(&project, &source(), &context.with_parameters(&parameters))?;
    let bytes = format_csv::to_mapped_document_bytes(&schema, &filtered, "DetailLines", &options)?;
    assert_eq!(
        bytes
            .iter()
            .map(|d| (d.path.as_str(), d.bytes.as_slice()))
            .collect::<Vec<_>>(),
        [
            (BP, b"\xef\xbb\xbfClient;SKU;Total;Label\n".as_slice()),
            (AP, b"\xef\xbb\xbfClient;SKU;Total;Label\n")
        ]
    );
    for operands in [vec![i64::MAX, 1], vec![i64::MAX, 1, -1]] {
        let mut rows = vec![row("B", "x", Value::Int(1), "ok", "unused")];
        rows.extend(
            operands
                .into_iter()
                .map(|qty| row("A", "x", Value::Int(qty), "bad", "unused")),
        );
        assert!(matches!(
            engine::run_with_context(&project, &Instance::Repeated(rows), &context),
            Err(engine::EngineError::AggregateIntegerOverflow {
                function: AggregateOp::Sum
            })
        ));
    }
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("LatePath", Value::String("   ".into()))?;
    assert!(matches!(
        engine::run_with_context(&project, &source(), &context.with_parameters(&parameters)),
        Err(engine::EngineError::EmptyDynamicTargetPath { node: 42 })
    ));
    let mut bad_path = project.clone();
    bad_path.graph.nodes.insert(23, literal(Value::Int(7)));
    assert!(matches!(
        engine::run_with_context(&bad_path, &source(), &context),
        Err(engine::EngineError::DynamicTargetPath {
            node: 42,
            found: "int"
        })
    ));
    // Constructed Int adaptation is intentionally broader than direct CSV
    // Float-to-Int coercion (covered separately in format-csv tests).
    let integral = engine::run_with_context(
        &project,
        &Instance::Repeated(vec![row("B", "x", Value::Float(3.0), "ok", "unused")]),
        &context,
    )?;
    assert_eq!(
        format_csv::to_mapped_document_bytes(&schema, &integral, "DetailLines", &options)?[0].bytes,
        b"\xef\xbb\xbfClient;SKU;Total;Label\nB;x;3;ok\n"
    );
    for path in [
        "same.csv",
        "../logical.csv",
        "C:/absolute.csv",
        "/absolute.csv",
    ] {
        let mut parameters = engine::RuntimeParameters::new();
        parameters.insert("Path", Value::String(path.into()))?;
        let output =
            engine::run_with_context(&project, &source(), &context.with_parameters(&parameters))?;
        let docs = format_csv::to_mapped_document_bytes(&schema, &output, "DetailLines", &options)?;
        assert_eq!(
            docs.iter()
                .map(|d| (d.path.as_str(), d.bytes.as_slice()))
                .collect::<Vec<_>>(),
            [(path, B), (path, A)]
        );
    }
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("Tick", Value::Bool(true))?;
    let counter = context
        .with_parameters(&parameters)
        .with_invocation_counter_limits(engine::InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 16,
        });
    for _ in 0..2 {
        let output = engine::run_with_context(&project, &source(), &counter)?;
        let docs = format_csv::to_mapped_document_bytes(&schema, &output, "DetailLines", &options)?;
        assert_eq!(
            docs.iter()
                .map(|d| (d.path.as_str(), d.bytes.as_slice()))
                .collect::<Vec<_>>(),
            [
                (
                    "out/B-3.csv",
                    b"\xef\xbb\xbfClient;SKU;Total;Label\nB;x;5;1\nB;y;6;2\n".as_slice()
                ),
                (
                    "out/A-6.csv",
                    b"\xef\xbb\xbfClient;SKU;Total;Label\nA;x;12;4\nA;z;-2;5\n"
                )
            ]
        );
    }
    Ok(())
}

#[test]
fn mapped_csv_documents_admission_is_explicit_and_legacy_output_stays_separate() -> TestResult<()> {
    let project = project(false);
    let program = codegen::lower(&project)?;
    let policy = CsvMappedDocumentOutputPolicy::from_project(&project, "DetailLines")?;
    codegen::validate_csv_mapped_document_output(&program, &policy)?;
    assert!(matches!(
        codegen::validate_csv_document_output(&program, &policy.target),
        Err(CsvOutputError::PrimaryRows)
    ));
    for selector in ["", "Rows", "detailLines", "DetailLines/Client"] {
        let bad = CsvMappedDocumentOutputPolicy {
            rows_field: selector.into(),
            ..policy.clone()
        };
        assert!(codegen::validate_csv_mapped_document_output(&program, &bad).is_err());
    }
    let mut constrained = program.clone();
    let SchemaKind::Group { children, .. } = &mut constrained.target.kind else {
        unreachable!()
    };
    let SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    children[3].fixed = Some("OK".into());
    constrained
        .expressions
        .iter_mut()
        .find(|expression| expression.id == 17)
        .unwrap()
        .expression = codegen::Expression::Const {
        value: Value::String("BAD".into()),
    };
    assert!(matches!(
        codegen::validate_csv_mapped_document_output(&constrained, &policy),
        Err(CsvMappedDocumentOutputError::Schema { .. })
    ));
    // Even an unbound fixed field is refused, never synthesized by a descriptor.
    constrained.root.children[0].bindings.remove(0);
    assert!(codegen::validate_csv_mapped_document_output(&constrained, &policy).is_err());
    let mut extra = program.clone();
    let SchemaKind::Group { children, .. } = &mut extra.target.kind else {
        unreachable!()
    };
    children.push(text("Spare"));
    assert!(codegen::validate_csv_mapped_document_output(&extra, &policy).is_err());
    let mut invalid = program.clone();
    invalid.root.children[0].bindings[0].expression = 999;
    let bad = CsvMappedDocumentOutputPolicy {
        target: codegen::CsvOutputPolicy {
            delimiter: Some('\n'),
            ..policy.target.clone()
        },
        ..policy.clone()
    };
    assert!(matches!(
        codegen::validate_csv_mapped_document_output(&invalid, &bad),
        Err(CsvMappedDocumentOutputError::Output(
            CsvOutputError::InvalidProgram(_)
        ))
    ));
    let mut options = project.target_options.clone();
    options.xml_document = true;
    let mut conflicts = project.clone();
    conflicts.target_options = options;
    assert!(matches!(
        CsvMappedDocumentOutputPolicy::from_project(&conflicts, "DetailLines"),
        Err(CsvMappedDocumentOutputError::Output(
            CsvOutputError::ConflictingFormatOptions
        ))
    ));
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let rust_options = codegen_rust::Options {
        package_name: "mapped-host-lib".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.to_string_lossy().into()),
    };
    let artifacts =
        codegen_rust::emit_with_mapped_csv_document_output(&program, &rust_options, &policy)?;
    let library = std::str::from_utf8(
        &artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap()
            .contents,
    )?;
    let adapters = library.split("fn csv_output_schema()").nth(1).unwrap();
    assert_eq!(
        adapters.matches("let output = execute(source)?;").count(),
        2
    );
    assert_eq!(
        adapters
            .matches("let output = execute_with_context(source, execution)?;")
            .count(),
        2
    );
    assert!(!adapters.contains("execute_csv_documents("));
    let csharp = codegen_csharp::emit_with_mapped_csv_document_output(&program, &policy)?;
    let adapter = std::str::from_utf8(
        &csharp
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.MappedCsvDocuments.cs")
            .unwrap()
            .contents,
    )?;
    assert_eq!(adapter.matches("var output = Execute(source);").count(), 2);
    assert_eq!(
        adapter
            .matches("var output = Execute(source, executionContext);")
            .count(),
        2
    );
    assert_eq!(
        serde_json::to_string(&codegen::GenerationProfile::MappedCsvDocumentOutput)?,
        "\"mapped_csv_document_output\""
    );
    // This is an admission-separation control, not an emitted-body freeze.
    // The old csv_api/csv_output and compiled csv_document_output regression
    // suites independently own the legacy emitted-settings/API checks.
    let legacy_project = super::csv_document_output::project()?;
    let legacy = codegen::lower(&legacy_project)?;
    let legacy_policy =
        codegen::CsvOutputPolicy::from_format_options(&legacy_project.target_options)?;
    codegen::validate_csv_document_output(&legacy, &legacy_policy)?;
    assert!(codegen::validate_csv_mapped_document_output(&legacy, &policy).is_err());
    Ok(())
}

fn write_artifacts(root: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for artifact in artifacts.files() {
        let path = root.join(artifact.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, &artifact.contents)?;
    }
    Ok(())
}

// Fresh generated workspaces have no lockfile. Seed exactly the reachable
// checked-in dependency graph, adding only these two package-free host nodes.
// Normalize only the fresh host graph offline, then verify all pins before any
// locked execution. Keep both locks and the normalization command as evidence.
pub(super) fn seed_host_lock(
    host: &Path,
    host_name: &str,
    library_name: &str,
) -> TestResult<String> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock: toml::Value = toml::from_str(&std::fs::read_to_string(workspace)?)?;
    let packages = lock["package"]
        .as_array()
        .ok_or("workspace lock packages")?;
    let mut pending = vec!["codegen-runtime".to_owned(), "format-csv".to_owned()];
    let mut selected = BTreeMap::new();
    while let Some(dependency) = pending.pop() {
        let words = dependency.split_whitespace().collect::<Vec<_>>();
        let found = packages
            .iter()
            .filter(|package| {
                package["name"].as_str() == words.first().copied()
                    && words
                        .get(1)
                        .is_none_or(|version| package["version"].as_str() == Some(*version))
                    && words.get(2).is_none_or(|source| {
                        package.get("source").and_then(toml::Value::as_str)
                            == Some(source.trim_matches(['(', ')']))
                    })
            })
            .collect::<Vec<_>>();
        if found.len() != 1 {
            return Err(format!("ambiguous checked-in dependency: {dependency}").into());
        }
        let package = found[0];
        let key = (
            package["name"].as_str().ok_or("package name")?.to_owned(),
            package["version"]
                .as_str()
                .ok_or("package version")?
                .to_owned(),
        );
        if selected.contains_key(&key) {
            continue;
        }
        if let Some(dependencies) = package.get("dependencies").and_then(toml::Value::as_array) {
            pending.extend(
                dependencies
                    .iter()
                    .map(|dependency| {
                        dependency
                            .as_str()
                            .ok_or("lock dependency")
                            .map(str::to_owned)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        selected.insert(key, package.clone());
    }
    for directory in [host.join("../rust"), host.to_path_buf()] {
        let manifest: toml::Value =
            toml::from_str(&std::fs::read_to_string(directory.join("Cargo.toml"))?)?;
        let name = manifest["package"]["name"].as_str().ok_or("host name")?;
        let version = manifest["package"]["version"]
            .as_str()
            .ok_or("host version")?;
        if name != host_name && name != library_name {
            return Err("unexpected host package".into());
        }
        let mut dependencies = manifest["dependencies"]
            .as_table()
            .ok_or("host dependencies")?
            .iter()
            .map(|(name, value)| {
                toml::Value::String(
                    value
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(name)
                        .into(),
                )
            })
            .collect::<Vec<_>>();
        dependencies.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        let package = toml::Value::Table(toml::map::Map::from_iter([
            ("name".into(), toml::Value::String(name.into())),
            ("version".into(), toml::Value::String(version.into())),
            ("dependencies".into(), toml::Value::Array(dependencies)),
        ]));
        selected.insert((name.into(), version.into()), package);
    }
    // The existing toml dependency is parse-only. Cargo lock package fields are
    // strings or arrays of strings, whose JSON spelling is also valid TOML.
    let mut body = format!(
        "version = {}\n",
        lock["version"].as_integer().ok_or("lock version")?
    );
    for package in selected.into_values() {
        body.push_str("\n[[package]]\n");
        for (key, value) in package.as_table().ok_or("lock package table")? {
            let encoded = match value {
                toml::Value::String(value) => serde_json::to_string(value)?,
                toml::Value::Array(values) => serde_json::to_string(
                    &values
                        .iter()
                        .map(|value| value.as_str().ok_or("string lock dependency"))
                        .collect::<Result<Vec<_>, _>>()?,
                )?,
                _ => return Err("unsupported pinned lock field".into()),
            };
            body.push_str(&format!("{key} = {encoded}\n"));
        }
    }
    std::fs::write(host.join("Cargo.lock"), &body)?;
    std::fs::write(host.join("Cargo.lock.seed"), &body)?;
    let root = host.parent().ok_or("host parent")?;
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let mut compiler_command = Command::new(&compiler);
    compiler_command.arg("-vV").current_dir(host);
    let compiler_output = super::host_policy::recorded_output_with_sources(
        &mut compiler_command,
        root,
        "rust-native-host",
        &["rust".into(), "rust-host".into()],
    )?;
    if !compiler_output.status.success() {
        return Err(NativeHostSetupError::CompilerFailed.into());
    }
    let native_host = parse_native_host(&compiler_output.stdout)?;
    let mut command = Command::new("cargo");
    command
        .args([
            "metadata",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            &native_host,
        ])
        .current_dir(host);
    // Full metadata updates isolated graph edges without replacing compatible
    // seed pins. Filter fetching to the effective compiler's native host;
    // every package in the resulting lock still undergoes the same pin check.
    std::fs::write(
        host.join("native-host-target.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"compiler": compiler.to_string_lossy(), "host": native_host, "metadata_filter_platform": native_host, "consumer_target": native_host}),
        )?,
    )?;
    let output = super::host_policy::recorded_output_with_sources(
        &mut command,
        root,
        "rust-lock-bootstrap",
        &["rust".into(), "rust-host".into()],
    )?;
    if host.join("Cargo.lock").exists() {
        std::fs::copy(host.join("Cargo.lock"), host.join("Cargo.lock.normalized"))?;
    }
    if !output.status.success() {
        return Err(format!(
            "offline host lock normalization failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let normalized: toml::Value =
        toml::from_str(&std::fs::read_to_string(host.join("Cargo.lock"))?)?;
    let seed: toml::Value = toml::from_str(&body)?;
    let mut receipts = Vec::new();
    let mut verified = true;
    for package in normalized["package"]
        .as_array()
        .ok_or("normalized packages")?
    {
        let name = package["name"].as_str().ok_or("normalized name")?;
        let local = name == host_name || name == library_name;
        let candidates = if local {
            seed["package"].as_array().ok_or("seed packages")?
        } else {
            packages
        };
        let matched = candidates
            .iter()
            .any(|candidate| host_lock_identity_matches(candidate, package))
            && (!local || (package.get("source").is_none() && package.get("checksum").is_none()));
        verified &= matched;
        let comparisons = candidates.iter().filter(|candidate| candidate.get("name") == package.get("name")).map(|candidate| {
            ["name", "version", "source", "checksum"].map(|key| serde_json::json!({"field": key, "expected": candidate.get(key), "actual": package.get(key), "equal": candidate.get(key) == package.get(key)}))
        }).collect::<Vec<_>>();
        receipts.push(serde_json::json!({"name": name, "version": package.get("version"), "source": package.get("source"), "checksum": package.get("checksum"), "authored_local": local, "verified": matched, "comparisons": comparisons}));
    }
    std::fs::write(
        host.join("lock-pin-verification.json"),
        serde_json::to_vec_pretty(&receipts)?,
    )?;
    if !verified {
        return Err("normalized host lock contains a package outside the checked-in pins".into());
    }
    Ok(native_host)
}

#[derive(Debug, PartialEq, Eq)]
enum NativeHostSetupError {
    CompilerFailed,
    OutputTooLarge,
    InvalidUtf8,
    MissingHost,
    DuplicateHost,
    MalformedHost,
}

impl std::fmt::Display for NativeHostSetupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "native host test setup refused: {self:?}")
    }
}

impl std::error::Error for NativeHostSetupError {}

fn parse_native_host(output: &[u8]) -> Result<String, NativeHostSetupError> {
    if output.len() > 16 * 1024 {
        return Err(NativeHostSetupError::OutputTooLarge);
    }
    let output = std::str::from_utf8(output).map_err(|_| NativeHostSetupError::InvalidUtf8)?;
    let mut hosts = output.lines().filter(|line| line.starts_with("host:"));
    let line = hosts.next().ok_or(NativeHostSetupError::MissingHost)?;
    if hosts.next().is_some() {
        return Err(NativeHostSetupError::DuplicateHost);
    }
    let host = line
        .strip_prefix("host: ")
        .ok_or(NativeHostSetupError::MalformedHost)?;
    if host.is_empty()
        || host.len() > 128
        || host.split('-').count() < 3
        || !host.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
    {
        return Err(NativeHostSetupError::MalformedHost);
    }
    Ok(host.to_owned())
}

#[test]
fn host_parser_accepts_native_headers_and_refuses_ambiguous_or_unsafe_output() {
    for host in [
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        for newline in ["\n", "\r\n"] {
            let header = format!(
                "rustc 1.98.1{newline}binary: rustc{newline}host: {host}{newline}release: 1.98.1{newline}"
            );
            assert_eq!(parse_native_host(header.as_bytes()), Ok(host.into()));
        }
    }
    assert_eq!(
        parse_native_host(b"rustc 1.98.1\n"),
        Err(NativeHostSetupError::MissingHost)
    );
    assert_eq!(
        parse_native_host(b"host: x86_64-pc-windows-msvc\nhost: x86_64-pc-windows-gnu\n"),
        Err(NativeHostSetupError::DuplicateHost)
    );
    assert_eq!(
        parse_native_host(&[0xff]),
        Err(NativeHostSetupError::InvalidUtf8)
    );
    assert_eq!(
        parse_native_host(&vec![b'x'; 16 * 1024 + 1]),
        Err(NativeHostSetupError::OutputTooLarge)
    );
    for host in [
        "",
        "x86_64",
        "x86_64--linux",
        "../native-target.json",
        "--target=x86_64-pc-windows-msvc",
        "x86_64-pc-windows-msvc ",
        "x86_64-pc-windows;echo",
        "é-pc-windows",
        "x86_64-pc-win\0dows",
    ] {
        assert_eq!(
            parse_native_host(format!("host: {host}\n").as_bytes()),
            Err(NativeHostSetupError::MalformedHost),
            "{host:?}"
        );
    }
    assert_eq!(
        parse_native_host(format!("host: x86_64-pc-{}\n", "a".repeat(128)).as_bytes()),
        Err(NativeHostSetupError::MalformedHost)
    );
    assert_eq!(
        parse_native_host(b"host:x86_64-pc-windows-msvc\n"),
        Err(NativeHostSetupError::MalformedHost)
    );
}

fn host_lock_identity_matches(candidate: &toml::Value, package: &toml::Value) -> bool {
    ["name", "version", "source", "checksum"]
        .iter()
        .all(|key| candidate.get(key) == package.get(key))
}

#[test]
fn host_lock_pin_guard_refuses_new_packages_and_changed_identity() -> TestResult<()> {
    let pinned: toml::Value = toml::from_str(
        "name = 'pinned'\nversion = '1.0.0'\nsource = 'registry+https://example.invalid/index'\nchecksum = 'original'\n",
    )?;
    assert!(host_lock_identity_matches(&pinned, &pinned));
    for field in ["name", "version", "source", "checksum"] {
        let mut changed = pinned.clone();
        changed[field] = toml::Value::String("unexpected".into());
        assert!(!host_lock_identity_matches(&pinned, &changed), "{field}");
        let mut missing = pinned.clone();
        missing.as_table_mut().ok_or("package table")?.remove(field);
        assert!(
            !host_lock_identity_matches(&pinned, &missing),
            "missing {field}"
        );
    }
    let local: toml::Value = toml::from_str("name = 'host'\nversion = '0.0.0'\n")?;
    assert!(host_lock_identity_matches(&local, &local));
    let mut external = local.clone();
    external.as_table_mut().ok_or("local table")?.insert(
        "source".into(),
        toml::Value::String("registry+https://example.invalid/index".into()),
    );
    assert!(!host_lock_identity_matches(&local, &external));
    Ok(())
}

fn check_manifest(
    artifacts: &codegen::ArtifactSet,
    library: codegen::LibraryIdentity,
    binding: codegen::RuntimeBinding,
) -> TestResult<()> {
    let file = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == codegen::MANIFEST_PATH)
        .ok_or("emitted manifest")?;
    let manifest = codegen::CompatibilityManifest::from_json(std::str::from_utf8(&file.contents)?)?;
    assert_eq!(
        manifest.profile,
        codegen::GenerationProfile::MappedCsvDocumentOutput
    );
    assert_eq!(manifest.library, library);
    assert_eq!(manifest.runtime.binding, binding);
    if manifest.runtime.package == "codegen-runtime" {
        assert_eq!(
            manifest.runtime.identity.as_str(),
            codegen_runtime::RUNTIME_IDENTITY
        );
    } else {
        assert_eq!(manifest.runtime.package, "Ferrule.Runtime");
        let identity = codegen::RuntimeIdentity::of_sources(
            artifacts
                .files()
                .iter()
                .filter(|file| {
                    file.path.as_str().starts_with("Runtime/")
                        && file.path.as_str() != "Runtime/FerruleRuntimeIdentity.cs"
                })
                .map(|file| (file.path.as_str(), file.contents.as_slice())),
        );
        assert_eq!(manifest.runtime.identity, identity);
    }
    Ok(())
}

#[test]
fn mapped_csv_documents_compiled_release_hosts() -> TestResult<()> {
    let directory = TempDir::new("mapped-csv-documents-hosts")?;
    let root = &directory.0;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    for kind in ["clock", "literal", "plain", "apostrophe", "disabled"] {
        let supplied_clock = kind == "clock";
        let variant = root.join(kind);
        let library_name = format!("mapped-host-lib-{kind}");
        let host_name = format!("mapped-host-{kind}");
        let project = project(supplied_clock);
        let mut policy = CsvMappedDocumentOutputPolicy::from_project(&project, "DetailLines")?;
        match kind {
            "plain" => {
                policy.target.has_headers = false;
                policy.target.utf8_bom = false;
            }
            "apostrophe" => {
                policy.target.quote = Some('\'');
                policy.target.utf8_bom = false;
            }
            "disabled" => {
                policy.target.quote_disabled = true;
                policy.target.utf8_bom = false;
            }
            _ => (),
        }
        let program = codegen::lower(&project)?;
        let generated = variant.join("rust");
        let rust_options = codegen_rust::Options {
            package_name: library_name.clone(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_string_lossy().into(),
            ),
        };
        let rust_artifacts = codegen_rust::Generator::new(&rust_options)
            .with_manifest(codegen::ManifestSelection::Included)
            .emit_with_mapped_csv_document_output(&program, &policy)?;
        write_artifacts(&generated, &rust_artifacts)?;
        check_manifest(
            &rust_artifacts,
            codegen::LibraryIdentity::Rust {
                package_name: library_name.clone(),
                crate_name: library_name.replace('-', "_"),
            },
            codegen::RuntimeBinding::Path {
                path: runtime.to_string_lossy().into(),
            },
        )?;
        let host = variant.join("rust-host");
        std::fs::create_dir_all(host.join("src"))?;
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={host_name:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nmapped-host-lib={{package={library_name:?},path=\"../rust\"}}\ncodegen-runtime={{path={}}}\nformat-csv={{path={}}}\n",
                serde_json::to_string(&runtime.to_string_lossy())?,
                serde_json::to_string(
                    &runtime
                        .parent()
                        .unwrap()
                        .join("format-csv")
                        .to_string_lossy()
                )?
            ),
        )?;
        std::fs::write(
            host.join("src/main.rs"),
            include_str!("mapped_csv_documents/Host.rs.txt"),
        )?;
        let native_host = seed_host_lock(&host, &host_name, &library_name)?;
        let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("cargo-target"));
        let mut rust_command = Command::new("cargo");
        rust_command
            .args([
                "run",
                "--release",
                "--quiet",
                "--locked",
                "--offline",
                "--jobs",
                "1",
                "--target",
                &native_host,
                "--",
                kind,
            ])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_BUILD_BUILD_DIR", &target)
            .env(
                "FERRULE_MAPPED_PUBLIC_CALLS",
                variant.join("rust-public-calls.txt"),
            )
            .env("RUSTFLAGS", "-D warnings");
        let rust = super::host_policy::recorded_output_with_sources(
            &mut rust_command,
            &variant,
            "rust-release",
            &["rust".into(), "rust-host".into()],
        )?;
        assert!(
            rust.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&rust.stdout),
            String::from_utf8_lossy(&rust.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&rust.stdout).trim(),
            "mapped documents passed"
        );
        let assembly = format!("Authored.Mapped.{kind}");
        let namespace = format!("Authored.Mapped.{kind}");
        let csharp_options = codegen_csharp::Options {
            identity: codegen_csharp::Identity::new(&assembly, &namespace, "ShipmentMapping")?,
            manifest: codegen::ManifestSelection::Included,
        };
        let csharp_artifacts = codegen_csharp::Generator::new(&csharp_options)
            .emit_with_mapped_csv_document_output(&program, &policy)?;
        write_artifacts(&variant.join("csharp"), &csharp_artifacts)?;
        check_manifest(
            &csharp_artifacts,
            codegen::LibraryIdentity::CSharp {
                assembly_name: assembly.clone(),
                root_namespace: namespace.clone(),
                mapping_class: "ShipmentMapping".into(),
            },
            codegen::RuntimeBinding::Vendored,
        )?;
        let host = variant.join("csharp-host");
        std::fs::create_dir_all(&host)?;
        std::fs::write(
            host.join("Host.csproj"),
            format!(
                r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><EnableTargetingPackDownload>false</EnableTargetingPackDownload><EnableRuntimePackDownload>false</EnableRuntimePackDownload></PropertyGroup><ItemGroup><ProjectReference Include="../csharp/{assembly}.csproj" /></ItemGroup></Project>"#
            ),
        )?;
        std::fs::write(
            host.join("Program.cs"),
            include_str!("mapped_csv_documents/Host.cs.txt").replace(
                "using Ferrule.Generated;",
                &format!("using ImplementationMapping = {namespace}.ShipmentMapping;"),
            ),
        )?;
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
            variant.join("global.json"),
        )?;
        std::fs::write(
            variant.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>",
        )?;
        let mut restore = dotnet_command(root);
        restore
            .args([
                "restore",
                "--disable-parallel",
                "csharp-host/Host.csproj",
                "--locked-mode",
                "--configfile",
                "NuGet.Config",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:BuildInParallel=false",
            ])
            .current_dir(&variant);
        let restored = super::host_policy::recorded_output_with_sources(
            &mut restore,
            &variant,
            "csharp-restore",
            &[
                "csharp".into(),
                "csharp-host".into(),
                "global.json".into(),
                "NuGet.Config".into(),
            ],
        )?;
        assert!(
            restored.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&restored.stdout),
            String::from_utf8_lossy(&restored.stderr)
        );
        let mut csharp_command = dotnet_command(root);
        csharp_command
            .args([
                "run",
                "--no-restore",
                "--project",
                "csharp-host/Host.csproj",
                "--configuration",
                "Release",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                "-p:BuildInParallel=false",
                "--",
                kind,
            ])
            .current_dir(&variant)
            .env(
                "FERRULE_MAPPED_PUBLIC_CALLS",
                variant.join("csharp-public-calls.txt"),
            );
        let csharp = super::host_policy::recorded_output_with_sources(
            &mut csharp_command,
            &variant,
            "csharp-release",
            &[
                "csharp".into(),
                "csharp-host".into(),
                "global.json".into(),
                "NuGet.Config".into(),
            ],
        )?;
        assert!(
            csharp.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&csharp.stdout),
            String::from_utf8_lossy(&csharp.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&csharp.stdout).trim(),
            "mapped documents passed"
        );
    }
    Ok(())
}
