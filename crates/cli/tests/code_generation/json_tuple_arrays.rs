//! The shared JSON Schema tuple-array corpus
//! (`crates/format-json/tests/fixtures/json_tuple_arrays/cases.json`) through
//! generated Rust and C# hosts.
//!
//! Each case model is an identity mapping run with `engine::run`; its native
//! outcomes become the expectations both hosts must reproduce through their
//! embedded version-5 descriptors. One case is also generated as a complete
//! mapping and run through the generated typed and JSON entry points.

use super::*;
use format_json::JsonFormatError;
use mapping::ScopeConstruction;
use serde_json::{Value as Json, json};

const CORPUS: &str =
    include_str!("../../../format-json/tests/fixtures/json_tuple_arrays/cases.json");
/// The case that is also generated as a complete mapping.
const MAPPED_CASE: &str = "draft-2019-09-frame-properties";

fn typed(value: &Json) -> TestResult<Instance> {
    Ok(match value["kind"].as_str() {
        Some("Repeated") => Instance::Repeated(
            value["items"]
                .as_array()
                .ok_or("repeated items")?
                .iter()
                .map(typed)
                .collect::<TestResult<_>>()?,
        ),
        Some("Group") => Instance::Group(
            value["fields"]
                .as_array()
                .ok_or("group fields")?
                .iter()
                .map(|field| {
                    Ok((
                        field["name"].as_str().ok_or("field name")?.to_owned(),
                        typed(&field["value"])?,
                    ))
                })
                .collect::<TestResult<Vec<_>>>()?
                .into(),
        ),
        Some("Scalar") => {
            let scalar = &value["value"];
            Instance::Scalar(match scalar["kind"].as_str() {
                Some("Int") => Value::Int(scalar["value"].as_i64().ok_or("int")?),
                Some("String") => Value::String(scalar["value"].as_str().ok_or("text")?.into()),
                Some("Bool") => Value::Bool(scalar["value"].as_bool().ok_or("bool")?),
                Some("Float") => Value::Float(scalar["value"].as_f64().ok_or("float")?),
                Some("JsonNull") => Value::json_null(),
                Some("Null") => Value::Null,
                _ => return Err(format!("closed authored scalar kind, got {scalar}").into()),
            })
        }
        _ => return Err(format!("closed authored instance kind, got {value}").into()),
    })
}

/// The inverse of [`typed`], for instances the engine produced.
fn literal(instance: &Instance) -> TestResult<Json> {
    Ok(match instance {
        Instance::Repeated(items) => json!({
            "kind": "Repeated",
            "items": items.iter().map(literal).collect::<TestResult<Vec<_>>>()?,
        }),
        Instance::Group(fields) => json!({
            "kind": "Group",
            "fields": fields
                .iter()
                .map(|(name, value)| Ok(json!({"name": name, "value": literal(value)?})))
                .collect::<TestResult<Vec<_>>>()?,
        }),
        Instance::Scalar(value) => json!({"kind": "Scalar", "value": match value {
            Value::Int(value) => json!({"kind": "Int", "value": value}),
            Value::String(value) => json!({"kind": "String", "value": value}),
            Value::Bool(value) => json!({"kind": "Bool", "value": value}),
            Value::Float(value) => json!({"kind": "Float", "value": value}),
            Value::JsonNull(_) => json!({"kind": "JsonNull"}),
            Value::Null => json!({"kind": "Null"}),
            other => return Err(format!("unexpected engine scalar {other:?}").into()),
        }}),
        other => return Err(format!("unexpected engine instance {other:?}").into()),
    })
}

