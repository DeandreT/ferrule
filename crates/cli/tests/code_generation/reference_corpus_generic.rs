//! Opt-in shared JSON host execution of sixty additional local ignored designs.
//! Source transport is exact; output coverage is schema-normalized ordered JSON.
//! Private inputs, expected outputs, and constants are derived at runtime.

use super::*;

#[path = "reference_corpus_generic/admission.rs"]
mod admission;
#[path = "reference_corpus_generic/envelope.rs"]
mod envelope;
#[allow(dead_code)]
#[path = "../../../mfd/tests/support/native_instance_input.rs"]
mod native_input;
#[path = "reference_corpus_generic/package.rs"]
pub(super) mod package;
#[path = "reference_corpus_generic/tests.rs"]
mod tests;

const CASE_FILTER: &str = "FERRULE_GENERIC_CORPUS_CASE";
const CASES: [&str; 60] = [
    "sha256:cbf7033e235190f640f40c8f236d619e30823c7d89ea366393b1f4fbd5751932",
    "sha256:e59cdc5a339a68ab6f4d6d3ab55dbc46332685d411831f7a0858660b3f7174a5",
    "Annual Average Temperature.mfd",
    "BookISBNConvertWS.mfd",
    "BuildHierarchyRecursive.mfd",
    "BuildSchemaTypeHierarchy.mfd",
    "CompletePO.mfd",
    "CompletePO_Join.mfd",
    "Customers_DB.mfd",
    "sha256:9088fefeef4c9776951cc08c00c43b38b9b72e59ae122a62274175b6cbdeec02",
    "sha256:a3346191c50fd7fdfc1c142def1b14c68bf5fd4b3671adc5a23526e5f08a8875",
    "DB_Denormalize.mfd",
    "DB_ManagerList_AllOffices.mfd",
    "DB_MostExpensiveArticle.mfd",
    "DB_UserList.mfd",
    "DistinctArticles.mfd",
    "DividePersonsByDepartmentIntoGroups.mfd",
    "ExcelWith2Dimensions.mfd",
    "Excel_Company_To_JSON.mfd",
    "ExportPersonAddressListToCSV.mfd",
    "FileNamesAsParameters.mfd",
    "GroupTemperaturesByYear_Fahrenheit.mfd",
    "HasMarketingExpenses.mfd",
    "ImportQuotationsFromTextfile.mfd",
    "IniToXML.mfd",
    "Java/FormatNumber.mfd",
    "MarketingAndDailyExpenses.mfd",
    "MarketingExpenses.mfd",
    "MarketingExpenses_DetachedSignature.mfd",
    "MarketingExpenses_EnvelopedSignature.mfd",
    "OrderInUSD.mfd",
    "PersonListsForAllBranchOffices.mfd",
    "PreserveFormatting.mfd",
    "QuotationsDoc.mfd",
    "Sales_to_Excel.mfd",
    "SplitFile.mfd",
    "TimeService/getServerCity.mfd",
    "TimeService/getServerTimeZone.mfd",
    "TimeServiceWsdl2/getServerCity.mfd",
    "TimeServiceWsdl2/getServerTimeZone.mfd",
    "TokenizeString1.mfd",
    "Tutorial/CalculateTax_XQuery.mfd",
    "Tutorial/ConvertProducts.mfd",
    "Tutorial/DatabaseExceptions.mfd",
    "Tutorial/FilterWithPriority.mfd",
    "Tutorial/GardenInvoice.mfd",
    "Tutorial/GroupingFunctions.mfd",
    "Tutorial/Head-detail-inline.mfd",
    "Tutorial/MissingFields.mfd",
    "Tutorial/ParentContext.mfd",
    "Tutorial/ParseString.mfd",
    "Tutorial/ReadJSON.mfd",
    "Tutorial/ReplaceEmptyFields.mfd",
    "Tutorial/Summing-nodes.mfd",
    "Tutorial/Tut-headerDetail.mfd",
    "Tutorial/Tut-xml2csv.mfd",
    "Tutorial/XMLtoSQLite.mfd",
    "Tutorial/YearlySales.mfd",
    "Tutorial/boa-balance-sheet.mfd",
    "Tutorial/getAuthor.mfd",
];

#[test]
#[ignore = "requires the local ignored sample corpus and .NET 10 SDK"]
fn generic_generated_hosts_execute_sixty_additional_local_designs() -> TestResult<()> {
    let original = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let directory = TempDir::new("generic_reference_corpus")?;
    // Import performs SQLite introspection too. Stage before any importer/native reader runs.
    let package = package::Snapshot::copy(&original, &directory.0.join("ReferenceSamples"))?;
    let resolved = package.resolve_cases(&CASES)?;
    let cases: Vec<_> = resolved.iter().map(String::as_str).collect();
    package.preflight_connections(&cases)?;
    let filter = std::env::var(CASE_FILTER).ok();
    check_case_filter(filter.as_deref(), &cases)?;
    let target = directory.0.join("rust-target");
    let mut executed = Vec::new();
    for (index, sample) in cases.iter().enumerate() {
        if filter.as_deref().is_some_and(|filter| filter != *sample) {
            continue;
        }
        let case = directory.0.join(format!("case-{index}"));
        std::fs::create_dir(&case)?;
        run_case(&package.root, sample, &case, &target)?;
        package.verify()?;
        executed.push(*sample);
        eprintln!("generic JSON corpus: {sample}: Rust/C# string and UTF-8 outputs passed");
    }
    if let Some(filter) = filter {
        assert_eq!(executed, vec![filter.as_str()]);
    } else {
        assert_eq!(
            executed.as_slice(),
            cases.as_slice(),
            "all sixty reviewed new designs must execute"
        );
    }
    package.verify()?;
    Ok(())
}

