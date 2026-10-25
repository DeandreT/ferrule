use super::primary_documents_harness::{self as harness, Cohort};
use super::primary_filename_documents::projects as csv_projects;
use super::*;

#[path = "../../../../conformance/primary-fixed-width-documents/projects.rs"]
mod projects;

fn emit(
    program: &codegen::DocumentProgram,
    language: &str,
    options: &codegen_rust::Options,
) -> TestResult<codegen::ArtifactSet> {
    Ok(if language == "rust" {
        codegen_rust::emit_with_primary_fixed_width_documents(program, options)?
    } else {
        codegen_csharp::emit_with_primary_fixed_width_documents(program)?
    })
}

static RUST_HOST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    include_str!("fixtures/primary_xml_documents_rust.rs.txt")
        .replace("PrimaryXmlDocumentError", "PrimaryFixedWidthDocumentError")
        .replace("execute_primary_xml_document_bytes", "execute_primary_fixed_width_document_bytes")
        .replace("\"xml\"", "\"fixed_width\"")
        .replace("None => (\"output\", (\"other\", error.to_string())),", "None => match &error { codegen_runtime::FixedWidthBoundaryError::Format(codegen_runtime::CsvFormatError::FixedWidthFieldOverflow { field, .. }) => (\"output\", (\"field_overflow\", field.clone())), _ => (\"output\", (\"other\", error.to_string())) },")
        .replace("other => return Err(format!(\"unknown model {other}\").into()),", "5 => run!(model5, &execution, &host, maximum),\n            other => return Err(format!(\"unknown model {other}\").into()),")
        .replace("    Ok(())\n}", "    check_boundaries()?;\n    Ok(())\n}") + RUST_CHECKS
});
static CSHARP_HOST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    include_str!("fixtures/primary_xml_documents_csharp.cs.txt")
        .replace(
            "ExecutePrimaryXmlDocumentBytes",
            "ExecutePrimaryFixedWidthDocumentBytes",
        )
        .replace(
            "FerruleXmlOutputSetException",
            "FerruleRuntimeException or FerruleFixedWidthException or FerruleCsvException",
        )
        .replace("\"xml\"", "\"fixed_width\"")
        .replace("return MappingCause(error) is", "if (error is FerruleFixedWidthException { Error: FerruleFixedWidthError.FieldOverflow } width) return [\"output\", \"field_overflow\", width.Field];\n    return MappingCause(error) is")
        .replace(
            "File.WriteAllText(args[2]",
            "if (model == 0) CheckBoundaries();\nFile.WriteAllText(args[2]",
        )
        .replace(
            "sealed class Recording",
            &(CSHARP_CHECKS.to_owned() + "\nsealed class Recording"),
        )
});
const RUST_CHECKS: &str = r#"
fn check_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(model0::parse_fixed_width_bytes("logical", None, b"short\n"), Err(codegen_runtime::FixedWidthBoundaryError::Format(codegen_runtime::CsvFormatError::PartialFixedWidthRecord { record: 0, expected: 8, got: 5 }))));
    assert!(matches!(model0::parse_fixed_width_bytes("logical", None, b"a_one_x_\n"), Err(codegen_runtime::FixedWidthBoundaryError::Format(codegen_runtime::CsvFormatError::Parse { row: 0, field, .. })) if field == "Count"));
    assert!(matches!(model0::parse_fixed_width_bytes("logical", None, b"a_one_x_\nshort\n"), Err(codegen_runtime::FixedWidthBoundaryError::Format(codegen_runtime::CsvFormatError::PartialFixedWidthRecord { record: 1, expected: 8, got: 5 }))));
    let parsed = model0::parse_fixed_width_bytes("logical.bin", Some("/chosen/actual.dat"), "\u{feff}é_😀___2_\r\na_two_3_\n".as_bytes())?;
    let rows = parsed.as_repeated().unwrap();
    for (index, row) in rows.iter().enumerate() {
        let codegen_runtime::Instance::Group(group) = row else { panic!("row") };
        let origin = group.source_origin().unwrap();
        assert_eq!(origin.position(), index+1);
        assert_eq!(origin.document().path(), "logical.bin");
        assert_eq!(origin.document().resolved_path(), Some("/chosen/actual.dat"));
    }
    let schema = codegen_runtime::SchemaNode::group("row", vec![codegen_runtime::SchemaNode::scalar("Text", codegen_runtime::ScalarType::String)]);
    let layout = codegen_runtime::FixedWidthLayout::new(vec![codegen_runtime::FixedFieldWidth::new(u32::MAX).unwrap()], '😀', true, true).unwrap();
    let source = codegen_runtime::Instance::Repeated(vec![codegen_runtime::Instance::Group(vec![("Text".into(), codegen_runtime::Instance::Scalar(codegen_runtime::Value::String("x".into())))].into())]);
    assert!(matches!(codegen_runtime::serialize_fixed_width_bytes(&schema, &layout, &source), Err(codegen_runtime::FixedWidthBoundaryError::OutputTooLarge { .. })));
    let mut budget = codegen_runtime::FixedWidthInputBudget::default();
    for _ in 0..4 { budget.charge(64*1024*1024)?; }
    assert!(matches!(budget.charge(1), Err(codegen_runtime::FixedWidthBoundaryError::InputTooLarge { maximum: 268435456, .. })));
    Ok(())
}
"#;
const CSHARP_CHECKS: &str = r#"
static void CheckBoundaries()
{
    try { GeneratedMapping.ParseFixedWidthBytes("logical", null, Encoding.UTF8.GetBytes("short\n")); throw new Exception("partial record"); }
    catch (FerruleFixedWidthException error) when (error.Error == FerruleFixedWidthError.PartialRecord && error.Record == 0 && error.Expected == 8 && error.Actual == 5) { }
    try { GeneratedMapping.ParseFixedWidthBytes("logical", null, Encoding.UTF8.GetBytes("a_one_x_\n")); throw new Exception("typed value"); }
    catch (FerruleCsvInputException error) when (error.Error == FerruleCsvInputError.Parse && error.Row == 0 && error.Field == "Count") { }
    try { GeneratedMapping.ParseFixedWidthBytes("logical", null, Encoding.UTF8.GetBytes("a_one_x_\nshort\n")); throw new Exception("record validation precedes scalar conversion"); }
    catch (FerruleFixedWidthException error) when (error.Error == FerruleFixedWidthError.PartialRecord && error.Record == 1 && error.Expected == 8 && error.Actual == 5) { }
    foreach (bool absent in new[] { false, true })
    {
        var one = FerruleFixedWidth.ParseBytes(Encoding.UTF8.GetBytes("___"), [new FerruleCsvReadField("Text", FerruleScalarType.String)], new FerruleFixedWidthLayout([3], 95, true, absent), "logical");
        var value = ((FerruleScalar)((FerruleGroup)one.Items[0]).Fields[0].Value).Value;
        if (value.Kind != (absent ? FerruleValueKind.Null : FerruleValueKind.String)) throw new Exception("empty policy");
    }
    var parsed = GeneratedMapping.ParseFixedWidthBytes("logical.bin", "/chosen/actual.dat", Encoding.UTF8.GetBytes("\uFEFFé_😀___2_\r\na_two_3_\n"));
    for (int index = 0; index < parsed.Items.Count; index++)
    {
        var origin = ((FerruleGroup)parsed.Items[index]).SourceOrigin!;
        if (origin.Position != (ulong)index+1 || origin.Document.Path != "logical.bin" || origin.Document.ResolvedPath != "/chosen/actual.dat") throw new Exception("origin");
    }
    var layout = new FerruleFixedWidthLayout([uint.MaxValue], 0x1F600, true, true);
    var source = new FerruleRepeated([new FerruleGroup([new FerruleField("Text", new FerruleScalar(FerruleValue.FromString("x")))])]);
    try { FerruleFixedWidth.SerializeBytes(source, [new FerruleCsvField("Text", FerruleScalarType.String)], layout); throw new Exception("output limit"); }
    catch (FerruleFixedWidthException error) when (error.Error == FerruleFixedWidthError.OutputTooLarge && error.Expected == 64*1024*1024) { }
    var budget = new FerruleFixedWidthInputBudget();
    for (int i = 0; i < 4; i++) budget.Charge(64*1024*1024);
    try { budget.Charge(1); throw new Exception("total limit"); }
    catch (FerruleFixedWidthException error) when (error.Error == FerruleFixedWidthError.InputTooLarge && error.Expected == 256*1024*1024) { }
}
"#;