/// The corpus record of an error: the innermost error's kind and node, and
/// the one-based position of the outermost positional error.
fn record(error: &JsonFormatError) -> Json {
    let mut position = None;
    let mut error = error;
    while let JsonFormatError::TuplePosition {
        position: at,
        error: inner,
        ..
    } = error
    {
        position.get_or_insert(*at);
        error = inner;
    }
    let (kind, name) = match error {
        JsonFormatError::Shape { name, .. } => ("Shape", name),
        JsonFormatError::TupleLengthMismatch { name, .. } => ("TupleLengthMismatch", name),
        JsonFormatError::TupleGap {
            name,
            position: gap,
        } => {
            position.get_or_insert(*gap);
            ("TupleGap", name)
        }
        JsonFormatError::UndeclaredTupleField { name, .. } => ("UndeclaredTupleField", name),
        JsonFormatError::TupleRootRows { name } => ("TupleRootRows", name),
        JsonFormatError::RangeMismatch { name, .. } => ("RangeMismatch", name),
        JsonFormatError::StringLengthMismatch { name, .. } => ("StringLengthMismatch", name),
        JsonFormatError::ItemCountMismatch { name, .. } => ("ItemCountMismatch", name),
        JsonFormatError::UniqueItemsMismatch { name, .. } => ("UniqueItemsMismatch", name),
        JsonFormatError::MissingRequiredProperty { object, .. } => {
            ("MissingRequiredProperty", object)
        }
        JsonFormatError::RecursiveJsonTupleFrame { node, frame } => {
            return json!({"kind": "RecursiveJsonTupleFrame", "name": node, "frame": frame});
        }
        other => return json!({"unexpected": format!("{other:?}")}),
    };
    json!({"kind": kind, "name": name, "position": position})
}

/// An identity mapping of one case model: the root copies the complete
/// source, or binds each scalar field of each row of an array of tuples.
/// Homogeneous scalar array roots have no identity mapping; their cases are
/// ordinary arrays, which the native codec alone describes.
fn identity(model: &SchemaNode) -> TestResult<Option<Project>> {
    let root = match (&model.kind, model.repeating) {
        (_, false) => Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
        (ir::SchemaKind::Group { children, .. }, true) => Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            bindings: (0..)
                .zip(children)
                .map(|(node, child)| {
                    if child.repeating || !child.is_scalar() {
                        return Err(format!("row field {} is not a scalar", child.name).into());
                    }
                    Ok(Binding {
                        target_field: child.name.clone(),
                        node,
                    })
                })
                .collect::<TestResult<_>>()?,
            ..Scope::default()
        },
        _ => return Ok(None),
    };
    let mut graph = Graph::default();
    for binding in &root.bindings {
        graph.nodes.insert(
            binding.node,
            Node::SourceField {
                path: vec![binding.target_field.clone()],
                frame: None,
            },
        );
    }
    Ok(Some(Project {
        source: model.clone(),
        target: model.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root,
    }))
}

/// The interpreter's result for one source instance, or the instance itself
/// for a model without an identity mapping.
fn engine_run(project: Option<&Project>, instance: &Instance) -> TestResult<Instance> {
    Ok(match project {
        Some(project) => engine::run(project, instance)?,
        None => instance.clone(),
    })
}

/// The written outcome for one instance: its output text, or the native
/// writer's error record and message.
fn engine_write(
    model: &SchemaNode,
    project: Option<&Project>,
    instance: &Instance,
) -> TestResult<Json> {
    let result = engine_run(project, instance)?;
    Ok(match format_json::to_string(model, &result) {
        Ok(output) => json!({"output": output}),
        Err(error) => json!({"error": record(&error), "message": error.to_string()}),
    })
}

