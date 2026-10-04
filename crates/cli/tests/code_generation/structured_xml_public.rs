use super::*;
use std::collections::BTreeSet;

#[path = "fixtures/structured_xml_snapshot.rs.txt"]
mod snapshot;

// Keep failed generated libraries and hosts available for diagnosis.
struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}

impl RegressionDirectory {
    fn new(name: &str) -> io::Result<Self> {
        let directory = TempDir::new(name)?;
        let path = directory.0.clone();
        std::mem::forget(directory);
        Ok(Self {
            path,
            complete: false,
        })
    }
}

impl Drop for RegressionDirectory {
    fn drop(&mut self) {
        if self.complete {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained generated XML test artifacts: {}",
                self.path.display()
            );
        }
    }
}

const CURRENT: &str = "2026-10-03T00:00:00Z";
const NOMINAL: &str = r#"<OrderBatch Code="batch" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><NeedContext>false</NeedContext><Order><Id>101</Id><Label>bolt😀</Label><Price>48</Price><Details><Counter>7</Counter><Maybe xsi:nil="1"/></Details></Order><Order><Id>202</Id><Label>nut&amp;washer</Label><Price>10</Price><Details><Counter>8</Counter><Maybe>6.5</Maybe></Details></Order></OrderBatch>"#;
type Inputs = Vec<(&'static str, String)>;
type Prepared = (Project, PathBuf, PathBuf, Inputs);

fn float(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
}

fn source_field(path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::SourceField {
        path: path.iter().map(|part| (*part).into()).collect(),
        frame: frame.map(|parts| parts.iter().map(|part| (*part).into()).collect()),
    }
}

fn bindings(values: &[(&str, u32)]) -> Vec<Binding> {
    values
        .iter()
        .map(|(name, node)| Binding {
            target_field: (*name).into(),
            node: *node,
        })
        .collect()
}

fn mapping() -> Project {
    let mut text = float("#text");
    text.text = true;
    Project {
        source: SchemaNode::group(
            "OrderBatch",
            vec![
                string("Code").attribute(),
                bool_("NeedContext"),
                float("Forbidden").nillable(),
                SchemaNode::group(
                    "Order",
                    vec![
                        int("Id"),
                        string("Label"),
                        float("Price"),
                        SchemaNode::group(
                            "Details",
                            vec![int("Counter"), float("Maybe").nillable()],
                        ),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Invoices",
            vec![
                string("Code").attribute(),
                string("Stamp"),
                string("Active"),
                string("Main"),
                float("Forbidden"),
                SchemaNode::group(
                    "Article",
                    vec![
                        int("Number"),
                        string("Name"),
                        SchemaNode::group("Stats", vec![int("Counter"), float("Maybe").nillable()]),
                        // Mapping-produced occurrences deliberately do not change
                        // the target declaration's ordinary JSON cardinality.
                        SchemaNode::group(
                            "SinglePrice",
                            vec![text, string("discount").attribute()],
                        ),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (1, source_field(&["Code"], None)),
                (2, source_field(&["NeedContext"], None)),
                (
                    3,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::CurrentDateTime,
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("offline".into()),
                    },
                ),
                (
                    5,
                    Node::If {
                        condition: 2,
                        then: 3,
                        else_: 4,
                    },
                ),
                (6, source_field(&["Forbidden"], None)),
                (
                    7,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::MappingFilePath,
                    },
                ),
                (
                    8,
                    Node::RuntimeValue {
                        value: mapping::RuntimeValue::MainMappingFilePath,
                    },
                ),
                (
                    9,
                    Node::If {
                        condition: 2,
                        then: 7,
                        else_: 4,
                    },
                ),
                (
                    10,
                    Node::If {
                        condition: 2,
                        then: 8,
                        else_: 4,
                    },
                ),
                (11, source_field(&["Id"], Some(&["Order"]))),
                (12, source_field(&["Label"], Some(&["Order"]))),
                (13, source_field(&["Order", "Details", "Counter"], None)),
                (14, source_field(&["Details", "Maybe"], Some(&["Order"]))),
                (15, source_field(&["Price"], Some(&["Order"]))),
                (
                    16,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
                (
                    17,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (18, source_field(&[], None)),
                (
                    19,
                    Node::Const {
                        value: Value::Int(25),
                    },
                ),
                (
                    20,
                    Node::Call {
                        function: "multiply".into(),
                        args: vec![18, 19],
                    },
                ),
                (
                    21,
                    Node::Const {
                        value: Value::Float(100.0),
                    },
                ),
                (
                    22,
                    Node::Call {
                        function: "subtract".into(),
                        args: vec![21, 20],
                    },
                ),
                (
                    23,
                    Node::Call {
                        function: "divide".into(),
                        args: vec![22, 21],
                    },
                ),
                (
                    24,
                    Node::Call {
                        function: "multiply".into(),
                        args: vec![15, 23],
                    },
                ),
                (
                    25,
                    Node::Const {
                        value: Value::String("%".into()),
                    },
                ),
                (
                    26,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![20, 25],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: bindings(&[
                ("Code", 1),
                ("Stamp", 5),
                ("Active", 9),
                ("Main", 10),
                ("Forbidden", 6),
            ]),
            children: vec![Scope {
                target_field: "Article".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                bindings: bindings(&[("Number", 11), ("Name", 12)]),
                children: vec![
                    Scope {
                        target_field: "Stats".into(),
                        bindings: bindings(&[("Counter", 13), ("Maybe", 14)]),
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "SinglePrice".into(),
                        iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Generate {
                            from: Some(16),
                            to: 17,
                            item: 18,
                        }),
                        iteration_output: mapping::IterationOutput::MappedSequence,
                        bindings: bindings(&[("#text", 24), ("discount", 26)]),
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn cases() -> Inputs {
    vec![
        ("nominal", NOMINAL.into()),
        (
            "empty",
            "<OrderBatch Code=\"empty\"><NeedContext>false</NeedContext></OrderBatch>".into(),
        ),
        (
            "context",
            NOMINAL.replace("<NeedContext>false", "<NeedContext>true"),
        ),
        (
            "nonnillable",
            NOMINAL.replace(
                "<NeedContext>false</NeedContext>",
                "<NeedContext>false</NeedContext><Forbidden xsi:nil=\"true\"/>",
            ),
        ),
        (
            "bad-number",
            NOMINAL.replace("<Counter>7</Counter>", "<Counter>1.2</Counter>"),
        ),
        ("missing-price", NOMINAL.replace("<Price>48</Price>", "")),
        (
            "missing-group",
            NOMINAL.replace(
                "<Details><Counter>7</Counter><Maybe xsi:nil=\"1\"/></Details>",
                "",
            ),
        ),
        (
            "missing-second-group",
            NOMINAL.replace(
                "<Details><Counter>8</Counter><Maybe>6.5</Maybe></Details>",
                "",
            ),
        ),
        (
            "qualified-attribute",
            NOMINAL.replace(
                "Code=\"batch\"",
                "xmlns:d=\"urn:ferrule:test:data\" d:Code=\"qualified\" Code=\"plain\"",
            ),
        ),
        (
            "empty-group",
            NOMINAL.replace(
                "<Details><Counter>7</Counter><Maybe xsi:nil=\"1\"/></Details>",
                "<Details/>",
            ),
        ),
    ]
}

fn expected_kind(case: &str, context: bool) -> &'static str {
    match case {
        "bad-number" => "Input",
        "missing-price" => "Mapping",
        "nonnillable" => "Output",
        "context" if !context => "Mapping",
        _ => "ok",
    }
}

fn assert_physical(result: &serde_json::Value, case: &str, project_path: &Path) {
    let physical = &result["physical"];
    assert_eq!(physical["root"], "Invoices");
    assert_eq!(physical["namespace"], "");
    let code = match case {
        "empty" => "empty",
        "qualified-attribute" => "qualified",
        _ => "batch",
    };
    assert_eq!(physical["code"], code);
    assert_eq!(
        physical["stamp"],
        if case == "context" {
            CURRENT
        } else {
            "offline"
        }
    );
    for name in ["active", "main"] {
        assert_eq!(
            physical[name],
            if case == "context" {
                project_path.to_str().expect("UTF8 temp path")
            } else {
                "offline"
            }
        );
    }
    let articles = physical["articles"]
        .as_array()
        .expect("all physical Articles");
    assert_eq!(articles.len(), if case == "empty" { 0 } else { 2 });
    for (index, article) in articles.iter().enumerate() {
        assert_eq!(article["number"], if index == 0 { "101" } else { "202" });
        assert_eq!(
            article["name"],
            if index == 0 { "bolt😀" } else { "nut&washer" }
        );
        let absent = (index == 0 && matches!(case, "missing-group" | "empty-group"))
            || (index == 1 && case == "missing-second-group");
        if absent {
            assert!(article["counter"].is_null());
        } else {
            assert_eq!(article["counter"], if index == 0 { "7" } else { "8" });
        }
        assert_eq!(article["maybe_nil"], index == 0 && !absent);
        if index == 1 && !absent {
            assert_eq!(article["maybe_bits"], format!("{:016x}", 6.5_f64.to_bits()));
        } else {
            assert!(article["maybe_bits"].is_null());
        }
        let quotes = article["quotes"]
            .as_array()
            .expect("all physical quote occurrences");
        assert_eq!(
            quotes.len(),
            4,
            "singular target declaration must preserve mapped occurrences"
        );
        let values = if index == 0 {
            [48.0_f64, 36.0, 24.0, 12.0]
        } else {
            [10.0_f64, 7.5, 5.0, 2.5]
        };
        for (item, quote) in quotes.iter().enumerate() {
            assert_eq!(quote["discount"], format!("{}%", item * 25));
            assert_eq!(quote["bits"], format!("{:016x}", values[item].to_bits()));
            assert!(
                quote["lexical"].as_str().is_some(),
                "retain original numeric lexical"
            );
        }
    }
}

fn assert_rows(
    output: &Output,
    project: &Project,
    project_path: &Path,
    inputs: &[(&str, String)],
) -> TestResult<()> {
    assert!(
        output.status.success(),
        "host stdout:{}\nstderr:{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "unexpected host stderr");
    let rows = String::from_utf8(output.stdout.clone())?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    let context = engine::ExecutionContext::new(project_path).with_current_datetime(CURRENT);
    for row in &rows {
        let case = row["case"].as_str().expect("case identity");
        let api = row["api"].as_str().expect("API identity");
        assert!(
            seen.insert((case.to_owned(), api.to_owned())),
            "duplicate case/API"
        );
        let xml = &inputs
            .iter()
            .find(|(name, _)| *name == case)
            .expect("known case")
            .1;
        let with_context = api.ends_with("context");
        let result = &row["result"];
        match api {
            "xml" | "xml-context" | "bytes" | "bytes-context" => {
                assert_eq!(
                    result["kind"],
                    expected_kind(case, with_context),
                    "{case}/{api}: {result}"
                );
                if result["kind"] == "ok" {
                    assert_physical(result, case, project_path);
                } else {
                    assert_eq!(result["no_output"], true);
                    assert!(result.get("xml").is_none());
                    assert!(result["detail"].as_str().is_some());
                    if case == "context" && !with_context {
                        assert_eq!(result["runtime_error"], "MissingRuntimeValue");
                        assert_eq!(result["runtime_value"], "CurrentDateTime");
                    }
                    if case == "missing-price" {
                        assert_eq!(result["function"], "multiply");
                        assert_eq!(
                            result["found"]
                                .as_str()
                                .expect("null tag")
                                .to_ascii_lowercase(),
                            "null"
                        );
                    }
                }
            }
            "source" | "source-bytes" => match format_xml::from_str(xml, &project.source) {
                Ok(native) => assert_eq!(
                    result["snapshot"],
                    snapshot::snapshot(&native),
                    "{case}/{api}"
                ),
                Err(_) => {
                    assert_eq!(result["kind"], "Input");
                    assert_eq!(result["no_result"], true);
                }
            },
            "typed" | "typed-context" => match format_xml::from_str(xml, &project.source) {
                Ok(native) => {
                    let expected = if with_context {
                        engine::run_with_context(project, &native, &context)
                    } else {
                        engine::run(project, &native)
                    };
                    match expected {
                        Ok(mapped) => assert_eq!(
                            result["snapshot"],
                            snapshot::snapshot(&mapped),
                            "{case}/{api}"
                        ),
                        Err(_) => {
                            assert_eq!(result["kind"], "Mapping");
                            assert_eq!(result["no_result"], true);
                        }
                    }
                }
                Err(_) => {
                    assert_eq!(result["kind"], "blocked_input");
                    assert_eq!(result["attempted"], false);
                }
            },
            "json-cardinality" => {
                assert_eq!(case, "nominal");
                assert_eq!(result["kind"], "InvalidOutput");
                assert_eq!(result["no_output"], true);
                assert!(result.get("json").is_none());
            }
            _ => panic!("unexpected API {api}"),
        }
    }
    let mut expected = inputs
        .iter()
        .flat_map(|(case, _)| {
            [
                "xml",
                "xml-context",
                "bytes",
                "bytes-context",
                "source",
                "source-bytes",
                "typed",
                "typed-context",
            ]
            .into_iter()
            .map(move |api| ((*case).to_owned(), api.to_owned()))
        })
        .collect::<BTreeSet<_>>();
    expected.insert(("nominal".into(), "json-cardinality".into()));
    assert_eq!(seen, expected);
    for (case, _) in inputs {
        let success = rows
            .iter()
            .filter(|row| {
                row["case"] == *case
                    && matches!(
                        row["api"].as_str(),
                        Some("xml" | "xml-context" | "bytes" | "bytes-context")
                    )
                    && row["result"]["kind"] == "ok"
            })
            .collect::<Vec<_>>();
        for row in success.iter().skip(1) {
            assert_eq!(
                row["result"]["xml"], success[0]["result"]["xml"],
                "overload byte agreement for {case}"
            );
        }
    }
    Ok(())
}

fn prepare(directory: &Path, language: &str) -> TestResult<Prepared> {
    let project = mapping();
    assert!(engine::validate(&project).is_empty());
    let lowered = codegen::lower(&project)?;
    assert_eq!(
        lowered
            .xml_boundary
            .as_ref()
            .expect("real lower XML admission")
            .input
            .profile(),
        Some(codegen::XmlInputProfile::Structured)
    );
    let project_path = directory.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let generated = directory.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&project_path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        command.arg("--rust-runtime-path").arg(&runtime);
    }
    let output = command.isolated_output()?;
    assert!(
        output.status.success(),
        "public CLI generation:{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inputs = cases();
    std::fs::write(
        directory.join("cases.json"),
        serde_json::to_vec(
            &inputs
                .iter()
                .map(|(case, xml)| serde_json::json!({"case":case,"xml":xml}))
                .collect::<Vec<_>>(),
        )?,
    )?;
    std::fs::write(
        directory.join("source-schema.json"),
        codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?,
    )?;
    Ok((project, project_path, generated, inputs))
}

#[test]
fn generated_rust_structured_xml_public_apis_preserve_nested_nil_and_all_quotes() -> TestResult<()>
{
    let mut directory = RegressionDirectory::new("structured_xml_rust")?;
    let (project, path, generated, inputs) = prepare(&directory.path, "rust")?;
    let unchanged = artifact_files(&generated)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let host = directory.path.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    std::fs::write(
        host.join("src/main.rs"),
        include_str!("fixtures/structured_xml_public_rust.rs.txt"),
    )?;
    std::fs::write(
        host.join("src/structured_xml_snapshot.rs.txt"),
        include_str!("fixtures/structured_xml_snapshot.rs.txt"),
    )?;
    std::fs::write(
        host.join("Cargo.toml"),
        format!(
            "[package]\nname=\"ferrule-structured-xml-regression-host\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={:?}}}\nserde_json=\"1\"\nroxmltree=\"0.21\"\n",
            runtime.join("../ir")
        ),
    )?;
    // An explicit host cache is separate from the outer Cargo build lock.
    let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            }
        }
        None => directory.path.join("rust-target"),
    };
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--jobs", "1", "--"])
        .arg(directory.path.join("cases.json"))
        .arg(directory.path.join("source-schema.json"))
        .arg(&path)
        .current_dir(&host)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_INCREMENTAL", "0")
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    assert_eq!(
        artifact_files(&generated)?,
        unchanged,
        "generated library must stay untouched"
    );
    assert_rows(&output, &project, &path, &inputs)?;
    directory.complete = true;
    Ok(())
}

#[test]
fn generated_csharp_structured_xml_public_apis_preserve_nested_nil_and_all_quotes() -> TestResult<()>
{
    let mut directory = RegressionDirectory::new("structured_xml_csharp")?;
    let (project, path, generated, inputs) = prepare(&directory.path, "csharp")?;
    let unchanged = artifact_files(&generated)?;
    let host = directory.path.join("host");
    std::fs::create_dir(&host)?;
    std::fs::write(
        host.join("Program.cs"),
        include_str!("fixtures/structured_xml_public_csharp.cs.txt"),
    )?;
    std::fs::write(
        host.join("Host.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
    )?;
    let artifacts = directory.path.join("artifacts");
    let build = dotnet_command(&directory.path)
        .args([
            "build",
            "--nologo",
            "--verbosity",
            "quiet",
            "--configuration",
            "Release",
            "--artifacts-path",
        ])
        .arg(&artifacts)
        .args(["-m:1", "host/Host.csproj"])
        .current_dir(&directory.path)
        .isolated_output()?;
    assert!(
        build.status.success(),
        "C# host build:{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let output = dotnet_command(&directory.path)
        .arg(artifacts.join("bin/Host/release/Host.dll"))
        .arg(directory.path.join("cases.json"))
        .arg(directory.path.join("source-schema.json"))
        .arg(&path)
        .current_dir(&directory.path)
        .isolated_output()?;
    assert_eq!(
        artifact_files(&generated)?,
        unchanged,
        "generated library must stay untouched"
    );
    assert_rows(&output, &project, &path, &inputs)?;
    directory.complete = true;
    Ok(())
}
