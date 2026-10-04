use super::*;
use std::collections::BTreeSet;

const RESOURCES: [&str; 4] = ["SimpleTotal.mfd", "ipo.xml", "ipo.xsd", "address.xsd"];

fn mutable_field<'a>(value: &'a mut Instance, name: &str) -> &'a mut Instance {
    let Instance::Group(fields) = value else {
        panic!("expected group owning {name}")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| key == name)
        .expect("declared field")
        .1
}

fn cases(source: &Instance) -> Vec<(&'static str, Instance, Option<&'static str>)> {
    let mut empty = source.clone();
    *mutable_field(mutable_field(&mut empty, "Items"), "item") = Instance::Repeated(Vec::new());
    let mut missing = source.clone();
    let Instance::Repeated(items) = mutable_field(mutable_field(&mut missing, "Items"), "item")
    else {
        panic!("item collection")
    };
    *mutable_field(&mut items[0], "price") = Instance::Scalar(Value::Null);
    let mut changed = source.clone();
    let Instance::Repeated(items) = mutable_field(mutable_field(&mut changed, "Items"), "item")
    else {
        panic!("item collection")
    };
    *mutable_field(&mut items[0], "quantity") = Instance::Scalar(Value::Int(4));
    vec![
        ("nominal", source.clone(), Some("7056.3")),
        ("empty-items", empty, Some("0")),
        ("missing-first-price", missing, None),
        ("changed-first-quantity", changed, Some("7256.200000000001")),
    ]
}

fn assert_engine(project: &Project, source: &Instance, expected: Option<&str>) -> TestResult<()> {
    match (expected, engine::run(project, source)) {
        (Some(expected), Ok(output)) => {
            let typed = if expected == "0" {
                Value::Int(0)
            } else {
                Value::Float(expected.parse()?)
            };
            assert_eq!(output.field("total"), Some(&Instance::Scalar(typed)));
            let serialized = format_json::to_string(&project.target, &output)?;
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&serialized)?,
                serde_json::json!({"total": expected})
            );
        }
        (
            None,
            Err(engine::EngineError::Function(codegen_runtime::FunctionError::TypeMismatch {
                function: "multiply",
                got: "null",
            })),
        ) => {}
        (expected, result) => {
            return Err(io::Error::other(format!(
                "scalar total expectation {expected:?}: {result:?}"
            ))
            .into());
        }
    }
    Ok(())
}