/// One case's engine-derived expectations, checked against the corpus.
fn expectations(case: &Json, failures: &mut Vec<String>) -> TestResult<Json> {
    let id = case["id"].as_str().ok_or("case id")?;
    let model: SchemaNode = serde_json::from_value(case["expected_model"].clone())?;
    let project = identity(&model)?;
    if let Some(project) = &project {
        let validation = engine::validate(project);
        if !validation.is_empty() {
            return Err(format!("{id}: identity project is invalid: {validation:?}").into());
        }
    }
    let descriptor =
        codegen::serialize_embedded_schema(&model, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)?;
    if model.contains_json_tuple() != descriptor.starts_with("FERRULE-EMBEDDED-SCHEMA/5\n") {
        return Err(format!("{id}: only frames use version-5 descriptors").into());
    }
    let empty = Vec::new();
    let mut accepted = Vec::new();
    for row in case["accepted"].as_array().unwrap_or(&empty) {
        let wire = row["wire"].as_str().ok_or("wire")?;
        let expected = typed(&row["typed"])?;
        let source = format_json::from_str(wire, &model)?;
        if source != expected {
            failures.push(format!("{id} {wire}: native read differs: {source:?}"));
        }
        let result = engine_run(project.as_ref(), &source)?;
        let output = format_json::to_string(&model, &result)?;
        let corpus: Json =
            serde_json::from_str(row.get("output").and_then(Json::as_str).unwrap_or(wire))?;
        if serde_json::from_str::<Json>(&output)? != corpus {
            failures.push(format!("{id} {wire}: engine output differs: {output}"));
        }
        accepted.push(json!({
            "wire": wire,
            "typed": row["typed"],
            "output": output,
            "mapped": literal(&result)?,
        }));
    }
    let mut rejected = Vec::new();
    for row in case["rejected"].as_array().unwrap_or(&empty) {
        let wire = row["wire"].as_str().ok_or("wire")?;
        match format_json::from_str(wire, &model) {
            Ok(read) => failures.push(format!("{id} {wire}: native read accepted {read:?}")),
            Err(error) => {
                if record(&error) != row["error"] {
                    failures.push(format!("{id} {wire}: native error differs: {error}"));
                }
                rejected.push(json!({
                    "wire": wire,
                    "error": row["error"],
                    "message": error.to_string(),
                }));
            }
        }
    }
    let mut writes = Vec::new();
    let mut write = |typed_literal: &Json, wanted: Json| -> TestResult<()> {
        let instance = typed(typed_literal)?;
        // Root rows against a non-repeating frame root, which no mapping of
        // that root produces, reach only the writer.
        let rows = matches!(instance, Instance::Repeated(_)) && !model.repeating;
        let mapping = project.as_ref().filter(|_| !rows);
        let outcome = engine_write(&model, mapping, &instance)?;
        let matches = match (&outcome["output"], &wanted["output"]) {
            (Json::String(actual), Json::String(expected)) => {
                serde_json::from_str::<Json>(actual)? == serde_json::from_str::<Json>(expected)?
            }
            _ => outcome["error"] == wanted["error"],
        };
        if !matches {
            failures.push(format!(
                "{id}: engine write differs: {outcome} for {wanted}"
            ));
        }
        let mut row = outcome;
        row["typed"] = typed_literal.clone();
        writes.push(row);
        Ok(())
    };
    for row in case["write_only"].as_array().unwrap_or(&empty) {
        write(&row["typed"], row.clone())?;
    }
    if let Some(error) = case.get("json_lines_error") {
        let rows = json!({"kind": "Repeated", "items": [case["accepted"][0]["typed"]]});
        write(&rows, json!({"error": error}))?;
    }
    Ok(json!({
        "id": id,
        "descriptor": descriptor,
        "accepted": accepted,
        "rejected": rejected,
        "write_only": writes,
    }))
}

fn raw_unique_items_precedence_case() -> Json {
    let mut codes = SchemaNode::scalar("codes", ir::ScalarType::Int).repeating();
    codes.json_unique_items = true;
    let frame = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("item1", ir::ScalarType::String),
            SchemaNode::group("item2", vec![codes]),
        ],
    )
    .with_json_tuple(ir::JsonTupleLayout {
        length: ir::ItemCountRange::new(0, Some(2)),
        tail: ir::JsonTupleTail::Closed,
    })
    .expect("heterogeneous frame");
    json!({
        "id": "tuple-raw-unique-items-precedence",
        "expected_model": frame,
        "rejected": [
            {"wire": r#"["a",{"codes":[1,1]}]"#, "error": {
                "kind": "UniqueItemsMismatch", "name": "codes", "position": null
            }},
            {"wire": r#"[0,{"codes":[1,1]}]"#, "error": {
                "kind": "UniqueItemsMismatch", "name": "codes", "position": null
            }},
            {"wire": r#"[0,{"codes":[1,2]}]"#, "error": {
                "kind": "Shape", "name": "item1", "position": 1
            }}
        ]
    })
}