fn check_case_filter(filter: Option<&str>, cases: &[&str]) -> io::Result<()> {
    if filter.is_some_and(|filter| !cases.contains(&filter)) {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "generic corpus filter must name an exact reviewed case",
        ))
    } else {
        Ok(())
    }
}

fn loaded_source(
    root: &Path,
    base: &Path,
    path: &str,
    schema: &SchemaNode,
    options: &mapping::FormatOptions,
) -> TestResult<Instance> {
    if options.local_xml_file_set {
        return Err(admission::AdmissionError::Unsupported("file set").into());
    }
    let path =
        native_input::resolve_sample_input_from(root, base, path).map_err(io::Error::other)?;
    check_source_adapter(&path, options)?;
    native_input::read_instance(&path, schema, options)
        .map_err(|error| io::Error::other(error).into())
}

fn check_source_adapter(
    path: &Path,
    options: &mapping::FormatOptions,
) -> Result<(), admission::AdmissionError> {
    use mapping::{EdiBoundaryKind, ExternalPayloadFormat};
    if options.json5 || options.http_get.is_some() || options.local_xml_file_set {
        return Err(admission::AdmissionError::Unsupported(
            "unhandled source adapter",
        ));
    }
    let external = options.external_source.as_ref();
    if external.is_some_and(|source| {
        matches!(source.payload(), ExternalPayloadFormat::Json) && options.xml_document
            || matches!(source.payload(), ExternalPayloadFormat::Xml)
                && (options.json_document || options.json_lines)
    }) {
        return Err(admission::AdmissionError::Unsupported(
            "conflicting captured document adapter",
        ));
    }
    let structured_xlsx = [
        options.xlsx_hierarchical.is_some(),
        options.xlsx_grid.is_some(),
        options.xlsx_worksheet_set.is_some(),
        options.xlsx_composite.is_some(),
    ]
    .into_iter()
    .filter(|value| *value)
    .count();
    let legacy_xlsx = !options.xlsx_rows.is_empty()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || options.xlsx_update_existing;
    let xlsx = structured_xlsx > 0 || legacy_xlsx;
    if structured_xlsx > 1
        || structured_xlsx > 0 && (legacy_xlsx || options.has_header_row.is_some())
        || options.xlsx_update_existing
        || !options.xlsx_headers.is_empty()
    {
        return Err(admission::AdmissionError::Unsupported(
            "conflicting or ignored worksheet options",
        ));
    }
    if (options.idoc.is_some() || options.swift_mt.is_some())
        && !options.edi_implied_decimals.is_empty()
    {
        return Err(admission::AdmissionError::Unsupported(
            "ignored embedded EDI decimal options",
        ));
    }
    let ordinary_edi =
        options.edi_kind.is_some() && options.idoc.is_none() && options.swift_mt.is_none();
    let csv = options.delimiter.is_some()
        || options.csv_quote.is_some()
        || options.csv_quote_disabled
        || options.csv_preserve_empty_strings
        || options.csv_utf8_bom
        || options.csv_text_repair_dependency.is_some();
    let csv_header = options.has_header_row.is_some() && !xlsx;
    let families = [
        options.xbrl.is_some(),
        options.idoc.is_some(),
        options.swift_mt.is_some(),
        external.is_some(),
        options.pdf.is_some(),
        options.flextext.is_some(),
        options.protobuf.is_some(),
        options.fixed_width.is_some(),
        (options.xml_document || options.wsdl.is_some()) && external.is_none(),
        (options.json_document || options.json_lines) && external.is_none(),
        ordinary_edi,
        xlsx,
    ];
    if (csv || csv_header) && families.iter().any(|family| *family) {
        return Err(admission::AdmissionError::Unsupported(
            "conflicting CSV adapter options",
        ));
    }
    if families.into_iter().filter(|family| *family).count() > 1 {
        return Err(admission::AdmissionError::Unsupported(
            "conflicting source adapters",
        ));
    }
    if options.idoc.is_some()
        && options
            .edi_kind
            .is_some_and(|kind| kind != EdiBoundaryKind::Idoc)
        || options.swift_mt.is_some()
            && options
                .edi_kind
                .is_some_and(|kind| kind != EdiBoundaryKind::SwiftMt)
    {
        return Err(admission::AdmissionError::Unsupported(
            "conflicting embedded EDI adapter",
        ));
    }
    if (options.json_document || options.json_lines) && external.is_none() {
        let extension = native_input::extension_for_dispatch(path, options)
            .map_err(|_| admission::AdmissionError::Unsupported("document suffix"))?;
        if !matches!(extension.as_str(), "json" | "jsonl" | "ndjson") {
            return Err(admission::AdmissionError::Unsupported(
                "ignored JSON document marker",
            ));
        }
    }
    if native_input::extension(path)
        .is_ok_and(|extension| matches!(extension.as_str(), "jsonl" | "ndjson"))
        && !options.json_lines
        && external.is_none()
    {
        return Err(admission::AdmissionError::Unsupported(
            "ignored JSON Lines suffix",
        ));
    }
    if ordinary_edi {
        let extension = native_input::extension_for_dispatch(path, options)
            .map_err(|_| admission::AdmissionError::Unsupported("EDI suffix"))?;
        if !matches!(extension.as_str(), "edi" | "x12" | "edifact" | "hl7") {
            return Err(admission::AdmissionError::Unsupported("ignored EDI marker"));
        }
    }
    if xlsx {
        let extension = native_input::extension_for_dispatch(path, options)
            .map_err(|_| admission::AdmissionError::Unsupported("worksheet suffix"))?;
        if extension != "xlsx" {
            return Err(admission::AdmissionError::Unsupported(
                "ignored worksheet layout",
            ));
        }
    }
    Ok(())
}

