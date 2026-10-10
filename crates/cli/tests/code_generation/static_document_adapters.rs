//! Authored native and compiled physical-document selected-target contracts.
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const CASES: &str = include_str!("static_document_adapters/fixtures/host-cases.json");
const HOST: &str = include_str!("static_document_adapters/Host.cs.txt");
const FIXTURES: &[(&str, &[u8])] = &[
    (
        "projects/base.json",
        include_bytes!("static_document_adapters/fixtures/projects/base.json"),
    ),
    (
        "projects/inverse.json",
        include_bytes!("static_document_adapters/fixtures/projects/inverse.json"),
    ),
    (
        "projects/completion.json",
        include_bytes!("static_document_adapters/fixtures/projects/completion.json"),
    ),
    (
        "projects/no_sources.json",
        include_bytes!("static_document_adapters/fixtures/projects/no_sources.json"),
    ),
    ("host-cases.json", CASES.as_bytes()),
    (
        "expected-typed-values.json",
        include_bytes!("static_document_adapters/fixtures/expected-typed-values.json"),
    ),
    (
        "primary-input.json",
        include_bytes!("static_document_adapters/fixtures/primary-input.json"),
    ),
    (
        "bad-code-input.json",
        include_bytes!("static_document_adapters/fixtures/bad-code-input.json"),
    ),
    (
        "supplement-input.json",
        include_bytes!("static_document_adapters/fixtures/supplement-input.json"),
    ),
    (
        "supplement-invalid.json",
        include_bytes!("static_document_adapters/fixtures/supplement-invalid.json"),
    ),
    (
        "unused-input.json",
        include_bytes!("static_document_adapters/fixtures/unused-input.json"),
    ),
    (
        "unused-invalid.json",
        include_bytes!("static_document_adapters/fixtures/unused-invalid.json"),
    ),
    (
        "reference-input.x12",
        include_bytes!("static_document_adapters/fixtures/reference-input.x12"),
    ),
    (
        "reference-invalid.x12",
        include_bytes!("static_document_adapters/fixtures/reference-invalid.x12"),
    ),
    (
        "malformed.json",
        include_bytes!("static_document_adapters/fixtures/malformed.json"),
    ),
    (
        "invalid-utf8.bin",
        include_bytes!("static_document_adapters/fixtures/invalid-utf8.bin"),
    ),
    (
        "expected-primary.json",
        include_bytes!("static_document_adapters/fixtures/expected-primary.json"),
    ),
    (
        "expected-bad-code-primary.json",
        include_bytes!("static_document_adapters/fixtures/expected-bad-code-primary.json"),
    ),
    (
        "expected-audit.json",
        include_bytes!("static_document_adapters/fixtures/expected-audit.json"),
    ),
    (
        "expected-audit-bool-string.json",
        include_bytes!("static_document_adapters/fixtures/expected-audit-bool-string.json"),
    ),
    (
        "expected-no-sources.json",
        include_bytes!("static_document_adapters/fixtures/expected-no-sources.json"),
    ),
    (
        "expected-advice.x12",
        include_bytes!("static_document_adapters/fixtures/expected-advice.x12"),
    ),
    (
        "expected-completion.x12",
        include_bytes!("static_document_adapters/fixtures/expected-completion.x12"),
    ),
];

struct Evidence(PathBuf);
impl Evidence {
    fn new(label: &str) -> TestResult<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-static-documents274-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        eprintln!("STATIC_DOCUMENTS274_ORIGINALS={}", path.display());
        for (name, bytes) in FIXTURES {
            let file = path.join("fixtures").join(name);
            std::fs::create_dir_all(file.parent().ok_or("fixture parent")?)?;
            std::fs::write(file, bytes)?;
        }
        Ok(Self(path))
    }
    fn record(&self, label: &str, original: &impl std::fmt::Debug) -> TestResult<()> {
        std::fs::write(
            self.0.join(format!("{label}-ORIGINAL.txt")),
            format!("{original:#?}\n"),
        )?;
        Ok(())
    }
    fn command(&self, label: &str, command: &mut Command) -> TestResult<Output> {
        self.record(&format!("{label}-COMMAND"), command)?;
        let original = command.isolated_output();
        self.record(&format!("{label}-COMMAND-RESULT"), &original)?;
        let output = original?;
        std::fs::write(self.0.join(format!("{label}-stdout.bin")), &output.stdout)?;
        std::fs::write(self.0.join(format!("{label}-stderr.bin")), &output.stderr)?;
        Ok(output)
    }
}

fn bytes(name: &str) -> TestResult<&'static [u8]> {
    FIXTURES
        .iter()
        .find(|(path, _)| *path == name)
        .map(|(_, bytes)| *bytes)
        .ok_or_else(|| format!("unknown authored literal {name}").into())
}