fn assert_host(
    output: &Output,
    expected: Option<&str>,
    language: &str,
    requested_case: &str,
) -> TestResult<()> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{language} scalar total host: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let mut calls = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    let mut expected_apis = [
        "json",
        "json-context",
        "bytes",
        "bytes-context",
        "typed-json",
        "typed-json-context",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    if language == "Rust" {
        expected_apis.extend([
            "typed-native-XML".into(),
            "typed-native-XML-context".into(),
            "interpreter-json".into(),
            "interpreter-native-XML".into(),
        ]);
    }
    let expected_consumed = if language == "Rust" {
        ["consumed-json-fields", "consumed-native-XML-fields"].as_slice()
    } else {
        ["consumed-json-fields"].as_slice()
    }
    .iter()
    .map(|value| (*value).to_owned())
    .collect::<BTreeSet<_>>();
    for line in String::from_utf8(output.stdout.clone())?.lines() {
        let row: serde_json::Value = serde_json::from_str(line)?;
        let api = row["api"]
            .as_str()
            .ok_or_else(|| io::Error::other("host API identity"))?;
        assert_eq!(row["case"], requested_case);
        if api.starts_with("consumed-") {
            assert!(
                consumed.insert(api.to_owned()),
                "duplicate consumed-source diagnostic {api}"
            );
            let fields = row["result"]["fields"]
                .as_array()
                .expect("consumed item fields");
            let count = if requested_case == "empty-items" {
                0
            } else {
                6
            };
            assert_eq!(row["result"]["count"], count);
            assert_eq!(fields.len(), count);
            let quantities = [
                if requested_case == "changed-first-quantity" {
                    4
                } else {
                    2
                },
                1,
                7,
                3,
                1,
                5,
            ];
            let prices = [99.95_f64, 248.90, 79.90, 89.90, 4879.00, 179.90];
            for (index, field) in fields.iter().enumerate() {
                assert_eq!(field["quantity_tag"], "Int");
                assert_eq!(field["quantity_value"], quantities[index]);
                if requested_case == "missing-first-price" && index == 0 {
                    assert_eq!(field["price_tag"], "Null");
                    assert!(field["price_f64_bits"].is_null());
                } else {
                    assert_eq!(field["price_tag"], "Float");
                    assert_eq!(
                        field["price_f64_bits"],
                        format!("{:016x}", prices[index].to_bits())
                    );
                }
            }
            continue;
        }
        assert!(matches!(
            api,
            "json"
                | "json-context"
                | "bytes"
                | "bytes-context"
                | "typed-json"
                | "typed-json-context"
                | "typed-native-XML"
                | "typed-native-XML-context"
                | "interpreter-json"
                | "interpreter-native-XML"
        ));
        assert!(
            calls.insert(api.to_owned()),
            "duplicate generated API call {api}"
        );
        let result = &row["result"];
        match expected {
            Some(expected) => {
                assert_eq!(result["kind"], "ok", "{language}/{api}: {result}");
                let parsed: serde_json::Value = serde_json::from_str(
                    result["json"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("host JSON result"))?,
                )?;
                assert_eq!(parsed, serde_json::json!({"total": expected}));
                if api.starts_with("typed-") || api.starts_with("interpreter-") {
                    if requested_case == "empty-items" {
                        assert_eq!(result["target_tag"], "Int");
                        assert_eq!(result["target_int"], 0);
                        assert!(result["target_f64_bits"].is_null());
                    } else {
                        assert_eq!(result["target_tag"], "Float");
                        assert!(result["target_int"].is_null());
                        assert_eq!(
                            result["target_f64_bits"],
                            format!("{:016x}", expected.parse::<f64>()?.to_bits())
                        );
                    }
                }
            }
            None => {
                assert_eq!(result["kind"], "error");
                assert_eq!(result["category"], "FunctionType");
                assert_eq!(result["function"], "multiply");
                assert_eq!(
                    result["found"]
                        .as_str()
                        .expect("typed null failure")
                        .to_ascii_lowercase(),
                    "null"
                );
                assert!(
                    result.get("json").is_none(),
                    "failure must return no output"
                );
            }
        }
    }
    assert_eq!(calls, expected_apis);
    assert_eq!(consumed, expected_consumed);
    Ok(())
}