fn run_case(root: &Path, sample: &str, case: &Path, rust_target: &Path) -> TestResult<()> {
    let mapping_path = root.join(sample);
    let imported = mfd::import_with_options(
        &mapping_path,
        &mfd::ImportOptions::default().with_package_root(root),
    )?;
    admission::check_warnings(imported.warnings.len())?;
    let project = imported.project;
    let lowered = codegen::lower(&project)?;
    admission::check_program(&lowered)?;
    let base = mapping_path
        .parent()
        .ok_or_else(|| io::Error::other("mapping has no parent"))?;
    let input = project
        .source_path
        .as_deref()
        .ok_or_else(|| io::Error::other("mapping has no stored source path"))?;
    let source = loaded_source(root, base, input, &project.source, &project.source_options)?;
    let extras = project
        .extra_sources
        .iter()
        .map(|source| {
            Ok((
                source.name.clone(),
                loaded_source(root, base, &source.path, &source.schema, &source.options)?,
            ))
        })
        .collect::<TestResult<Vec<_>>>()?;
    let admitted = admission::admit(&project, &source, &extras, &mapping_path)?;
    let project_path = case.join("project.json");
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let mut input_paths = Vec::new();
    for (index, input) in admitted.inputs.iter().enumerate() {
        let path = case.join(format!("source-{index}.json"));
        std::fs::write(&path, &input.json)?;
        input_paths.push((input.name.clone(), path));
    }
    run_rust(
        &project_path,
        &input_paths,
        &admitted.outputs,
        case,
        rust_target,
    )?;
    run_csharp(&project_path, &input_paths, &admitted.outputs, case)?;
    Ok(())
}

fn command_success(output: &Output, stage: &str) -> TestResult<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{stage} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into())
    }
}

fn input_arguments(command: &mut Command, inputs: &[(String, PathBuf)]) {
    command.arg(&inputs[0].1);
    for (name, path) in &inputs[1..] {
        command.arg(name).arg(path);
    }
}

fn run_rust(
    project: &Path,
    inputs: &[(String, PathBuf)],
    expected: &[envelope::Document],
    case: &Path,
    target: &Path,
) -> TestResult<()> {
    let output = case.join("rust");
    generate_project(
        project,
        &output,
        GenerateTarget::Rust {
            runtime_path: Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"),
        },
    )?;
    std::fs::write(
        output.join("src/main.rs"),
        include_str!("fixtures/reference_corpus_generic_rust_harness.rs.txt"),
    )?;
    let build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&output)
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    command_success(&build, "generated Rust build")?;
    let mut command = Command::new("cargo");
    command
        .args(["run", "--quiet", "--"])
        .current_dir(&output)
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTFLAGS", "-D warnings");
    input_arguments(&mut command, inputs);
    let output = command.isolated_output()?;
    command_success(&output, "generated Rust execution")?;
    envelope::compare(&output.stdout, expected)?;
    Ok(())
}

fn run_csharp(
    project: &Path,
    inputs: &[(String, PathBuf)],
    expected: &[envelope::Document],
    case: &Path,
) -> TestResult<()> {
    let output = case.join("csharp");
    generate_project(project, &output, GenerateTarget::CSharp)?;
    let harness = output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("fixtures/reference_corpus_generic_csharp_harness.cs.txt"),
    )?;
    let build = dotnet_command(&output)
        .args([
            "build",
            "--configuration",
            "Release",
            "--nologo",
            "--verbosity",
            "quiet",
            "Harness/Harness.csproj",
        ])
        .current_dir(&output)
        .isolated_output()?;
    command_success(&build, "generated C# build")?;
    let mut command = dotnet_command(&output);
    command
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
        .current_dir(&output);
    input_arguments(&mut command, inputs);
    let output = command.isolated_output()?;
    command_success(&output, "generated C# execution")?;
    envelope::compare(&output.stdout, expected)?;
    Ok(())
}