fn project(family: &str) -> TestResult<Project> {
    Ok(serde_json::from_slice(bytes(&format!(
        "projects/{family}.json"
    ))?)?)
}

fn syntax(options: &mapping::FormatOptions) -> TestResult<format_edi::x12::Separators> {
    let separators = options
        .x12_separators
        .as_ref()
        .ok_or("authored explicit separators")?;
    Ok(format_edi::x12::Separators {
        element: separators.element,
        component: separators.component,
        segment: separators.segment,
        repetition: separators.repetition,
        release: separators.release,
    })
}

fn parse(
    schema: &SchemaNode,
    options: &mapping::FormatOptions,
    input: &[u8],
) -> TestResult<Instance> {
    let text = std::str::from_utf8(input)?;
    if options.edi_kind == Some(mapping::EdiBoundaryKind::X12) {
        Ok(format_edi::x12::from_str_with_separators(
            text,
            schema,
            false,
            Some(syntax(options)?),
        )?)
    } else {
        Ok(format_json::from_str(text, schema)?)
    }
}

fn tagged(value: &Instance) -> Json {
    match value {
        Instance::Group(fields) => {
            let origin = match fields.xml_type_origin() {
                ir::XmlTypeOrigin::Unknown => {
                    json!({"kind":"Unknown","identity":null,"literal":null})
                }
                ir::XmlTypeOrigin::Absent => {
                    json!({"kind":"Absent","identity":null,"literal":null})
                }
                ir::XmlTypeOrigin::Explicit(identity) => {
                    json!({"kind":"Explicit","identity":identity,"literal":null})
                }
                ir::XmlTypeOrigin::ExplicitPadded {
                    literal,
                    resolved_identity,
                } => {
                    json!({"kind":"ExplicitPadded","identity":resolved_identity,"literal":literal})
                }
            };
            json!({"kind":"group","xml_type_origin":origin,"fields":fields.iter().map(|(name,value)| json!({"name":name,"value":tagged(value)})).collect::<Vec<_>>()})
        }
        Instance::Scalar(value) => match value {
            Value::Null => json!({"kind":"Null"}),
            Value::JsonNull(_) => json!({"kind":"JsonNull"}),
            Value::String(value) => json!({"kind":"String","value":value}),
            Value::Int(value) => json!({"kind":"Int64","value":value}),
            Value::Bool(value) => json!({"kind":"Bool","value":value}),
            other => json!({"unqualified":format!("{other:?}")}),
        },
        other => json!({"unqualified":format!("{other:?}")}),
    }
}