fn tuple_without_unique_items_precedence_case(predicate_only: bool) -> Json {
    let point = SchemaNode::group(
        "point",
        vec![
            SchemaNode::scalar("item1", ir::ScalarType::String),
            SchemaNode::scalar("item2", ir::ScalarType::Int),
        ],
    )
    .with_json_tuple(ir::JsonTupleLayout {
        length: ir::ItemCountRange::new(0, Some(2)),
        tail: ir::JsonTupleTail::Closed,
    })
    .expect("heterogeneous frame");
    let mut next = SchemaNode::group("next", vec![]);
    next.recursive_ref = Some("Root".into());
    next.container_nullable = true;
    let mut model = SchemaNode::group("Root", vec![point, next]);
    if predicate_only {
        let mut codes = SchemaNode::scalar("codes", ir::ScalarType::Int).repeating();
        codes.json_unique_items = true;
        model.json_dependent_schemas =
            ir::JsonDependentSchemaConstraints::new([ir::JsonDependentSchemaConstraint::new(
                "unused",
                ir::JsonSchemaPredicate::schema(SchemaNode::group("Predicate", vec![codes])),
            )]);
    }
    let mut nested = "null".to_owned();
    for _ in 0..65 {
        nested = format!(r#"{{"next":{nested}}}"#);
    }
    let wire = format!(r#"{{"point":[0],"next":{nested}}}"#);
    json!({
        "id": if predicate_only { "tuple-predicate-only-uniqueness-keeps-first-error" }
              else { "tuple-without-unique-items-keeps-first-error" },
        "expected_model": model,
        "rejected": [{"wire": wire, "error": {
            "kind": "Shape", "name": "item1", "position": 1
        }}]
    })
}

/// Recursive frames are refused by the native codec rather than expanded.
fn refusal_expectations(case: &Json, failures: &mut Vec<String>) -> TestResult<Json> {
    let id = case["id"].as_str().ok_or("case id")?;
    let model: SchemaNode = serde_json::from_value(case["expected_model"].clone())?;
    let descriptor =
        codegen::serialize_embedded_schema(&model, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)?;
    let wire = case["wire"].as_str().ok_or("wire")?;
    let empty = json!({"kind": "Group", "fields": []});
    let mut outcomes = Vec::new();
    for (route, result) in [
        ("read", format_json::from_str(wire, &model).map(|_| ())),
        (
            "write",
            format_json::to_string(&model, &typed(&empty)?).map(|_| ()),
        ),
    ] {
        match result {
            Err(error) if record(&error) == case["error"] => {
                outcomes.push(json!({"error": case["error"], "message": error.to_string()}));
            }
            other => failures.push(format!("{id} {route}: native refusal differs: {other:?}")),
        }
    }
    Ok(json!({
        "id": id,
        "descriptor": descriptor,
        "accepted": [],
        "rejected": [{"wire": wire, "error": case["error"], "message": outcomes[0]["message"]}],
        "write_only": [{"typed": empty, "error": case["error"], "message": outcomes[1]["message"]}],
    }))
}

fn run_host(root: &Path, language: &str, project_file: &Path, cases: &[u8]) -> TestResult<Output> {
    let directory = root.join(language);
    let target = if language == "rust" {
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .canonicalize()?,
        }
    } else {
        GenerateTarget::CSharp
    };
    generate_project(project_file, &directory, target)?;
    let mapping = std::fs::read_to_string(directory.join(if language == "rust" {
        "src/lib.rs"
    } else {
        "GeneratedMapping.cs"
    }))?;
    if !mapping.contains("FERRULE-EMBEDDED-SCHEMA/5") {
        return Err(format!("{language}: the mapping omits its version-5 descriptors").into());
    }
    std::fs::write(directory.join("tuple-cases.json"), cases)?;
    let output = if language == "rust" {
        let manifest_path = directory.join("Cargo.toml");
        let manifest = std::fs::read_to_string(&manifest_path)?;
        std::fs::write(
            manifest_path,
            manifest.replace(
                "\n[workspace]",
                "\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n\n[workspace]",
            ),
        )?;
        std::fs::write(
            directory.join("src/main.rs"),
            include_str!("json_tuple_arrays/Host.rs.txt"),
        )?;
        host_policy::name_generated_host(&directory, "json-tuple-arrays-host")?;
        let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("cargo-target"));
        if !target.is_absolute() {
            return Err("generated host target must be absolute".into());
        }
        let mut run = Command::new("cargo");
        run.args(["run", "--quiet", "--offline", "--jobs", "1", "--"])
            .arg(&directory)
            .current_dir(&directory)
            .env("CARGO_BUILD_BUILD_DIR", &target)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Dwarnings");
        run.isolated_output()?
    } else {
        std::fs::create_dir(directory.join("Harness"))?;
        std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
        std::fs::write(
            directory.join("Harness/Program.cs"),
            include_str!("json_tuple_arrays/Host.cs.txt"),
        )?;
        std::fs::write(
            directory.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>\n",
        )?;
        let mut build = dotnet_command(&directory);
        build
            .args([
                "msbuild",
                "Harness/Host.csproj",
                "-t:Restore;Build",
                "-nologo",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:NuGetAudit=false",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
            ])
            .current_dir(&directory)
            .env("MSBuildEnableWorkloadResolver", "false")
            .env("DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE", "true")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1");
        let built = build.isolated_output()?;
        if !built.status.success() {
            built
        } else {
            let mut run = dotnet_command(&directory);
            run.arg(directory.join("Harness/bin/Debug/net10.0/Host.dll"))
                .arg(&directory)
                .current_dir(&directory);
            run.isolated_output()?
        }
    };
    std::fs::write(root.join(format!("{language}.stdout")), &output.stdout)?;
    std::fs::write(root.join(format!("{language}.stderr")), &output.stderr)?;
    Ok(output)
}