#[test]
#[ignore = "requires the local ignored ReferenceSamples corpus and .NET 10 SDK"]
fn scalar_total_corpus_executes_computed_sum_frames_in_both_languages() -> TestResult<()> {
    let original = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let directory = TempDir::new("scalar_total_corpus")?;
    let package = directory.0.join("package");
    std::fs::create_dir(&package)?;
    let before = RESOURCES
        .iter()
        .map(|name| Ok((*name, std::fs::read(original.join(name))?)))
        .collect::<io::Result<Vec<_>>>()?;
    for (name, bytes) in &before {
        std::fs::write(package.join(name), bytes)?;
    }
    let imported = mfd::import_with_options(
        &package.join("SimpleTotal.mfd"),
        &mfd::ImportOptions::default().with_package_root(&package),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = imported.project;
    assert!(engine::validate(&project).is_empty());
    codegen::lower(&project)?;
    let aggregate = project
        .graph
        .nodes
        .iter()
        .find_map(|(id, node)| match node {
            Node::Aggregate {
                function,
                collection,
                expression: Some(expression),
                ..
            } if *function == mapping::AggregateOp::Sum
                && collection.as_slice() == ["Items", "item"] =>
            {
                Some((*id, *expression))
            }
            _ => None,
        })
        .expect("computed sum over the item frame");
    let Node::Call { function, args } = &project.graph.nodes[&aggregate.1] else {
        panic!("sum expression is a call")
    };
    assert_eq!(function, "multiply");
    assert_eq!(args.len(), 2);
    for (id, field) in args.iter().zip(["quantity", "price"]) {
        assert!(
            matches!(&project.graph.nodes[id], Node::SourceField { path, frame: Some(frame) } if path == &[field] && frame == &["Items", "item"])
        );
    }
    assert!(
        project
            .root
            .bindings
            .iter()
            .any(|binding| binding.target_field == "total" && binding.node == aggregate.0)
    );
    let source = format_xml::read(&package.join("ipo.xml"), &project.source)?;
    let project_path = directory.0.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let source_schema = directory.0.join("source-schema.json");
    let target_schema = directory.0.join("target-schema.json");
    std::fs::write(
        &source_schema,
        codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?,
    )?;
    std::fs::write(
        &target_schema,
        codegen::serialize_embedded_schema(
            &project.target,
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        )?,
    )?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let rust = directory.0.join("rust");
    generate_project(
        &project_path,
        &rust,
        GenerateTarget::Rust {
            runtime_path: runtime.clone(),
        },
    )?;
    let rust_host = directory.0.join("rust-host");
    std::fs::create_dir_all(rust_host.join("src"))?;
    std::fs::write(
        rust_host.join("src/main.rs"),
        include_str!("fixtures/scalar_total_rust_harness.rs.txt"),
    )?;
    std::fs::write(
        rust_host.join("Cargo.toml"),
        format!(
            "[package]\nname = \"ferrule-scalar-total-host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nferrule-generated-mapping = {{ path = {:?} }}\ncodegen-runtime = {{ path = {:?} }}\nir = {{ path = {:?} }}\nformat-xml = {{ path = {:?} }}\nengine = {{ path = {:?} }}\nmapping-ir = {{ package = \"mapping\", path = {:?} }}\nserde_json = \"1.0\"\n[workspace]\n",
            rust,
            runtime,
            runtime.join("../ir"),
            runtime.join("../format-xml"),
            runtime.join("../engine"),
            runtime.join("../mapping")
        ),
    )?;
    let target = directory.0.join("rust-target");
    let build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_host)
        .env("CARGO_TARGET_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let csharp = directory.0.join("csharp");
    generate_project(&project_path, &csharp, GenerateTarget::CSharp)?;
    let harness = csharp.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/scalar_total_csharp_harness.cs.txt"),
    )?;
    std::fs::write(
        harness.join("Harness.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../Ferrule.Generated.csproj\" /></ItemGroup></Project>",
    )?;
    let build = dotnet_command(&csharp)
        .args([
            "build",
            "--configuration",
            "Release",
            "--nologo",
            "--verbosity",
            "quiet",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp)
        .isolated_output()?;
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    for (name, input, expected) in cases(&source) {
        assert_engine(&project, &input, expected)?;
        let folder = directory.0.join(name);
        std::fs::create_dir(&folder)?;
        let xml = folder.join("input.xml");
        let json = folder.join("input.json");
        std::fs::write(&xml, format_xml::to_string(&project.source, &input)?)?;
        std::fs::write(&json, format_json::to_string(&project.source, &input)?)?;
        assert_engine(
            &project,
            &format_xml::read(&xml, &project.source)?,
            expected,
        )?;
        // Mapping output is the transport invariant. Nested XML type-origin
        // metadata is outside the ordinary JSON adapter's source contract.
        assert_engine(
            &project,
            &format_json::from_str(&std::fs::read_to_string(&json)?, &project.source)?,
            expected,
        )?;
        let output = Command::new("cargo")
            .args(["run", "--quiet", "--"])
            .arg(name)
            .arg(&json)
            .arg(&xml)
            .arg(&source_schema)
            .arg(&target_schema)
            .arg(&project_path)
            .current_dir(&rust_host)
            .env("CARGO_TARGET_DIR", &target)
            .env("RUSTFLAGS", "-D warnings")
            .isolated_output()?;
        assert_host(&output, expected, "Rust", name)?;
        let output = dotnet_command(&csharp)
            .args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "--configuration",
                "Release",
                "--no-build",
                "--no-restore",
                "--",
            ])
            .arg(name)
            .arg(&json)
            .arg(&source_schema)
            .arg(&target_schema)
            .arg(&project_path)
            .current_dir(&csharp)
            .isolated_output()?;
        assert_host(&output, expected, "CSharp", name)?;
    }
    for (name, bytes) in before {
        assert_eq!(std::fs::read(original.join(name))?, bytes);
        assert_eq!(std::fs::read(package.join(name))?, bytes);
    }
    Ok(())
}