fn native_row(evidence: &Evidence, row: &Json) -> TestResult<bool> {
    let id = row["id"].as_str().ok_or("authored case id")?;
    let project = project(row["family"].as_str().ok_or("authored family")?)?;
    evidence.record(&format!("{id}-PROJECT"), &project)?;
    let validation = engine::validate(&project);
    evidence.record(&format!("{id}-VALIDATION"), &validation)?;
    if !validation.is_empty() {
        return Err("authored native project must validate".into());
    }
    let source_original = parse(
        &project.source,
        &project.source_options,
        bytes(row["primary"].as_str().ok_or("authored source")?)?,
    );
    evidence.record(&format!("{id}-PRIMARY-PARSE"), &source_original)?;
    let source = source_original?;
    let mut inputs = Vec::new();
    for entry in row["named_inputs"]
        .as_array()
        .ok_or("authored named list")?
    {
        let name = entry["name"].as_str().ok_or("authored name")?;
        let side = project
            .extra_sources
            .iter()
            .find(|side| side.name == name)
            .ok_or("authored source declaration")?;
        let parsed = parse(
            &side.schema,
            &side.options,
            bytes(entry["document"].as_str().ok_or("authored document")?)?,
        );
        evidence.record(&format!("{id}-{name}-PARSE"), &parsed)?;
        inputs.push((name.to_owned(), parsed?));
    }
    let source_before = source.clone();
    let inputs_before = inputs.clone();
    let context = &row["context"];
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in context["parameters"]
        .as_object()
        .ok_or("authored parameters")?
    {
        let scalar = match value["kind"].as_str() {
            Some("String") => {
                Value::String(value["value"].as_str().ok_or("string parameter")?.into())
            }
            Some("Bool") => Value::Bool(value["value"].as_bool().ok_or("bool parameter")?),
            _ => return Err("unqualified authored parameter".into()),
        };
        parameters.insert(name, scalar)?;
    }
    let mapping_path = Path::new(
        context["mapping_file_path"]
            .as_str()
            .ok_or("mapping path")?,
    );
    let main_path = Path::new(
        context["main_mapping_file_path"]
            .as_str()
            .ok_or("main path")?,
    );
    let mut execution =
        engine::ExecutionContext::with_main_mapping_file_path(mapping_path, main_path)
            .with_parameters(&parameters);
    if let Some(datetime) = context["current_date_time"].as_str() {
        execution = execution.with_current_datetime(datetime);
    }
    let name = row["selection"].as_str().ok_or("selection")?;
    let selection = if name == "Primary" {
        engine::TargetSelection::Primary
    } else {
        engine::TargetSelection::Named(name)
    };
    let original = engine::run_selected_target_with_sources_and_context(
        &project,
        &source,
        inputs.clone(),
        &execution,
        selection,
    );
    evidence.record(&format!("{id}-COMPLETE-SELECTED-OUTCOME"), &original)?;
    evidence.record(&format!("{id}-COMPLETE-INPUTS-AFTER"), &(&source, &inputs))?;
    if row.get("expected_error").is_some() {
        return Ok(
            matches!(original, Err(engine::EngineError::MappingFailure {rule:1,message:Some(ref message)}) if message == "authored stop"),
        );
    }
    let output = original?;
    let (actual, schema, options) = match &output {
        engine::SelectedTargetOutput::Primary(instance) => {
            (instance, &project.target, &project.target_options)
        }
        engine::SelectedTargetOutput::Named(output) => {
            let side = project
                .extra_targets
                .iter()
                .find(|side| side.name == output.name)
                .ok_or("authored target declaration")?;
            (&output.instance, &side.schema, &side.options)
        }
    };
    let before = tagged(actual);
    let serialized = (|| -> TestResult<String> {
        if options.edi_kind == Some(mapping::EdiBoundaryKind::X12) {
            if let Some(mapping::EdiAutocomplete::X12(completion)) = &options.edi_autocomplete {
                Ok(format_edi::x12::to_string_with_syntax_and_autocomplete(
                    schema,
                    actual,
                    syntax(options)?,
                    options.x12_interchange_version.as_deref(),
                    format_edi::x12::Autocomplete {
                        current_datetime: context["current_date_time"]
                            .as_str()
                            .ok_or("completion timestamp")?,
                        request_acknowledgement: completion.request_acknowledgement,
                        transaction_set: completion.transaction_set.as_deref(),
                    },
                )?)
            } else {
                Ok(format_edi::x12::to_string_with_syntax(
                    schema,
                    actual,
                    syntax(options)?,
                    options.x12_interchange_version.as_deref(),
                )?)
            }
        } else {
            Ok(format_json::to_string(schema, actual)?)
        }
    })();
    evidence.record(&format!("{id}-COMPLETE-SERIALIZATION"), &serialized)?;
    let serialized = serialized?;
    let after = tagged(actual);
    let expected_wire = bytes(row["expected_file"].as_str().ok_or("expected wire")?)?;
    evidence.record(
        &format!("{id}-COMPLETE-COMPARISON"),
        &(
            &before,
            &row["expected_typed"],
            &after,
            &serialized,
            &expected_wire,
        ),
    )?;
    Ok(before == row["expected_typed"]
        && before == after
        && serialized.as_bytes() == expected_wire
        && source == source_before
        && inputs == inputs_before)
}

#[test]
fn static_document_adapters_native_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new("native")?;
    let cases: Json = serde_json::from_str(CASES)?;
    let mut results = Vec::new();
    for row in cases
        .as_array()
        .ok_or("authored rows")?
        .iter()
        .filter(|row| {
            row.get("expected_error").is_none()
                || row["id"] == "global-failure-before-selected-context"
        })
    {
        let result = native_row(&evidence, row);
        evidence.record(
            &format!("{}-COMPLETE-RESULT", row["id"].as_str().ok_or("id")?),
            &result,
        )?;
        results.push((row["id"].clone(), result));
    }
    evidence.record("ALL-COMPLETE-NATIVE-RESULTS", &results)?;
    if results.len() != 14
        || results
            .iter()
            .any(|(_, result)| !matches!(result, Ok(true)))
    {
        return Err("complete authored native selected-document outcomes differ".into());
    }
    Ok(())
}

const X12_PATHS: [&str; 8] = [
    "Runtime/X12/FerruleX12.cs",
    "Runtime/X12/FerruleX12.Schema.cs",
    "Runtime/X12/FerruleX12.Reader.cs",
    "Runtime/X12/FerruleX12.Numeric.cs",
    "Runtime/X12/FerruleX12.Writer.cs",
    "Runtime/X12/FerruleX12.Completion.cs",
    "Runtime/X12/FerruleX12.Lexical.cs",
    "Runtime/X12/FerruleX12Exception.cs",
];