#[test]
fn json_tuple_array_corpus_matches_the_engine_in_generated_rust_and_csharp() -> TestResult<()> {
    let corpus: Json = serde_json::from_str(CORPUS)?;
    let mut failures = Vec::new();
    let mut cases = Vec::new();
    for section in ["cases", "codec_cases"] {
        for case in corpus[section].as_array().ok_or("case list")? {
            cases.push(expectations(case, &mut failures)?);
        }
    }
    cases.push(expectations(
        &raw_unique_items_precedence_case(),
        &mut failures,
    )?);
    for predicate_only in [false, true] {
        cases.push(expectations(
            &tuple_without_unique_items_precedence_case(predicate_only),
            &mut failures,
        )?);
    }
    for case in corpus["codec_refusal_cases"]
        .as_array()
        .ok_or("refusal list")?
    {
        cases.push(refusal_expectations(case, &mut failures)?);
    }
    if !failures.is_empty() {
        return Err(failures.join("\n").into());
    }
    let mapped = corpus["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"] == MAPPED_CASE))
        .ok_or("mapped case")?;
    let project = identity(&serde_json::from_value(mapped["expected_model"].clone())?)?
        .ok_or("the mapped case has an identity mapping")?;
    if let Err(error) = codegen::lower(&project) {
        return Err(format!("the mapped tuple project does not lower: {error:?}").into());
    }

    let root = TempDir::new("json_tuple_arrays")?;
    let project_file = root.0.join("project.json");
    std::fs::write(
        &project_file,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let cases = serde_json::to_vec_pretty(&json!({"mapped": MAPPED_CASE, "cases": cases}))?;
    for language in ["rust", "csharp"] {
        let output = run_host(&root.0, language, &project_file, &cases)?;
        if !output.status.success() {
            return Err(format!(
                "generated {language} tuple-array host failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        eprint!("{}", String::from_utf8_lossy(&output.stdout));
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