static COHORT: std::sync::LazyLock<Cohort> = std::sync::LazyLock::new(|| Cohort {
    tag: "primary-fixed-width-documents",
    cases: include_str!("../../../../conformance/primary-fixed-width-documents/cases.json"),
    models: &projects::MODELS,
    document: projects::document,
    emit,
    rust_fixture: &RUST_HOST,
    csharp_fixture: &CSHARP_HOST,
    configured_model: 1,
});

#[test]
fn primary_fixed_width_inventory() -> TestResult<()> {
    harness::check_inventory(&COHORT)
}
#[test]
fn primary_fixed_width_interpreter() -> TestResult<()> {
    harness::check_interpreter(&COHORT)
}
#[test]
fn primary_fixed_width_emission() -> TestResult<()> {
    harness::check_emission(&COHORT)
}
#[test]
fn primary_fixed_width_cli_generation() -> TestResult<()> {
    harness::check_cli_generation(&COHORT)
}
#[test]
fn primary_fixed_width_preflight_and_manifests() -> TestResult<()> {
    for model in ["literal_direct", "parameter_wildcard_map_no_documents"] {
        let base = projects::document(model);
        for kind in 0..5 {
            let mut project = base.project().clone();
            match kind {
                0 => {
                    project.source_options.fixed_width = Some(mapping::FixedWidthLayout::new(
                        vec![mapping::FixedFieldWidth::new(8).unwrap()],
                        '_',
                        true,
                        true,
                    )?)
                }
                1 => project.target.repeating = true,
                2 => project.source_options.json_document = true,
                3 => project.extra_targets.push(mapping::NamedTarget {
                    name: "Other".into(),
                    path: Some("other.dat".into()),
                    schema: project.target.clone(),
                    options: project.target_options.clone(),
                    root: project.root.clone(),
                }),
                _ => project.target_options.csv_quote_disabled = true,
            }
            let document = mapping::DocumentProject::new(project, base.primary_source().clone());
            let expected = document.primary_route().unwrap_err();
            let temp = TempDir::new("fixed-width-preflight")?;
            let host = harness::Recording {
                inner: format_csv::LocalPrimaryFiles::new(&temp.0)?,
                calls: Default::default(),
            };
            assert!(
                matches!(cli::execute_primary_document_with_host(&document, Path::new("project.json"), &host, 10, None), Err(cli::PrimaryDocumentError::Route(error)) if error == expected)
            );
            assert!(host.calls.borrow().is_empty());
            let file = temp.0.join("project.json");
            std::fs::write(
                &file,
                mapping::document_project_file::encode_pretty(&document)?,
            )?;
            for language in ["rust", "csharp"] {
                let output = temp.0.join(language);
                let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
                command
                    .args([
                        "generate",
                        "--primary-documents",
                        "--language",
                        language,
                        "--project",
                    ])
                    .arg(&file)
                    .arg("--out")
                    .arg(&output);
                if language == "rust" {
                    command
                        .arg("--rust-runtime-path")
                        .arg(harness::runtime_path()?);
                }
                assert!(!command.output()?.status.success());
                assert!(!output.exists());
            }
        }
    }
    let program = codegen::lower_document(&projects::document("parameter_wildcard"))?;
    let options = codegen_csharp::Options {
        identity: codegen_csharp::Identity::new(
            "Acme.Documents",
            "Acme.Documents",
            "DocumentMapping",
        )?,
        manifest: codegen::ManifestSelection::Included,
    };
    let cs = codegen_csharp::Generator::new(&options)
        .emit_with_primary_fixed_width_documents(&program)?;
    let rust = harness::rust_options(&COHORT, 0)?;
    let rs = codegen_rust::Generator::new(&rust)
        .with_manifest(codegen::ManifestSelection::Included)
        .emit_with_primary_fixed_width_documents(&program)?;
    for artifacts in [cs, rs] {
        let file = artifacts
            .files()
            .iter()
            .find(|f| f.path.as_str() == "ferrule-manifest.json")
            .unwrap();
        assert_eq!(
            codegen::CompatibilityManifest::from_json(std::str::from_utf8(&file.contents)?)?
                .profile,
            codegen::GenerationProfile::PrimaryFixedWidthDocument
        );
    }
    Ok(())
}

#[test]
#[ignore = "builds generated Rust and C# hosts with cargo and .NET 10"]
fn primary_fixed_width_generated_hosts() -> TestResult<()> {
    harness::check_generated_hosts(&COHORT)
}