#[test]
fn static_document_adapters_compiled_complete_oracles() -> TestResult<()> {
    let evidence = Evidence::new("compiled")?;
    let fixture_root = evidence.0.join("fixtures");
    let mut results = Vec::new();
    for family in ["base", "inverse", "completion", "no_sources"] {
        let directory = evidence.0.join(family);
        let input = fixture_root.join("projects").join(format!("{family}.json"));
        let generated = cli::generate_project_with_static_document_adapters(
            &input,
            &directory,
            GenerateTarget::CSharp,
        );
        evidence.record(&format!("{family}-COMPLETE-GENERATION"), &generated)?;
        generated?;
        let emitted = artifact_files(&directory)?;
        evidence.record(&format!("{family}-COMPLETE-EMITTED-ARTIFACTS"), &emitted)?;
        let mut expected = expected_csharp_artifact_paths(false)
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        expected.extend(X12_PATHS.into_iter().map(str::to_owned));
        expected.insert("GeneratedMapping.Documents.cs".into());
        let actual = emitted
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<BTreeSet<_>>();
        evidence.record(
            &format!("{family}-COMPLETE-PATH-MEMBERSHIP"),
            &(&actual, &expected),
        )?;
        if actual != expected || actual.len() != 86 {
            return Err("complete static companion publish set differs".into());
        }
        let runtime =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/csharp/Ferrule.Runtime");
        let source_bodies = emitted
            .iter()
            .filter(|(path, _)| path.starts_with("Runtime/"))
            .map(|(path, body)| {
                (
                    path.clone(),
                    std::fs::read(runtime.join(path.strip_prefix("Runtime/").unwrap())),
                    body.clone(),
                )
            })
            .collect::<Vec<_>>();
        evidence.record(
            &format!("{family}-COMPLETE-RUNTIME-BODY-READBACK"),
            &source_bodies,
        )?;
        if source_bodies.len() != 82
            || source_bodies
                .iter()
                .any(|(_, source, body)| !matches!(source,Ok(source) if source == body))
        {
            return Err("vendored runtime body differs from source".into());
        }
        std::fs::create_dir(directory.join("Harness"))?;
        std::fs::write(directory.join("Harness/Host.csproj"), HARNESS)?;
        std::fs::write(directory.join("Harness/Program.cs"), HOST)?;
        std::fs::write(
            directory.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>\n",
        )?;
        let bound = artifact_files(&directory)?;
        evidence.record(&format!("{family}-COMPLETE-BOUND-ARTIFACTS"), &bound)?;
        let outcome = (|| -> TestResult<bool> {
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
                .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
                .env("MSBUILDDISABLENODEREUSE", "1");
            if !evidence
                .command(&format!("{family}-BUILD"), &mut build)?
                .status
                .success()
            {
                return Ok(false);
            }
            let mut host = dotnet_command(&directory);
            host.arg(directory.join("Harness/bin/Debug/net10.0/Host.dll"))
                .arg(&fixture_root)
                .arg(family)
                .arg(directory.join("host-evidence"))
                .current_dir(&directory);
            Ok(evidence
                .command(&format!("{family}-HOST"), &mut host)?
                .status
                .success())
        })();
        evidence.record(&format!("{family}-COMPLETE-HOST-RESULT"), &outcome)?;
        let readback = bound
            .iter()
            .map(|(path, _)| std::fs::read(directory.join(path)))
            .collect::<Vec<_>>();
        evidence.record(&format!("{family}-COMPLETE-BOUND-READBACK"), &readback)?;
        let stable = readback
            .iter()
            .zip(&bound)
            .all(|(actual, (_, wanted))| matches!(actual,Ok(actual) if actual == wanted));
        results.push((family, outcome, stable));
    }
    let fixtures_after = FIXTURES
        .iter()
        .map(|(name, _)| std::fs::read(fixture_root.join(name)))
        .collect::<Vec<_>>();
    evidence.record("ALL-COMPLETE-FIXTURE-READBACK", &fixtures_after)?;
    evidence.record("ALL-COMPLETE-COMPILED-RESULTS", &results)?;
    if results
        .iter()
        .any(|(_, result, stable)| !*stable || !matches!(result, Ok(true)))
        || !fixtures_after.iter().zip(FIXTURES).all(
            |(actual, (_, wanted))| matches!(actual,Ok(actual) if actual.as_slice() == *wanted),
        )
    {
        return Err("complete compiled document outcomes or source identity differ".into());
    }
    Ok(())
}

const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
