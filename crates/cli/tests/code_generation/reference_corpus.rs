//! Opt-in generated-backend execution against thirty-seven local, gitignored mappings.
//! Run with `cargo test -p cli --features codegen-tests --test code_generation
//! reference_corpus -- --ignored --nocapture` when the local sample corpus and
//! .NET 10 SDK are available. No sample contents are copied into this test.

use super::*;

#[derive(Clone, Copy)]
enum SourceKind {
    Json,
    Xml,
    XmlFileSet,
    Edifact,
    X12,
    Idoc,
    Xbrl,
    Sqlite,
    FlexText,
    Csv,
    Pdf,
    Protobuf,
    XlsxTransposed,
}

#[derive(Clone, Copy)]
enum TargetKind {
    Json,
    Xml,
    Csv,
    Protobuf,
    Xlsx,
    Xbrl,
}

struct CorpusCase {
    sample: &'static str,
    input: &'static str,
    source_kind: SourceKind,
    target_kind: TargetKind,
}

const CASES: [CorpusCase; 37] = [
    CorpusCase {
        sample: "EmployeesToJSONObject.mfd",
        input: "Altova_Hierarchical.json",
        source_kind: SourceKind::Json,
        target_kind: TargetKind::Json,
    },
    CorpusCase {
        sample: "Altova_Hierarchical_JSON.mfd",
        input: "Altova_Hierarchical.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Json,
    },
    CorpusCase {
        sample: "Altova_Hierarchical_FLF.mfd",
        input: "Altova_Hierarchical_FLF.txt",
        source_kind: SourceKind::FlexText,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "BuildHierarchyFromTextfile.mfd",
        input: "People.txt",
        source_kind: SourceKind::Csv,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "GroupTemperaturesByYear.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "SortByMultipleKeys.mfd",
        input: "OrgChart.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "FindHighestTemperatures.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "PositionInFilteredSequence.mfd",
        input: "BranchOffices.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "StringJoin.mfd",
        input: "BranchOffices.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "ClassifyTemperatures.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "TokenizeString2.mfd",
        input: "AltovaTools.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "Tutorial/JoinPeopleInfo.mfd",
        input: "Tutorial/People.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "Tutorial/ReadProtocolBuffers.mfd",
        input: "Tutorial/assets.bin",
        source_kind: SourceKind::Protobuf,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "ExcelColumnsToRecords.mfd",
        input: "ValuesByRegion.xlsx",
        source_kind: SourceKind::XlsxTransposed,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "FlattenHierarchy.mfd",
        input: "Directory.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "Tutorial/Expense-valmap.mfd",
        input: "Tutorial/ExpReport-item.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "KeyValueList.mfd",
        input: "KeyValueList.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "EmployeesToKeyValueList.mfd",
        input: "Employees.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "MultipleInputToMultipleOutputFiles.mfd",
        input: "Nanonull-*.xml",
        source_kind: SourceKind::XmlFileSet,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "MergeMultipleFiles.mfd",
        input: "Nanonull-*.xml",
        source_kind: SourceKind::XmlFileSet,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "Tutorial/Tut-ExpReport-multi.mfd",
        input: "Tutorial/mf-ExpReport.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "ArticlesInStock.mfd",
        input: "ClothingStockData2024.pdf",
        source_kind: SourceKind::Pdf,
        target_kind: TargetKind::Json,
    },
    CorpusCase {
        sample: "HandlingXsiNil.mfd",
        input: "BranchOffice2.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "Tutorial/ExtractCustomEDIFACT.mfd",
        input: "Tutorial/Orders-Custom.EDI",
        source_kind: SourceKind::Edifact,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "RecursiveDirectoryFilter.mfd",
        input: "Directory.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "MergeMultipleFiles_List.mfd",
        input: "NanonullFiles.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "ExpenseLimit.mfd",
        input: "ExpReport.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "ParseStringWithFlexText.mfd",
        input: "Names.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "InputIsSequence.mfd",
        input: "Temperatures.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "SelectPropertyFromJSON.mfd",
        input: "itemlist.json",
        source_kind: SourceKind::Json,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "JSON_To_Xml_PurchaseOrders.mfd",
        input: "ipos.json",
        source_kind: SourceKind::Json,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "PersonsToProtobuf.mfd",
        input: "Altova_Hierarchical.xml",
        source_kind: SourceKind::Xml,
        target_kind: TargetKind::Protobuf,
    },
    CorpusCase {
        sample: "IDoc_Order.mfd",
        input: "ORDERS.idoc",
        source_kind: SourceKind::Idoc,
        target_kind: TargetKind::Xml,
    },
    CorpusCase {
        sample: "Tutorial/ExtractCustomX12.mfd",
        input: "Tutorial/Orders-Custom.X12",
        source_kind: SourceKind::X12,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "XBRL_ReadOperatingExpensesFromTable.mfd",
        input: "nanonull.xbrl",
        source_kind: SourceKind::Xbrl,
        target_kind: TargetKind::Xlsx,
    },
    CorpusCase {
        sample: "DB_ApplicationList.mfd",
        input: "Accounts.sqlite",
        source_kind: SourceKind::Sqlite,
        target_kind: TargetKind::Csv,
    },
    CorpusCase {
        sample: "XBRL_WriteStatementsOfIncomeTable.mfd",
        input: "Nanonull.sqlite",
        source_kind: SourceKind::Sqlite,
        target_kind: TargetKind::Xbrl,
    },
];

#[test]
#[ignore = "requires the local ignored ReferenceSamples corpus and .NET 10 SDK"]
fn generated_rust_and_csharp_execute_local_samples_like_engine() -> TestResult<()> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let directory = TempDir::new("reference_corpus")?;
    let rust_target = directory.0.join("rust-target");
    let case_filter = std::env::var("FERRULE_REFERENCE_CORPUS_CASE").ok();
    let mut executed = 0;
    for (index, case) in CASES.iter().enumerate() {
        if case_filter
            .as_deref()
            .is_some_and(|filter| filter != case.sample)
        {
            continue;
        }
        let case_dir = directory.0.join(format!("case-{index}"));
        std::fs::create_dir(&case_dir)?;
        run_case(&samples, &case_dir, &rust_target, case)?;
        executed += 1;
    }
    assert!(executed > 0, "no local corpus case matched the filter");
    println!(
        "{} local mappings compiled and executed in generated Rust and C#",
        executed
    );
    Ok(())
}

fn run_case(
    samples: &Path,
    case_dir: &Path,
    rust_target: &Path,
    case: &CorpusCase,
) -> TestResult<()> {
    let sample = case.sample;
    let mapping_path = samples.join(sample);
    let input_path = samples.join(case.input);
    let imported = mfd::import_with_options(
        &mapping_path,
        &mfd::ImportOptions::default().with_package_root(samples),
    )?;
    assert!(
        imported.warnings.is_empty(),
        "{sample}: import warnings: {:?}",
        imported.warnings
    );
    let project = imported.project;
    assert!(
        project.runtime_dependencies().is_empty(),
        "{sample}: host dependencies prevent deterministic execution: {:?}",
        project.runtime_dependencies()
    );
    let named_input = (sample == "Tutorial/JoinPeopleInfo.mfd")
        .then_some(("Addresses", "Tutorial/Addresses.xml"));
    assert_eq!(
        project
            .extra_sources
            .iter()
            .filter(|source| source.dynamic_path.is_none())
            .count(),
        usize::from(named_input.is_some()),
        "{sample}: unexpected static named inputs"
    );
    assert!(
        match case.target_kind {
            TargetKind::Json => project.target_options.json_document,
            TargetKind::Xml => project.target_options.xml_document,
            TargetKind::Csv => {
                project.target_options.tabular_kind == Some(mapping::TabularBoundaryKind::Csv)
            }
            TargetKind::Protobuf => project.target_options.protobuf.is_some(),
            TargetKind::Xlsx => {
                project.target_options.tabular_kind == Some(mapping::TabularBoundaryKind::Xlsx)
                    && !project.target_options.xlsx_update_existing
            }
            TargetKind::Xbrl =>
                project.target_options.xbrl.as_ref().is_some_and(
                    |options| options.mode() == mapping::XbrlBoundaryMode::ExternalTarget
                ),
        },
        "{sample}: unexpected output format"
    );
    assert!(
        match case.source_kind {
            SourceKind::Json => project.source_options.json_document,
            SourceKind::Xml => project.source_options.xml_document,
            SourceKind::XmlFileSet => {
                project.source_options.xml_document && project.source_options.local_xml_file_set
            }
            SourceKind::Edifact => {
                project.source_options.edi_kind == Some(mapping::EdiBoundaryKind::Edifact)
            }
            SourceKind::X12 => {
                project.source_options.edi_kind == Some(mapping::EdiBoundaryKind::X12)
                    && project.source_options.x12_separators.is_some()
                    && !project.source_options.edi_implied_decimals.is_empty()
            }
            SourceKind::Idoc => {
                project.source_options.edi_kind == Some(mapping::EdiBoundaryKind::Idoc)
                    && project.source_options.idoc.is_some()
            }
            SourceKind::Xbrl => project.source_options.xbrl.is_some(),
            SourceKind::Sqlite => {
                project.source.name
                    == if sample == "XBRL_WriteStatementsOfIncomeTable.mfd" {
                        "Period"
                    } else {
                        "Users"
                    }
                    && project.source.repeating
            }
            SourceKind::FlexText => project.source_options.flextext.is_some(),
            SourceKind::Csv => {
                project.source_options.tabular_kind == Some(mapping::TabularBoundaryKind::Csv)
            }
            SourceKind::Pdf => project.source_options.pdf.is_some(),
            SourceKind::Protobuf => project.source_options.protobuf.is_some(),
            SourceKind::XlsxTransposed => {
                project.source_options.tabular_kind == Some(mapping::TabularBoundaryKind::Xlsx)
                    && !project.source_options.xlsx_rows.is_empty()
            }
        },
        "{sample}: unexpected input format"
    );
    assert_eq!(
        project
            .source_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(OsStr::to_str),
        Path::new(case.input).file_name().and_then(OsStr::to_str),
        "{sample}: unexpected source instance"
    );
    let validation = engine::validate(&project);
    assert!(validation.is_empty(), "{sample}: {validation:?}");

    let source = match case.source_kind {
        SourceKind::Json => format_json::read(&input_path, &project.source)?,
        SourceKind::Xml => format_xml::read(&input_path, &project.source)?,
        SourceKind::XmlFileSet => {
            format_xml::read_local_file_set(
                samples,
                Path::new(case.input),
                &project.source,
                format_xml::LocalFileSetLimits::default(),
            )?
            .instance
        }
        SourceKind::Edifact => {
            assert_eq!(
                project.source_options.edi_kind,
                Some(mapping::EdiBoundaryKind::Edifact),
                "{sample}: EDIFACT source boundary"
            );
            let mut instance = format_edi::edifact::read(
                &input_path,
                &project.source,
                project.source_options.lenient_segments,
            )?;
            format_edi::apply_implied_decimals(
                &mut instance,
                &project.source_options.edi_implied_decimals,
            )?;
            instance
        }
        SourceKind::X12 => {
            let separators = project
                .source_options
                .x12_separators
                .expect("retained X12 separators");
            assert_eq!(separators.element, '+');
            assert_eq!(separators.component, ':');
            assert_eq!(separators.segment, '\'');
            assert_eq!(separators.repetition, Some('!'));
            assert_eq!(separators.release, Some('?'));
            let mut instance = format_edi::x12::read_with_separators(
                &input_path,
                &project.source,
                project.source_options.lenient_segments,
                Some(format_edi::x12::Separators {
                    element: separators.element,
                    component: separators.component,
                    segment: separators.segment,
                    repetition: separators.repetition,
                    release: separators.release,
                }),
            )?;
            format_edi::apply_implied_decimals(
                &mut instance,
                &project.source_options.edi_implied_decimals,
            )?;
            instance
        }
        SourceKind::Idoc => format_edi::idoc::read(
            &input_path,
            &project.source,
            project
                .source_options
                .idoc
                .as_ref()
                .expect("embedded IDoc layout"),
            project.source_options.lenient_segments,
        )?,
        SourceKind::Xbrl => format_xbrl::read_with_options(
            &input_path,
            &project.source,
            project.source_options.xbrl.as_ref().expect("XBRL boundary"),
        )?,
        SourceKind::Sqlite => format_db::read_instance(&input_path, &project.source)?,
        SourceKind::FlexText => format_flextext::read(
            &input_path,
            &project.source,
            project.source_options.flextext.as_ref().unwrap(),
        )?,
        SourceKind::Csv => Instance::Repeated(format_csv::read(
            &input_path,
            &project.source,
            project.source_options.delimiter,
            project.source_options.has_header_row.unwrap_or(true),
        )?),
        SourceKind::Pdf => format_pdf::read(
            &input_path,
            project.source_options.pdf.as_ref().expect("PDF layout"),
        )?,
        SourceKind::Protobuf => {
            let options = project.source_options.protobuf.as_ref().unwrap();
            assert!(options.imports.is_empty(), "{sample}: protobuf imports");
            let layout = format_protobuf::Layout::parse(&options.schema)?;
            format_protobuf::read(&input_path, &layout, &options.root_message)?
        }
        SourceKind::XlsxTransposed => Instance::Repeated(format_xlsx::read_transposed(
            &input_path,
            &project.source,
            project.source_options.xlsx_sheet.as_deref(),
            &project.source_options.xlsx_rows,
        )?),
    };
    if matches!(case.source_kind, SourceKind::XmlFileSet) {
        return run_file_set_case(
            case_dir,
            rust_target,
            &mapping_path,
            &project,
            &source,
            sample,
        );
    }
    if sample == "MergeMultipleFiles_List.mfd" {
        return run_dynamic_source_case(
            case_dir,
            rust_target,
            &mapping_path,
            &project,
            &source,
            sample,
        );
    }
    if sample == "ExpenseLimit.mfd" {
        return run_failure_rule_case(case_dir, rust_target, &project, &source, sample);
    }
    if sample == "Tutorial/Tut-ExpReport-multi.mfd" {
        return run_multi_target_case(case_dir, rust_target, &project, &source, sample);
    }
    if sample == "HandlingXsiNil.mfd" {
        return run_xml_nil_case(case_dir, rust_target, &project, &source, sample);
    }
    // Generated hosts expose a schema-shaped JSON API. Preserve each native
    // reader's typed instance while crossing that host API.
    let source_json = match case.source_kind {
        SourceKind::Json => std::fs::read_to_string(&input_path)?,
        SourceKind::Xml
        | SourceKind::Edifact
        | SourceKind::X12
        | SourceKind::Idoc
        | SourceKind::Xbrl
        | SourceKind::Sqlite
        | SourceKind::FlexText
        | SourceKind::Csv
        | SourceKind::Pdf
        | SourceKind::Protobuf
        | SourceKind::XlsxTransposed => format_json::to_string(&project.source, &source)?,
        SourceKind::XmlFileSet => unreachable!("file sets use the typed generated host APIs"),
    };
    let mut named_sources = Vec::new();
    let mut named_input_paths = Vec::new();
    if let Some((name, input)) = named_input {
        let named_source = &project.extra_sources[0];
        assert_eq!(named_source.name, name, "{sample}: named input identity");
        assert_eq!(
            Path::new(&named_source.path)
                .file_name()
                .and_then(OsStr::to_str),
            Path::new(input).file_name().and_then(OsStr::to_str),
            "{sample}: named input instance"
        );
        assert!(
            named_source.options.xml_document,
            "{sample}: named input format"
        );
        assert!(
            named_source.dynamic_path.is_none(),
            "{sample}: static named input"
        );
        let instance = format_xml::read(&samples.join(input), &named_source.schema)?;
        let named_json = format_json::to_string(&named_source.schema, &instance)?;
        let named_path = case_dir.join("named-source.json");
        std::fs::write(&named_path, named_json)?;
        named_sources.push((name.to_owned(), instance));
        named_input_paths.push((name, named_path));
    }
    let mut expected = if named_sources.is_empty() {
        engine::run(&project, &source)?
    } else {
        engine::run_with_sources(&project, &source, named_sources)?
    };
    if matches!(
        sample,
        "FlattenHierarchy.mfd"
            | "EmployeesToKeyValueList.mfd"
            | "ArticlesInStock.mfd"
            | "ParseStringWithFlexText.mfd"
            | "InputIsSequence.mfd"
            | "SelectPropertyFromJSON.mfd"
            | "JSON_To_Xml_PurchaseOrders.mfd"
            | "PersonsToProtobuf.mfd"
            | "IDoc_Order.mfd"
            | "XBRL_ReadOperatingExpensesFromTable.mfd"
            | "DB_ApplicationList.mfd"
            | "XBRL_WriteStatementsOfIncomeTable.mfd"
    ) {
        let round_tripped = format_json::from_str(&source_json, &project.source)?;
        if matches!(
            sample,
            "InputIsSequence.mfd"
                | "SelectPropertyFromJSON.mfd"
                | "JSON_To_Xml_PurchaseOrders.mfd"
                | "IDoc_Order.mfd"
                | "XBRL_ReadOperatingExpensesFromTable.mfd"
                | "DB_ApplicationList.mfd"
                | "XBRL_WriteStatementsOfIncomeTable.mfd"
        ) {
            assert_eq!(
                round_tripped, source,
                "{sample}: source changes across schema-shaped JSON transport"
            );
        }
        assert_eq!(
            expected,
            engine::run(&project, &round_tripped)?,
            "{sample}: schema-shaped JSON boundary changed mapping output"
        );
    }
    if matches!(
        sample,
        "Tutorial/ExtractCustomEDIFACT.mfd" | "Tutorial/ExtractCustomX12.mfd"
    ) {
        let round_tripped = format_json::from_str(&source_json, &project.source)?;
        assert_eq!(
            round_tripped, source,
            "{sample}: schema-shaped JSON changes the native EDI instance"
        );
        assert_eq!(
            engine::run(&project, &round_tripped)?,
            expected,
            "{sample}: schema-shaped JSON changes the mapped customer row"
        );
    }
    if sample == "RecursiveDirectoryFilter.mfd" {
        let round_tripped = format_json::from_str(&source_json, &project.source)?;
        // Schema-shaped JSON cannot carry XML's private choice-order stream.
        // Compare generated hosts with the interpreter under this same input
        // boundary, while checking native-source XML output separately.
        assert_eq!(
            format_json::to_string(&project.source, &round_tripped)?,
            source_json,
            "{sample}: schema-shaped source JSON roundtrip"
        );
        let transported_output = engine::run(&project, &round_tripped)?;
        assert_eq!(
            format_json::to_string(&project.target, &transported_output)?,
            format_json::to_string(&project.target, &expected)?,
            "{sample}: source transport changes visible recursive filtering"
        );
        let native_xml = format_xml::to_string_with_options(
            &project.target,
            &expected,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                default_namespace: None,
            },
        )?;
        let native_xml_roundtrip = format_xml::from_str(&native_xml, &project.target)?;
        assert_eq!(
            format_json::to_string(&project.target, &native_xml_roundtrip)?,
            format_json::to_string(&project.target, &expected)?,
            "{sample}: native XML output must not resurrect dropped files"
        );
        expected = transported_output;
    }
    // A mapped XML sequence can contain multiple occurrences under a nominally
    // nonrepeating XSD field. Keep the JSON boundary strict and compare that
    // case through the generated Instance API and XML serializers instead.
    let mapped_xml_output = sample == "Tutorial/Expense-valmap.mfd";
    let recursive_xml_output = sample == "RecursiveDirectoryFilter.mfd";
    let purchase_orders_xml_output = sample == "JSON_To_Xml_PurchaseOrders.mfd";
    let protobuf_output = sample == "PersonsToProtobuf.mfd";
    let idoc_xml_output = sample == "IDoc_Order.mfd";
    let xlsx_output = sample == "XBRL_ReadOperatingExpensesFromTable.mfd";
    let sqlite_csv_output = sample == "DB_ApplicationList.mfd";
    let xbrl_output = sample == "XBRL_WriteStatementsOfIncomeTable.mfd";
    let typed_xml_output = recursive_xml_output
        || sample == "InputIsSequence.mfd"
        || purchase_orders_xml_output
        || idoc_xml_output;
    let expected_xml = if mapped_xml_output || typed_xml_output {
        Some(format_xml::to_string_with_options(
            &project.target,
            &expected,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                default_namespace: None,
            },
        )?)
    } else {
        None
    };
    let expected_json: serde_json::Value = if mapped_xml_output {
        serde_json::Value::Null
    } else if idoc_xml_output {
        // The IDoc fields remain lexical strings in the mapped Instance. Both
        // XML and JSON output boundaries convert exact decimal Amount values.
        let direct = format_json::to_string(&project.target, &expected)?;
        let normalized = format_xml::from_str(
            expected_xml.as_ref().expect("IDoc XML target"),
            &project.target,
        )?;
        let xml_json = format_json::to_string(&project.target, &normalized)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&direct)?,
            serde_json::from_str::<serde_json::Value>(&xml_json)?,
            "{sample}: native JSON and XML target boundaries disagree"
        );
        serde_json::from_str(&direct)?
    } else {
        serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?
    };
    if sample == "IDoc_Order.mfd" {
        assert!(
            project.source_options.idoc.is_some(),
            "{sample}: the local parser layout must be embedded in the imported project"
        );
        assert!(project.extra_targets.is_empty(), "{sample}: one XML target");
        assert_eq!(expected_json["Header"]["Number"], "4500000327");
        assert_eq!(
            expected_json["Header"]["Received"], "1999-06-21T09:30:00",
            "{sample}: the two-argument IDoc date function"
        );
        let customer = expected_json["Customer"]
            .as_object()
            .expect("mapped customer group");
        assert!(
            customer
                .get("Address")
                .and_then(serde_json::Value::as_object)
                .is_some_and(serde_json::Map::is_empty)
                && !customer.contains_key("Number")
                && !customer.contains_key("ContactName")
                && !customer.contains_key("CompanyName"),
            "{sample}: the first sparse partner segment is selected"
        );
        let items = expected_json["LineItems"]["LineItem"]
            .as_array()
            .expect("mapped order items");
        assert_eq!(items.len(), 2, "{sample}: both IDoc item records");
        let raw_amounts = expected
            .field("LineItems")
            .and_then(|items| items.field("LineItem"))
            .and_then(Instance::as_repeated)
            .expect("raw IDoc order items")
            .iter()
            .map(|item| {
                item.field("Article")
                    .and_then(|article| article.field("Amount"))
                    .and_then(Instance::as_scalar)
                    .cloned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            raw_amounts,
            vec![
                Some(Value::String("1.000".into())),
                Some(Value::String("2.000".into())),
            ],
            "{sample}: IDoc lexical decimal strings reach integer XML fields"
        );
        for (item, (number, amount, price, tax)) in
            items.iter().zip([(10, 1, 7.2, 1.44), (20, 2, 13.2, 2.64)])
        {
            let article = &item["Article"];
            assert_eq!(article["Number"], number);
            assert_eq!(article["Amount"], amount);
            assert_eq!(article["Price"], price);
            assert_eq!(article["Tax"], tax);
            assert!(
                article.get("Name").is_none(),
                "{sample}: the first sparse text segment is selected"
            );
        }
    }
    if purchase_orders_xml_output {
        assert!(project.extra_targets.is_empty(), "{sample}: one XML target");
        assert_eq!(
            project
                .target
                .xml_namespace
                .as_ref()
                .and_then(ir::XmlNamespace::uri),
            Some("http://www.altova.com/IPO"),
            "{sample}: qualified purchase-order root"
        );
        assert_eq!(expected_json["customer"], "MFGB");
        let orders = expected_json["purchaseOrder"]
            .as_array()
            .expect("mapped purchase orders");
        assert_eq!(orders.len(), 3, "{sample}: all orders");
        assert_eq!(
            orders
                .iter()
                .map(|order| order["Items"]["item"].as_array().expect("line items").len())
                .collect::<Vec<_>>(),
            vec![6, 3, 3],
            "{sample}: all twelve line items retain their order"
        );
        assert_eq!(orders[0]["Items"]["item"][0]["partNum"], "833-AA");
        assert_eq!(orders[1]["Items"]["item"][0]["partNum"], "150-RS");
        assert_eq!(orders[2]["Items"]["item"][0]["partNum"], "940-SR");
        assert!(
            orders[0]["shipTo"].get("postcode").is_some()
                && orders[0]["shipTo"].get("state").is_none(),
            "{sample}: first shipping address selects the EU alternative"
        );
        assert!(
            orders[1..].iter().all(|order| {
                order["shipTo"].get("postcode").is_none()
                    && order["shipTo"].get("state").is_some()
                    && order["shipTo"].get("zip").is_some()
            }) && orders.iter().all(|order| {
                order["billTo"].get("state").is_some() && order["billTo"].get("zip").is_some()
            }),
            "{sample}: remaining shipping and all billing addresses select the US alternative"
        );
        let xml = expected_xml.as_ref().expect("typed XML output");
        assert_eq!(xml.matches("xsi:type=\"ft:EU-Address\"").count(), 1);
        assert_eq!(xml.matches("xsi:type=\"ft:US-Address\"").count(), 5);
    }
    if protobuf_output {
        assert!(
            project.extra_targets.is_empty(),
            "{sample}: one binary target"
        );
        let people = expected_json["person"]
            .as_array()
            .expect("mapped Protobuf people");
        assert_eq!(people.len(), 21, "{sample}: every XML person is mapped");
        for (index, person) in people.iter().enumerate() {
            assert_eq!(
                person["id"],
                (index + 1) as i64,
                "{sample}: source ID order"
            );
            let phone = person["phone"].as_array().expect("one phone group");
            assert_eq!(phone.len(), 1, "{sample}: one phone per person");
            assert_eq!(phone[0]["type"], 2, "{sample}: WORK enum value");
        }
        assert_eq!(people[0]["name"], "Vernon Callaby");
        assert_eq!(people[0]["email"], "v.callaby@nanonull.com");
        assert_eq!(people[0]["phone"][0]["number"], "582");
        assert_eq!(people[20]["name"], "Mark Redgreen");
        assert_eq!(people[20]["email"], "m.redgreen@nanonull.com");
        assert_eq!(people[20]["phone"][0]["number"], "152");
    }
    if sample == "BuildHierarchyFromTextfile.mfd" {
        let Instance::Repeated(rows) = &source else {
            panic!("{sample}: CSV source should contain repeated rows");
        };
        assert_eq!(rows.len(), 30, "{sample}: source row count");
        assert_eq!(expected_json["Name"], "Organization Chart");
        let offices = expected_json["Office"].as_array().expect("office array");
        assert_eq!(offices.len(), 2, "{sample}: company groups");
        let departments = offices
            .iter()
            .flat_map(|office| office["Department"].as_array().expect("department array"))
            .collect::<Vec<_>>();
        assert_eq!(departments.len(), 7, "{sample}: department groups");
        assert_eq!(
            departments
                .iter()
                .map(|department| department["Person"].as_array().expect("person array").len())
                .sum::<usize>(),
            30,
            "{sample}: mapped people"
        );
    }
    if sample == "GroupTemperaturesByYear.mfd" {
        let years = expected_json["YearlyStats"]
            .as_array()
            .expect("yearly temperature groups");
        assert_eq!(years.len(), 5, "{sample}: one group per year");
        assert_eq!(
            years
                .iter()
                .map(|year| year["Year"].as_i64())
                .collect::<Vec<_>>(),
            vec![Some(2006), Some(2007), Some(2008), Some(2009), Some(2010)],
            "{sample}: groups retain first-seen year order"
        );
        assert_eq!(years[0]["MinimumTemp"], -3.6);
        assert_eq!(years[0]["MaximumTemp"], 23.2);
        assert_eq!(years[0]["AverageTemp"], 11.375);
    }
    if sample == "SortByMultipleKeys.mfd" {
        assert_eq!(expected_json["Name"], "Share Ranking");
        let offices = expected_json["Office"].as_array().expect("office array");
        assert_eq!(offices.len(), 1, "{sample}: source has one office");
        let people = offices[0]["Person"].as_array().expect("person array");
        assert_eq!(people.len(), 16, "{sample}: all department members");
        assert_eq!(people[0]["Shares"], 2000);
        assert_eq!(people[0]["Last"], "Landis");
        assert_eq!(people[1]["Shares"], 2000);
        assert_eq!(people[1]["Last"], "Martin");
        assert_eq!(people[2]["Last"], "Martin");
        assert_eq!(people[3]["Last"], "Martin");
        assert_eq!(people[1]["First"]["#text"], "Alex");
        assert_eq!(people[2]["First"]["#text"], "Joe");
        assert_eq!(people[3]["First"]["#text"], "Susan");
        assert_eq!(people[4]["Shares"], 1500);
        assert_eq!(people[4]["Last"], "Butler");
        assert_eq!(people[5]["Shares"], 1500);
        assert_eq!(people[5]["Last"], "Callaby");
    }
    if sample == "FindHighestTemperatures.mfd" {
        let data = expected_json["data"]
            .as_array()
            .expect("selected temperatures");
        assert_eq!(data.len(), 10, "{sample}: top ten temperatures");
        assert_eq!(
            data.iter()
                .map(|item| item["temp"].as_f64().expect("numeric temperature"))
                .collect::<Vec<_>>(),
            vec![24.0, 23.8, 23.2, 22.7, 22.3, 22.3, 21.5, 21.4, 21.1, 20.7],
            "{sample}: selected temperatures retain descending order"
        );
        assert_eq!(data[0]["month"], "2008-07");
        assert_eq!(data[9]["month"], "2007-08");
    }
    if sample == "PositionInFilteredSequence.mfd" {
        let contacts = expected_json["Contact"]
            .as_array()
            .expect("filtered contacts");
        assert_eq!(contacts.len(), 8, "{sample}: last names after M");
        assert_eq!(
            contacts
                .iter()
                .map(|contact| contact["ID"].as_str().expect("position ID").to_owned())
                .collect::<Vec<_>>(),
            (1..=8).map(|id| id.to_string()).collect::<Vec<_>>(),
            "{sample}: filtered positions are compact and one-based"
        );
        assert_eq!(contacts[0]["First"], "Loby");
        assert_eq!(contacts[0]["Last"], "Matise");
        assert_eq!(contacts[7]["First"], "Mark");
        assert_eq!(contacts[7]["Last"], "Redgreen");
    }
    if sample == "StringJoin.mfd" {
        let info = expected_json["Info"].as_array().expect("one joined notice");
        assert_eq!(info.len(), 1, "{sample}: one notice");
        let title = info[0]["Title"].as_str().expect("joined title");
        assert!(
            title.starts_with("Dear Vernon, Frank, Loby, "),
            "{sample}: {title}"
        );
        assert!(
            title.ends_with(", Valentin, Carl, Mark"),
            "{sample}: {title}"
        );
        assert_eq!(title.split(", ").count(), 21, "{sample}: joined contacts");
        assert_eq!(info[0]["Description"]["#text"], "You are all promoted.");
    }
    if sample == "ClassifyTemperatures.mfd" {
        let data = expected_json["data"]
            .as_array()
            .expect("classified readings");
        assert_eq!(data.len(), 60, "{sample}: all readings retain source order");
        assert_eq!(data[0]["month"], "2006-01");
        assert_eq!(data[0]["desc"], "low");
        assert_eq!(data[30]["month"], "2008-07");
        assert_eq!(data[30]["desc"], "high");
        assert_eq!(data[32]["month"], "2008-09");
        assert_eq!(data[32]["temp"], 20.0);
        assert!(
            data[32].get("desc").is_none(),
            "{sample}: threshold 20 has no class"
        );
        assert_eq!(data[59]["month"], "2010-12");
        assert_eq!(data[59]["desc"], "low");
        assert_eq!(
            data.iter()
                .filter(|reading| reading["desc"] == "low")
                .count(),
            16,
            "{sample}: low classifications"
        );
        assert_eq!(
            data.iter()
                .filter(|reading| reading["desc"] == "high")
                .count(),
            11,
            "{sample}: high classifications"
        );
        assert_eq!(
            data.iter()
                .filter(|reading| reading.get("desc").is_none())
                .count(),
            33,
            "{sample}: unclassified middle readings"
        );
    }
    if sample == "TokenizeString2.mfd" {
        let rows = expected_json.as_array().expect("concatenated CSV rows");
        assert_eq!(rows.len(), 10, "{sample}: heading and nine tools");
        assert_eq!(
            rows.iter()
                .enumerate()
                .filter_map(|(index, row)| (index != 2).then_some(row["Tool"].as_str()))
                .collect::<Vec<_>>(),
            vec![
                Some("Tool"),
                Some("XMLSpy"),
                Some("StyleVision"),
                Some("UModel"),
                Some("DatabaseSpy"),
                Some("DiffDog"),
                Some("SchemaAgent"),
                Some("SemanticWorks"),
                Some("Authentic"),
            ],
            "{sample}: ordered source tools"
        );
        assert!(
            rows[2]["Tool"]
                .as_str()
                .is_some_and(|tool| !tool.is_empty())
        );
        assert_eq!(
            rows.iter()
                .map(|row| row["ExistsInMissionKit"].as_str())
                .collect::<Vec<_>>(),
            vec![
                Some("MissionKit for Enterprise XML Developers"),
                Some("Y"),
                Some("Y"),
                Some("Y"),
                Some("N"),
                Some("N"),
                Some("Y"),
                Some("Y"),
                Some("Y"),
                Some("N"),
            ],
            "{sample}: lookup-fed token existence"
        );
    }
    if sample == "Tutorial/JoinPeopleInfo.mfd" {
        let rows = expected_json["Row"].as_array().expect("joined people");
        assert_eq!(rows.len(), 3, "{sample}: only matched people remain");
        let mut keys = std::collections::BTreeSet::new();
        for row in rows {
            let first = row["FirstName"].as_str().expect("joined first name");
            let last = row["LastName"].as_str().expect("joined last name");
            assert!(!first.is_empty() && !last.is_empty());
            assert!(
                keys.insert((first, last)),
                "{sample}: joined names are distinct"
            );
            assert!(row["City"].as_str().is_some_and(|city| !city.is_empty()));
            assert!(
                row["Street"]
                    .as_str()
                    .is_some_and(|street| !street.is_empty())
            );
            assert!(row["Email"].as_str().is_some_and(|email| !email.is_empty()));
        }
    }
    if sample == "Tutorial/ReadProtocolBuffers.mfd" {
        let rows = expected_json.as_array().expect("CSV painting rows");
        assert_eq!(rows.len(), 7, "{sample}: one row per protobuf painting");
        assert!(rows.iter().all(|row| {
            ["Name", "Period", "Dimensions", "Format", "Location"]
                .into_iter()
                .all(|field| row[field].as_str().is_some_and(|value| !value.is_empty()))
        }));
        assert_eq!(rows[0]["Dimensions"], "61.4 in x 67.8 in");
        assert_eq!(rows[6]["Dimensions"], "11 in x 51.2 in");
        assert_eq!(
            rows.iter()
                .filter(|row| row["Location"] == "Museum")
                .count(),
            4,
            "{sample}: first value-map branch"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row["Location"] == "Temple")
                .count(),
            1,
            "{sample}: second value-map branch"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row["Location"] == "Private collection")
                .count(),
            2,
            "{sample}: third value-map branch"
        );
    }
    if sample == "ExcelColumnsToRecords.mfd" {
        let rows = expected_json.as_array().expect("transposed XLSX rows");
        assert_eq!(rows.len(), 4, "{sample}: four non-header region columns");
        assert_eq!(
            rows.iter()
                .map(|row| row["Region"].as_str())
                .collect::<Vec<_>>(),
            vec![Some("US"), Some("EU"), Some("JP"), Some("AU")],
            "{sample}: source column order"
        );
        for row in rows {
            let component = |field: &str| row[field].as_f64().expect("numeric revenue field");
            assert_eq!(
                component("Passenger tickets")
                    + component("Onboard and other")
                    + component("Tour and other"),
                component("Revenues"),
                "{sample}: item-at fields align within a region column"
            );
        }
        assert_eq!(rows[0]["Revenues"], 2_406_000_000.0);
        assert_eq!(rows[3]["Revenues"], 4_954_000_000.0);
    }
    if sample == "FlattenHierarchy.mfd" {
        let paths = expected_json["File"]
            .as_array()
            .expect("recursively collected file paths");
        assert_eq!(paths.len(), 90, "{sample}: all nested files are collected");
        let depths = paths
            .iter()
            .map(|path| {
                let path = path.as_str().expect("scalar file path");
                assert!(path.starts_with('\\'), "{sample}: root prefix");
                assert!(!path.contains("\\\\"), "{sample}: no empty path segment");
                path.matches('\\').count()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            depths.iter().filter(|depth| **depth == 3).count(),
            61,
            "{sample}: immediate child files"
        );
        assert_eq!(
            depths.iter().filter(|depth| **depth == 4).count(),
            24,
            "{sample}: nested child files"
        );
        assert_eq!(
            depths.iter().filter(|depth| **depth == 5).count(),
            5,
            "{sample}: deepest child files"
        );
        assert_eq!(
            paths
                .iter()
                .map(|path| path.as_str().expect("scalar file path"))
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            paths.len(),
            "{sample}: each collected path is distinct"
        );
    }

    if sample == "Tutorial/Expense-valmap.mfd" {
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(node, Node::ValueMap { .. }))
                .count(),
            2,
            "{sample}: integer and boolean value maps are present"
        );
        let Instance::Group(fields) = &expected else {
            panic!("{sample}: mapped root group");
        };
        let (_, Instance::MappedSequence(items)) = fields
            .iter()
            .find(|(name, _)| name == "expense-item")
            .expect("mapped expense field")
        else {
            panic!("{sample}: mapped expense sequence");
        };
        assert_eq!(items.len(), 4, "{sample}: four mapped expense items");
        let weekdays = items
            .iter()
            .map(|item| corpus_string_field(item, "Weekday"))
            .collect::<Vec<_>>();
        assert!(weekdays.iter().all(|value| !value.is_empty()));
        assert_eq!(
            weekdays
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3,
            "{sample}: integer value map selects distinct weekday branches"
        );
        let notes = items
            .iter()
            .map(|item| corpus_string_field(item, "Notes"))
            .collect::<Vec<_>>();
        assert!(notes.iter().all(|value| !value.is_empty()));
        assert_eq!(
            notes
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2,
            "{sample}: boolean value map selects mapped and default branches"
        );
        assert_eq!(
            notes.iter().filter(|note| **note == notes[0]).count(),
            1,
            "{sample}: one expense selects the non-default note"
        );
    }
    if sample == "KeyValueList.mfd" {
        let expression = project
            .graph
            .nodes
            .values()
            .find_map(|node| match node {
                Node::Aggregate {
                    function: mapping::AggregateOp::Join,
                    collection,
                    expression: Some(expression),
                    ..
                } if collection.len() == 1 && collection[0] == "Item" => Some(expression),
                _ => None,
            })
            .expect("per-item join expression");
        let Some(Node::Call { args, .. }) = project.graph.nodes.get(expression) else {
            panic!("{sample}: join expression should combine lookup results");
        };
        assert_eq!(
            args.iter()
                .filter(|node| matches!(project.graph.nodes.get(node), Some(Node::Lookup { .. })))
                .count(),
            2,
            "{sample}: two lookups execute for each aggregate item"
        );
        let info = expected_json["Info"]
            .as_array()
            .expect("one summarized info item");
        assert_eq!(info.len(), 1, "{sample}: one summary item");
        assert!(
            info[0]["Title"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
        assert!(
            info[0]["Description"]["#text"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
    }
    if sample == "EmployeesToKeyValueList.mfd" {
        let source_fields = project
            .graph
            .nodes
            .values()
            .filter_map(|node| match node {
                Node::SourceField {
                    path,
                    frame: Some(frame),
                } if frame.len() == 3
                    && frame[0] == "Employees"
                    && frame[1] == "element()"
                    && frame[2] == "element()" =>
                {
                    Some(path)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(source_fields.len(), 2, "{sample}: generic XML frame fields");
        for name in ["LocalName", "#text"] {
            assert!(
                source_fields
                    .iter()
                    .any(|path| path.len() == 1 && path[0] == name),
                "{sample}: runtime element name and text stay pinned"
            );
        }
        let items = expected_json["Item"].as_array().expect("mapped items");
        assert_eq!(items.len(), 4, "{sample}: four source elements");
        let properties = items
            .iter()
            .flat_map(|item| {
                let properties = item["Property"].as_array().expect("item properties");
                assert_eq!(properties.len(), 4, "{sample}: four fields per item");
                properties.iter()
            })
            .collect::<Vec<_>>();
        assert_eq!(properties.len(), 16, "{sample}: all generic fields map");
        let keys = properties
            .iter()
            .map(|property| property["Key"].as_str().expect("property key"))
            .collect::<Vec<_>>();
        assert!(keys.iter().all(|key| !key.is_empty()));
        assert!(properties.iter().all(|property| {
            property["#text"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        }));
        assert_eq!(
            keys.iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4,
            "{sample}: four distinct runtime field names"
        );
    }
    if sample == "ArticlesInStock.mfd" {
        let articles = expected_json.as_array().expect("PDF stock articles");
        assert_eq!(
            articles.len(),
            11,
            "{sample}: all PDF articles are extracted"
        );
        assert_eq!(articles[0]["Number"], 123456.0);
        assert_eq!(articles[0]["Name"], "Flowing Silk Maxi Dress");
        let numbers = articles
            .iter()
            .map(|article| {
                article["Number"]
                    .as_f64()
                    .expect("article number")
                    .to_bits()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(numbers.len(), articles.len(), "{sample}: distinct articles");
        for article in articles {
            let stores = article["StoreDetails"].as_array().expect("article stores");
            assert_eq!(stores.len(), 2, "{sample}: both stores are retained");
            assert!(stores.iter().all(|store| {
                store["Store"].as_str().is_some_and(|name| !name.is_empty())
                    && store["Available"]
                        .as_object()
                        .is_some_and(|sizes| !sizes.is_empty())
            }));
        }
        assert_eq!(articles[0]["StoreDetails"][0]["Available"]["XS"], 1.0);
        assert_eq!(articles[0]["StoreDetails"][1]["Available"]["XL"], 6.0);
    }
    if recursive_xml_output {
        assert_eq!(
            expected_json["name"], "Examples",
            "{sample}: root directory"
        );
        let mut names = Vec::new();
        let directories = collect_recursive_directory_files(&expected_json, &mut names);
        assert_eq!(directories, 16, "{sample}: recursive directory shape");
        assert_eq!(names.len(), 33, "{sample}: retained XML files");
        assert!(
            names.iter().all(|name| name.contains(".xml")),
            "{sample}: nonmatching files must be filtered at every depth"
        );
        for name in ["blocks.xml", "Newsml-example.xml", "datatypes.xml"] {
            assert!(names.contains(&name), "{sample}: missing nested {name}");
        }
        for name in ["blocks.sps", "altova.mdb"] {
            assert!(!names.contains(&name), "{sample}: retained {name}");
        }
    }
    if sample == "InputIsSequence.mfd" {
        let [year_scope] = project.root.children.as_slice() else {
            panic!("{sample}: one generated YearlyStats scope");
        };
        assert!(
            matches!(
                &year_scope.iteration,
                ScopeIteration::Sequence(mapping::SequenceExpr::Generate { .. })
            ),
            "{sample}: generated singleton sequence drives the target"
        );
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(
                    node,
                    Node::Aggregate {
                        expression: Some(_),
                        ..
                    }
                ))
                .count(),
            3,
            "{sample}: min/max/avg use per-source-item expressions"
        );
        let years = expected_json["YearlyStats"]
            .as_array()
            .expect("generated yearly statistics");
        assert_eq!(years.len(), 1, "{sample}: one generated item");
        assert_eq!(years[0]["Year"], 2008);
        assert_eq!(years[0]["MinimumTemp"], -0.5);
        assert_eq!(years[0]["MaximumTemp"], 24.0);
        assert_eq!(years[0]["AverageTemp"], 11.6);
    }
    let expected_csv = if sample == "Tutorial/ExtractCustomEDIFACT.mfd" {
        assert_eq!(
            project.target_options.delimiter,
            Some(','),
            "{sample}: comma-separated target"
        );
        assert_eq!(
            project.target_options.has_header_row,
            Some(false),
            "{sample}: headerless target"
        );
        let rows = expected_json.as_array().expect("EDIFACT buyer CSV rows");
        assert_eq!(rows.len(), 1, "{sample}: one buyer row");
        assert_eq!(rows[0]["Name"], "Michelle Butler");
        assert_eq!(rows[0]["Salutation"], "Mrs");
        assert!(
            rows[0]["Date"]
                .as_str()
                .is_some_and(|date| date.starts_with("2020-04-30T17:42:00")),
            "{sample}: EDIFACT 2379 date-time conversion"
        );
        Some(corpus_csv_bytes(&project, &expected)?)
    } else if sample == "Tutorial/ExtractCustomX12.mfd" {
        assert_eq!(project.target_options.delimiter, Some(','));
        assert_eq!(project.target_options.has_header_row, Some(false));
        let rows = expected_json.as_array().expect("X12 customer CSV rows");
        assert_eq!(rows.len(), 1, "{sample}: one customer row");
        assert_eq!(rows[0]["Name"], "Michelle Butler");
        assert_eq!(rows[0]["Salutation"], "Mrs");
        assert_eq!(rows[0]["Date"], "20200430");
        let bytes = corpus_csv_bytes(&project, &expected)?;
        assert_eq!(bytes, b"Michelle Butler,Mrs,20200430\n");
        Some(bytes)
    } else if sqlite_csv_output {
        assert_eq!(project.target_options.delimiter, Some(','));
        assert_eq!(project.target_options.has_header_row, Some(false));
        assert_eq!(source.as_repeated().expect("SQLite user rows").len(), 4);
        let rows = expected_json
            .as_array()
            .expect("SQLite application CSV rows");
        assert_eq!(rows.len(), 4, "{sample}: one row per user");
        for (row, user) in rows.iter().zip([
            "Vernon Callaby",
            "Frank Further",
            "Loby Matise",
            "Susi Sanna",
        ]) {
            assert_eq!(row["User"], user);
        }
        assert_eq!(rows[0]["Application"], rows[1]["Application"]);
        assert_eq!(rows[1]["Application"], rows[2]["Application"]);
        assert_eq!(rows[0]["Category"], "IDE");
        assert_eq!(rows[1]["Category"], "IDE");
        assert_eq!(rows[2]["Category"], "IDE");
        assert_eq!(rows[3]["Application"], "Notepad");
        assert_eq!(rows[3]["Category"], "Misc.");
        assert_eq!(rows[3]["Description"], "No Description");
        let bytes = corpus_csv_bytes(&project, &expected)?;
        assert_eq!(bytes.len(), 234, "{sample}: exact four-row CSV length");
        assert!(bytes.starts_with(b"Vernon Callaby,"));
        assert!(bytes.ends_with(b"Susi Sanna,Notepad,Misc.,No Description\n"));
        assert_eq!(bytes.iter().filter(|&&byte| byte == b'\n').count(), 4);
        Some(bytes)
    } else if sample == "ParseStringWithFlexText.mfd" {
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(node, Node::Call { function, .. } if function == "flextext_parse_field"))
                .count(),
            4,
            "{sample}: all four embedded FlexText field projections"
        );
        assert_eq!(project.target_options.delimiter, Some(','));
        assert_eq!(project.target_options.has_header_row, Some(false));
        let rows = expected_json.as_array().expect("parsed name rows");
        let names = [
            ("Ted", "McAllister", "Agathe", "Steve"),
            ("Susan", "Edwards", "Sue", "Max"),
            ("Fred", "Landis", "Ann", "Martin"),
            ("George", "Hammer", "Jessica", "Robert"),
        ];
        assert_eq!(rows.len(), names.len(), "{sample}: all source names");
        for (row, (first, last, mother, father)) in rows.iter().zip(names) {
            assert_eq!(row["First"], first);
            assert_eq!(row["Last"], last);
            assert_eq!(row["Mother's Name"], mother);
            assert_eq!(row["Father's Name"], father);
        }
        Some(corpus_csv_bytes(&project, &expected)?)
    } else if sample == "SelectPropertyFromJSON.mfd" {
        assert_eq!(
            project
                .graph
                .nodes
                .values()
                .filter(|node| matches!(node, Node::DynamicSourceField { .. }))
                .count(),
            1,
            "{sample}: one computed JSON source property"
        );
        assert_eq!(
            project.root.sort_filter_order,
            mapping::SortFilterOrder::FilterThenSort,
            "{sample}: filter runs before the part-number sort"
        );
        assert_eq!(project.target_options.delimiter, Some(','));
        assert_eq!(project.target_options.has_header_row, Some(true));
        let inputs = source.as_repeated().expect("JSON root rows");
        assert_eq!(inputs.len(), 12);
        assert_eq!(
            inputs
                .iter()
                .filter(|row| {
                    row.field("out-of-stock")
                        .and_then(Instance::as_scalar)
                        .is_none_or(|value| matches!(value, Value::Null))
                })
                .count(),
            5,
            "{sample}: absent properties remain distinct from false"
        );
        let rows = expected_json.as_array().expect("out-of-stock CSV rows");
        let expected_rows = [
            ("148-ON", "Coral necklace"),
            ("229-OB", "Pearl necklace"),
            ("238-KK", "Amber ring"),
            ("745-JW", "White pearl jade necklace"),
            ("748-OT", "Diamond heart"),
        ];
        assert_eq!(rows.len(), expected_rows.len());
        for (row, (part, name)) in rows.iter().zip(expected_rows) {
            assert_eq!(row["Part Number (Out of Stock)"], part);
            assert_eq!(row["Product Name"], name);
        }
        Some(corpus_csv_bytes(&project, &expected)?)
    } else {
        None
    };
    let expected_protobuf = if protobuf_output {
        let options = project
            .target_options
            .protobuf
            .as_ref()
            .expect("Protobuf target");
        assert!(options.imports.is_empty(), "{sample}: local proto2 schema");
        assert_eq!(options.root_message, "Persons");
        let layout = format_protobuf::Layout::parse(&options.schema)?;
        let bytes = format_protobuf::to_vec(&layout, &options.root_message, &expected)?;
        let decoded = format_protobuf::from_slice(&layout, &options.root_message, &bytes)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&format_json::to_string(
                &project.target,
                &decoded,
            )?)?,
            expected_json,
            "{sample}: native Protobuf encoding changes mapped values"
        );
        Some(bytes)
    } else {
        None
    };
    let expected_xlsx_cells = if xlsx_output {
        assert_eq!(
            project.target_options.xlsx_sheet.as_deref(),
            Some("Operating Expenses")
        );
        assert_eq!(project.target_options.xlsx_start_row, Some(1));
        assert_eq!(
            project.target_options.xlsx_columns,
            (1..=10).collect::<Vec<_>>()
        );
        assert_eq!(project.target_options.has_header_row, Some(true));
        assert!(project.target_options.xlsx_headers.is_empty());
        let rows = expected_json
            .as_array()
            .expect("XBRL operating expense rows");
        assert_eq!(rows.len(), 4, "{sample}: four statement periods");
        let periods = [
            ("2009-12-01", "2010-08-30", 3_454_000_000.0),
            ("2008-12-01", "2009-08-30", 2_823_000_000.0),
            ("2009-06-01", "2009-08-30", 1_096_000_000.0),
            ("2010-06-01", "2010-08-30", 1_319_000_000.0),
        ];
        for (row, (start, end, total)) in rows.iter().zip(periods) {
            assert_eq!(row["Start Date"], start);
            assert_eq!(row["End Date"], end);
            assert_eq!(row["Total"].as_f64(), Some(total));
        }
        assert_eq!(
            rows[0]["Commissions, transportation and other"].as_f64(),
            Some(872_000_000.0)
        );
        let cells = corpus_xlsx_cells(&project, &expected)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&format_json::to_string(
                &project.target,
                &Instance::Repeated(cells.clone()),
            )?)?,
            expected_json,
            "{sample}: workbook cells differ from the mapped values"
        );
        Some(cells)
    } else {
        None
    };
    let expected_xbrl = if xbrl_output {
        assert!(
            project.extra_targets.is_empty(),
            "{sample}: one XBRL target"
        );
        let options = project.target_options.xbrl.as_ref().expect("XBRL target");
        assert_eq!(options.mode(), mapping::XbrlBoundaryMode::ExternalTarget);
        assert_eq!(options.taxonomy(), "Taxonomy\\nanonull.xsd");
        assert_eq!(source.as_repeated().expect("SQLite periods").len(), 4);
        let xbrl = format_xbrl::to_string(&project.target, &expected, options)?;
        assert_eq!(xbrl.len(), 14_198, "{sample}: local instance length");
        assert_eq!(xbrl.matches("<xbrli:context id=").count(), 4);
        assert_eq!(xbrl.matches("<xbrli:unit id=").count(), 2);
        assert_eq!(xbrl.matches(" contextRef=").count(), 100);
        assert!(xbrl.contains("<xbrli:startDate>2009-12-01</xbrli:startDate>"));
        assert!(xbrl.contains("<xbrli:endDate>2010-08-31</xbrli:endDate>"));
        assert!(xbrl.contains(">4342000000</ns1:PassengerRevenue>"));
        assert!(xbrl.contains(">980000000</ns1:NetIncomeLoss>"));
        let transported =
            format_json::from_str(&serde_json::to_string(&expected_json)?, &project.target)?;
        assert_eq!(
            format_xbrl::to_string(&project.target, &transported, options)?,
            xbrl,
            "{sample}: schema-shaped JSON changes the XBRL instance"
        );
        Some(xbrl)
    } else {
        None
    };

    let generated_input = case_dir.join("source.json");
    std::fs::write(&generated_input, source_json)?;
    let project_path = case_dir.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project)?)?;
    let typed_host_schemas = if mapped_xml_output
        || typed_xml_output
        || protobuf_output
        || xlsx_output
        || sqlite_csv_output
        || xbrl_output
    {
        let source_schema = case_dir.join("source-schema.json");
        let target_schema = case_dir.join("target-schema.json");
        std::fs::write(&source_schema, serde_json::to_vec(&project.source)?)?;
        std::fs::write(&target_schema, serde_json::to_vec(&project.target)?)?;
        Some((source_schema, target_schema))
    } else {
        None
    };

    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    let rust_harness = if mapped_xml_output {
        r#"use codegen_runtime::{Value, parse_json, serialize_xml};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = std::fs::read_to_string(args.next().expect("input path"))?;
    let source_schema = std::fs::read_to_string(args.next().expect("source schema path"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema path"))?;
    let source = parse_json(&source_schema, &input)?;
    let output = execute(&source)?;
    let Value::String(xml) = serialize_xml(0, &target_schema, &output, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    print!("{xml}");
    Ok(())
}
"#
    } else if typed_xml_output {
        RECURSIVE_FILTER_RUST_HARNESS
    } else if protobuf_output || xlsx_output || sqlite_csv_output || xbrl_output {
        TYPED_JSON_RUST_HARNESS
    } else {
        r#"use ferrule_generated_mapping::{NamedJsonInput, execute_json_with_sources};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input_path = args.next().expect("input path");
    let input = std::fs::read_to_string(input_path)?;
    let named_args: Vec<_> = args.collect();
    assert_eq!(named_args.len() % 2, 0, "named input arguments are pairs");
    let named_documents = named_args
        .chunks_exact(2)
        .map(|pair| {
            Ok((
                pair[0].to_string_lossy().into_owned(),
                std::fs::read_to_string(&pair[1])?,
            ))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    let named_inputs = named_documents
        .iter()
        .map(|(name, document)| NamedJsonInput { name, document })
        .collect::<Vec<_>>();
    print!("{}", execute_json_with_sources(&input, &named_inputs)?);
    Ok(())
}
"#
    };
    std::fs::write(rust_output.join("src/main.rs"), rust_harness)?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let mut rust_run_command = Command::new("cargo");
    rust_run_command
        .args(["run", "--quiet", "--"])
        .arg(&generated_input)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target);
    if let Some((source_schema, target_schema)) = &typed_host_schemas {
        rust_run_command.arg(source_schema).arg(target_schema);
    } else {
        for (name, path) in &named_input_paths {
            rust_run_command.arg(name).arg(path);
        }
    }
    let rust_run = rust_run_command.isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    if typed_xml_output {
        assert_eq!(
            parse_multi_target_outputs(&rust_run.stdout)?,
            vec![CorpusTargetOutput {
                name: String::new(),
                xml: expected_xml.clone().expect("typed XML output"),
                value: expected_json.clone(),
            }],
            "{sample}: generated Rust typed XML/JSON differs from engine"
        );
    } else if let Some(expected_xml) = &expected_xml {
        assert!(
            rust_run.stdout == expected_xml.as_bytes(),
            "{sample}: generated Rust XML differs from engine"
        );
    } else {
        let rust_json: serde_json::Value = serde_json::from_slice(&rust_run.stdout)?;
        assert_eq!(
            rust_json, expected_json,
            "{sample}: generated Rust differs from engine"
        );
        if let Some(expected_csv) = &expected_csv {
            assert_generated_csv_bytes(
                &project,
                &expected,
                &rust_json,
                expected_csv,
                sample,
                "Rust",
            )?;
        }
        if let Some(expected_protobuf) = &expected_protobuf {
            assert_generated_protobuf_bytes(
                &project,
                &rust_json,
                expected_protobuf,
                sample,
                "Rust",
            )?;
        }
        if let Some(expected_cells) = &expected_xlsx_cells {
            assert_generated_xlsx_cells(&project, &rust_json, expected_cells, sample, "Rust")?;
        }
        if let Some(expected_xbrl) = &expected_xbrl {
            assert_generated_xbrl(&project, &rust_json, expected_xbrl, sample, "Rust")?;
        }
    }

    let csharp_output = case_dir.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    let csharp_harness = if mapped_xml_output {
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var input = File.ReadAllText(args[0]);
var sourceSchema = File.ReadAllText(args[1]);
var targetSchema = File.ReadAllText(args[2]);
var source = FerruleJson.Parse(sourceSchema, input);
var output = GeneratedMapping.Execute(source);
Console.Out.Write(FerruleXml.Serialize(0, targetSchema, output, false, false, null).StringValue);
"#
    } else if typed_xml_output {
        RECURSIVE_FILTER_CSHARP_HARNESS
    } else if protobuf_output || xlsx_output || sqlite_csv_output || xbrl_output {
        TYPED_JSON_CSHARP_HARNESS
    } else {
        r#"using Ferrule.Generated;

var input = File.ReadAllText(args[0]);
if ((args.Length - 1) % 2 != 0)
{
    throw new ArgumentException("Named input arguments are pairs.");
}
var namedInputs = new List<NamedJsonInput>();
for (var index = 1; index < args.Length; index += 2)
{
    namedInputs.Add(new NamedJsonInput(args[index], File.ReadAllText(args[index + 1])));
}
Console.Out.Write(GeneratedMapping.ExecuteJsonWithSources(input, namedInputs));
"#
    };
    std::fs::write(harness.join("Program.cs"), csharp_harness)?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let mut csharp_run_command = dotnet_command(&csharp_output);
    csharp_run_command
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
        .arg(&generated_input)
        .current_dir(&csharp_output);
    if let Some((source_schema, target_schema)) = &typed_host_schemas {
        csharp_run_command.arg(source_schema).arg(target_schema);
        if purchase_orders_xml_output {
            csharp_run_command.arg("http://www.altova.com/IPO");
        }
    } else {
        for (name, path) in &named_input_paths {
            csharp_run_command.arg(name).arg(path);
        }
    }
    let csharp_run = csharp_run_command.isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    if typed_xml_output {
        assert_eq!(
            parse_multi_target_outputs(&csharp_run.stdout)?,
            vec![CorpusTargetOutput {
                name: String::new(),
                xml: expected_xml.clone().expect("typed XML output"),
                value: expected_json.clone(),
            }],
            "{sample}: generated C# typed XML/JSON differs from engine"
        );
    } else if let Some(expected_xml) = &expected_xml {
        assert!(
            csharp_run.stdout == expected_xml.as_bytes(),
            "{sample}: generated C# XML differs from engine"
        );
    } else {
        let csharp_json: serde_json::Value = serde_json::from_slice(&csharp_run.stdout)?;
        assert_eq!(
            csharp_json, expected_json,
            "{sample}: generated C# differs from engine"
        );
        if let Some(expected_csv) = &expected_csv {
            assert_generated_csv_bytes(
                &project,
                &expected,
                &csharp_json,
                expected_csv,
                sample,
                "C#",
            )?;
        }
        if let Some(expected_protobuf) = &expected_protobuf {
            assert_generated_protobuf_bytes(
                &project,
                &csharp_json,
                expected_protobuf,
                sample,
                "C#",
            )?;
        }
        if let Some(expected_cells) = &expected_xlsx_cells {
            assert_generated_xlsx_cells(&project, &csharp_json, expected_cells, sample, "C#")?;
        }
        if let Some(expected_xbrl) = &expected_xbrl {
            assert_generated_xbrl(&project, &csharp_json, expected_xbrl, sample, "C#")?;
        }
    }
    println!("{sample}: generated Rust and C# match the interpreter");
    Ok(())
}

fn collect_recursive_directory_files<'a>(
    directory: &'a serde_json::Value,
    names: &mut Vec<&'a str>,
) -> usize {
    let fields = directory.as_object().expect("recursive directory object");
    assert!(
        fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "directory retains its name attribute"
    );
    if let Some(files) = fields.get("file").and_then(serde_json::Value::as_array) {
        for file in files {
            names.push(file["name"].as_str().expect("recursive file name"));
        }
    }
    let mut directories = 1;
    if let Some(children) = fields
        .get("directory")
        .and_then(serde_json::Value::as_array)
    {
        for child in children {
            directories += collect_recursive_directory_files(child, names);
        }
    }
    directories
}

const RECURSIVE_FILTER_RUST_HARNESS: &str = r#"use codegen_runtime::{Value, parse_json, serialize_json, serialize_xml};
use ferrule_generated_mapping::{execute, execute_json, execute_json_bytes};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = std::fs::read_to_string(args.next().expect("input path"))?;
    let source_schema = std::fs::read_to_string(args.next().expect("source schema path"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema path"))?;
    let source = parse_json(&source_schema, &input)?;
    let output = execute(&source)?;
    let Value::String(xml) = serialize_xml(0, &target_schema, &output, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    let json = serialize_json(&target_schema, &output)?;
    assert_eq!(json, execute_json(&input)?, "typed and JSON generated APIs agree");
    assert_eq!(json.as_bytes(), execute_json_bytes(input.as_bytes())?);
    print!("\0{xml}\0{json}\0");
    Ok(())
}
"#;

const RECURSIVE_FILTER_CSHARP_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var input = File.ReadAllText(args[0]);
var sourceSchema = File.ReadAllText(args[1]);
var targetSchema = File.ReadAllText(args[2]);
var source = FerruleJson.Parse(sourceSchema, input);
var output = GeneratedMapping.Execute(source);
var rootNamespace = args.Length > 3 ? args[3] : null;
var xml = FerruleXml.Serialize(0, targetSchema, output, false, false, rootNamespace).StringValue;
var json = FerruleJson.Serialize(targetSchema, output);
if (json != GeneratedMapping.ExecuteJson(input))
{
    throw new InvalidOperationException("Typed and JSON generated APIs disagree.");
}
if (!System.Text.Encoding.UTF8.GetBytes(json).AsSpan().SequenceEqual(
        GeneratedMapping.ExecuteJsonBytes(System.Text.Encoding.UTF8.GetBytes(input))))
{
    throw new InvalidOperationException("String and byte JSON generated APIs disagree.");
}
Console.Out.Write('\0');
Console.Out.Write(xml);
Console.Out.Write('\0');
Console.Out.Write(json);
Console.Out.Write('\0');
"#;

const TYPED_JSON_RUST_HARNESS: &str = r#"use codegen_runtime::{parse_json, serialize_json};
use ferrule_generated_mapping::{execute, execute_json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = std::fs::read_to_string(args.next().expect("input path"))?;
    let source_schema = std::fs::read_to_string(args.next().expect("source schema path"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema path"))?;
    let source = parse_json(&source_schema, &input)?;
    let output = execute(&source)?;
    let json = serialize_json(&target_schema, &output)?;
    assert_eq!(json, execute_json(&input)?, "typed and JSON generated APIs agree");
    print!("{json}");
    Ok(())
}
"#;

const TYPED_JSON_CSHARP_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var input = File.ReadAllText(args[0]);
var sourceSchema = File.ReadAllText(args[1]);
var targetSchema = File.ReadAllText(args[2]);
var source = FerruleJson.Parse(sourceSchema, input);
var output = GeneratedMapping.Execute(source);
var json = FerruleJson.Serialize(targetSchema, output);
if (json != GeneratedMapping.ExecuteJson(input))
{
    throw new InvalidOperationException("Typed and JSON generated APIs disagree.");
}
Console.Out.Write(json);
"#;

fn corpus_csv_bytes(project: &Project, instance: &Instance) -> TestResult<Vec<u8>> {
    let Instance::Repeated(rows) = instance else {
        panic!("CSV target should contain repeated rows");
    };
    Ok(format_csv::to_string_with_dialect(
        &project.target,
        rows,
        project.target_options.delimiter,
        project.target_options.csv_quote,
        project.target_options.csv_quote_disabled,
        project.target_options.has_header_row.unwrap_or(true),
    )?
    .into_bytes())
}

fn corpus_xlsx_cells(project: &Project, instance: &Instance) -> TestResult<Vec<Instance>> {
    let rows = instance.as_repeated().expect("XLSX target rows");
    let options = &project.target_options;
    let sheet = options.xlsx_sheet.as_deref();
    let start_row = options.xlsx_start_row.unwrap_or(1);
    let columns = &options.xlsx_columns;
    let has_header = options.has_header_row.unwrap_or(true);
    let bytes = format_xlsx::to_bytes_with_options(
        &project.target,
        rows,
        format_xlsx::FlatTableWriteOptions {
            sheet,
            start_row,
            columns,
            headers: &options.xlsx_headers,
            has_header,
        },
    )?;
    let ir::SchemaKind::Group { children, .. } = &project.target.kind else {
        panic!("XLSX target must be a flat group");
    };
    let header_schema = SchemaNode::group(
        "Workbook header",
        children
            .iter()
            .map(|child| SchemaNode::scalar(&child.name, ScalarType::String))
            .collect(),
    );
    let header_rows =
        format_xlsx::from_bytes(&bytes, &header_schema, sheet, start_row, columns, false)?;
    let expected_header = Instance::Group(
        children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let label = options
                    .xlsx_headers
                    .get(index)
                    .unwrap_or(&child.name)
                    .clone();
                (child.name.clone(), Instance::Scalar(Value::String(label)))
            })
            .collect(),
    );
    assert_eq!(
        header_rows.first(),
        Some(&expected_header),
        "XLSX header cells"
    );
    Ok(format_xlsx::from_bytes(
        &bytes,
        &project.target,
        sheet,
        start_row,
        columns,
        has_header,
    )?)
}

fn assert_generated_xlsx_cells(
    project: &Project,
    generated_json: &serde_json::Value,
    expected_cells: &[Instance],
    sample: &str,
    backend: &str,
) -> TestResult<()> {
    let generated =
        format_json::from_str(&serde_json::to_string(generated_json)?, &project.target)?;
    assert_eq!(
        corpus_xlsx_cells(project, &generated)?,
        expected_cells,
        "{sample}: generated {backend} workbook cells differ from the interpreter"
    );
    Ok(())
}

fn assert_generated_xbrl(
    project: &Project,
    generated_json: &serde_json::Value,
    expected_xbrl: &str,
    sample: &str,
    backend: &str,
) -> TestResult<()> {
    let generated =
        format_json::from_str(&serde_json::to_string(generated_json)?, &project.target)?;
    let xbrl = format_xbrl::to_string(
        &project.target,
        &generated,
        project.target_options.xbrl.as_ref().expect("XBRL target"),
    )?;
    assert_eq!(
        xbrl, expected_xbrl,
        "{sample}: generated {backend} XBRL instance differs from the interpreter"
    );
    Ok(())
}

fn assert_generated_csv_bytes(
    project: &Project,
    expected: &Instance,
    generated_json: &serde_json::Value,
    expected_csv: &[u8],
    sample: &str,
    backend: &str,
) -> TestResult<()> {
    let generated =
        format_json::from_str(&serde_json::to_string(generated_json)?, &project.target)?;
    // A CSV mapping may bind fields in a different order from its flat row
    // schema. Both generated hosts cross a JSON boundary that restores schema
    // order; compare the interpreter after the same boundary normalization.
    let normalized_expected = format_json::from_str(
        &format_json::to_string(&project.target, expected)?,
        &project.target,
    )?;
    assert_eq!(
        generated, normalized_expected,
        "{sample}: generated {backend} typed CSV target differs after schema-shaped JSON transport"
    );
    assert_eq!(
        corpus_csv_bytes(project, &generated)?,
        expected_csv,
        "{sample}: generated {backend} CSV bytes differ from engine"
    );
    Ok(())
}

fn assert_generated_protobuf_bytes(
    project: &Project,
    generated_json: &serde_json::Value,
    expected_bytes: &[u8],
    sample: &str,
    backend: &str,
) -> TestResult<()> {
    let options = project
        .target_options
        .protobuf
        .as_ref()
        .expect("Protobuf target");
    let layout = format_protobuf::Layout::parse(&options.schema)?;
    let generated =
        format_json::from_str(&serde_json::to_string(generated_json)?, &project.target)?;
    let bytes = format_protobuf::to_vec(&layout, &options.root_message, &generated)?;
    assert_eq!(
        bytes, expected_bytes,
        "{sample}: generated {backend} Protobuf bytes differ from engine"
    );
    let decoded = format_protobuf::from_slice(&layout, &options.root_message, &bytes)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&format_json::to_string(
            &project.target,
            &decoded
        )?)?,
        *generated_json,
        "{sample}: generated {backend} Protobuf bytes decode to different values"
    );
    Ok(())
}

#[derive(Debug, PartialEq)]
struct CorpusTargetOutput {
    name: String,
    xml: String,
    value: serde_json::Value,
}

fn run_multi_target_case(
    case_dir: &Path,
    rust_target: &Path,
    project: &Project,
    source: &Instance,
    sample: &str,
) -> TestResult<()> {
    assert_eq!(
        project.target_path.as_deref(),
        Some("SecondXML.xml"),
        "{sample}: default output is the third XML component"
    );
    let [named] = project.extra_targets.as_slice() else {
        panic!("{sample}: expected one additional XML target");
    };
    assert_eq!(named.name, "Company", "{sample}: named target identity");
    assert_eq!(
        named.path.as_deref(),
        Some("ExpReport-Target.xml"),
        "{sample}: named target path"
    );
    assert!(named.options.xml_document, "{sample}: named target format");
    assert_eq!(project.target.name, "Company", "{sample}: primary XML root");
    assert_eq!(named.schema.name, "Company", "{sample}: named XML root");
    let namespace = project
        .target
        .xml_namespace
        .as_ref()
        .and_then(ir::XmlNamespace::uri)
        .expect("sample has a qualified XML root");
    assert_eq!(
        named
            .schema
            .xml_namespace
            .as_ref()
            .and_then(ir::XmlNamespace::uri),
        Some(namespace),
        "{sample}: both XML targets use the same root namespace"
    );

    // The source and target XSDs retain recursive mixed-description branches.
    // They are not connected by this mapping, and their repeated local anchors
    // are not representable by the generic JSON codec. Project only those
    // unreferenced branches away for typed-host JSON transport/comparison.
    let mut source_json_schema = project.source.clone();
    let mut primary_json_schema = project.target.clone();
    let mut named_json_schema = named.schema.clone();
    assert!(omit_corpus_description(&mut source_json_schema) > 0);
    assert!(omit_corpus_description(&mut primary_json_schema) > 0);
    assert!(omit_corpus_description(&mut named_json_schema) > 0);
    let source_json = format_json::to_string(&source_json_schema, source)?;
    let expected = engine::run_outputs(project, source)?;
    let round_tripped = format_json::from_str(&source_json, &source_json_schema)?;
    let transported = engine::run_outputs(project, &round_tripped)?;
    assert_eq!(
        transported.primary, expected.primary,
        "{sample}: source JSON transport changed the primary output"
    );
    assert_eq!(
        transported.extras, expected.extras,
        "{sample}: source JSON transport changed the named output"
    );
    let [extra_output] = expected.extras.as_slice() else {
        panic!("{sample}: interpreter must produce one named output");
    };
    assert_eq!(
        extra_output.name, named.name,
        "{sample}: named output order"
    );

    let xml_options = format_xml::XmlWriteOptions {
        declaration: false,
        indent: false,
        default_namespace: None,
    };
    let expected_outputs = [
        ("", &project.target, &primary_json_schema, &expected.primary),
        (
            named.name.as_str(),
            &named.schema,
            &named_json_schema,
            &extra_output.instance,
        ),
    ]
    .into_iter()
    .map(
        |(name, xml_schema, json_schema, value)| -> TestResult<CorpusTargetOutput> {
            Ok(CorpusTargetOutput {
                name: name.to_owned(),
                xml: format_xml::to_string_with_options(xml_schema, value, &xml_options)?,
                value: serde_json::from_str(&format_json::to_string(json_schema, value)?)?,
            })
        },
    )
    .collect::<TestResult<Vec<_>>>()?;
    assert!(
        expected_outputs[0].xml.contains("<DomesticAcc"),
        "{sample}: primary maps accommodation expenses"
    );
    assert!(
        !expected_outputs[0].xml.contains("<Travel"),
        "{sample}: primary excludes travel expenses"
    );
    assert!(
        expected_outputs[1].xml.contains("<Travel"),
        "{sample}: named output maps travel expenses"
    );
    assert!(
        !expected_outputs[1].xml.contains("<DomesticAcc"),
        "{sample}: named output excludes accommodation expenses"
    );
    let primary_items = expected_outputs[0].value["Employee"][0]["expense-item"]
        .as_array()
        .expect("primary employee expense items");
    assert_eq!(
        primary_items.len(),
        2,
        "{sample}: primary lodging and meal items"
    );
    assert_eq!(
        primary_items[0]["Accommodation"][0]["DomesticAcc"][0]["DomesticAcc-Cost"], 121.2,
        "{sample}: lodging cost reaches the primary target"
    );
    let named_items = expected_outputs[1].value["Employee"][0]["expense-item"]
        .as_array()
        .expect("named employee expense items");
    assert_eq!(named_items.len(), 3, "{sample}: named travel items");
    assert_eq!(
        named_items
            .iter()
            .map(|item| item["Travel"][0]["Travel-Cost"].as_f64())
            .collect::<Vec<_>>(),
        [Some(337.88), Some(1014.22), Some(2000.0)],
        "{sample}: travel costs reach the named target in source order"
    );

    let source_schema = case_dir.join("source-schema.json");
    let primary_xml_schema = case_dir.join("primary-xml-schema.json");
    let primary_json_schema_path = case_dir.join("primary-json-schema.json");
    let named_xml_schema = case_dir.join("named-xml-schema.json");
    let named_json_schema_path = case_dir.join("named-json-schema.json");
    let source_path = case_dir.join("source.json");
    let project_path = case_dir.join("project.json");
    std::fs::write(&source_schema, serde_json::to_vec(&source_json_schema)?)?;
    std::fs::write(&primary_xml_schema, serde_json::to_vec(&project.target)?)?;
    std::fs::write(
        &primary_json_schema_path,
        serde_json::to_vec(&primary_json_schema)?,
    )?;
    std::fs::write(&named_xml_schema, serde_json::to_vec(&named.schema)?)?;
    std::fs::write(
        &named_json_schema_path,
        serde_json::to_vec(&named_json_schema)?,
    )?;
    std::fs::write(&source_path, source_json)?;
    std::fs::write(&project_path, serde_json::to_vec_pretty(project)?)?;

    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(rust_output.join("src/main.rs"), MULTI_TARGET_RUST_HARNESS)?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let rust_run = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&source_schema)
        .arg(&primary_xml_schema)
        .arg(&primary_json_schema_path)
        .arg(&named_xml_schema)
        .arg(&named_json_schema_path)
        .arg(&source_path)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    assert_eq!(
        parse_multi_target_outputs(&rust_run.stdout)?,
        expected_outputs,
        "{sample}: generated Rust target order or contents differ from engine"
    );

    let csharp_output = case_dir.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(harness.join("Program.cs"), MULTI_TARGET_CSHARP_HARNESS)?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let csharp_run = dotnet_command(&csharp_output)
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
        .arg(&source_schema)
        .arg(&primary_xml_schema)
        .arg(&primary_json_schema_path)
        .arg(&named_xml_schema)
        .arg(&named_json_schema_path)
        .arg(&source_path)
        .arg(namespace)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    assert_eq!(
        parse_multi_target_outputs(&csharp_run.stdout)?,
        expected_outputs,
        "{sample}: generated C# target order or contents differ from engine"
    );
    println!("{sample}: generated Rust and C# output sets match the interpreter");
    Ok(())
}

fn parse_multi_target_outputs(bytes: &[u8]) -> TestResult<Vec<CorpusTargetOutput>> {
    let text = std::str::from_utf8(bytes)?;
    let mut fields = text.split('\0').collect::<Vec<_>>();
    assert_eq!(fields.pop(), Some(""), "missing named output terminator");
    let (records, remainder) = fields.as_chunks::<3>();
    assert!(remainder.is_empty(), "incomplete named output");
    records
        .iter()
        .map(|fields| {
            Ok(CorpusTargetOutput {
                name: fields[0].to_owned(),
                xml: fields[1].to_owned(),
                value: serde_json::from_str(fields[2])?,
            })
        })
        .collect()
}

fn omit_corpus_description(schema: &mut SchemaNode) -> usize {
    // XML compositor metadata can reference description; the JSON projection
    // carries only ordinary named fields, not XML occurrence ordering.
    schema.xml_repeating_sequences.clear();
    schema.xml_repeating_choices.clear();
    let ir::SchemaKind::Group {
        children, dynamic, ..
    } = &mut schema.kind
    else {
        return 0;
    };
    let previous = children.len();
    children.retain(|child| child.name != "description");
    let mut removed = previous - children.len();
    for child in children {
        removed += omit_corpus_description(child);
    }
    if let Some(dynamic) = dynamic {
        removed += omit_corpus_description(dynamic);
    }
    removed
}

const MULTI_TARGET_RUST_HARNESS: &str = r#"use codegen_runtime::{Instance, Value, parse_json, serialize_json, serialize_xml};
use ferrule_generated_mapping::execute_outputs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source_schema = std::fs::read_to_string(args.next().expect("source schema"))?;
    let primary_xml_schema = std::fs::read_to_string(args.next().expect("primary XML schema"))?;
    let primary_json_schema = std::fs::read_to_string(args.next().expect("primary JSON schema"))?;
    let named_xml_schema = std::fs::read_to_string(args.next().expect("named XML schema"))?;
    let named_json_schema = std::fs::read_to_string(args.next().expect("named JSON schema"))?;
    let source_json = std::fs::read_to_string(args.next().expect("source JSON"))?;
    let source = parse_json(&source_schema, &source_json)?;
    let outputs = execute_outputs(&source)?;
    assert_eq!(outputs.extras.len(), 1, "expected one named output");
    print_output("", &outputs.primary, &primary_xml_schema, &primary_json_schema)?;
    for output in &outputs.extras {
        print_output(output.name, &output.instance, &named_xml_schema, &named_json_schema)?;
    }
    Ok(())
}

fn print_output(name: &str, value: &Instance, xml_schema: &str, json_schema: &str) -> Result<(), Box<dyn std::error::Error>> {
    let Value::String(xml) = serialize_xml(0, xml_schema, value, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    let json = serialize_json(json_schema, value)?;
    print!("{name}\0{xml}\0{json}\0");
    Ok(())
}
"#;

const MULTI_TARGET_CSHARP_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var sourceSchema = File.ReadAllText(args[0]);
var primaryXmlSchema = File.ReadAllText(args[1]);
var primaryJsonSchema = File.ReadAllText(args[2]);
var namedXmlSchema = File.ReadAllText(args[3]);
var namedJsonSchema = File.ReadAllText(args[4]);
var source = FerruleJson.Parse(sourceSchema, File.ReadAllText(args[5]));
var rootNamespace = args[6];
var outputs = GeneratedMapping.ExecuteOutputs(source);
if (outputs.Extras.Count != 1)
{
    throw new InvalidOperationException("Expected one named output.");
}
WriteOutput("", outputs.Primary, primaryXmlSchema, primaryJsonSchema, rootNamespace);
foreach (var output in outputs.Extras)
{
    WriteOutput(output.Name, output.Instance, namedXmlSchema, namedJsonSchema, rootNamespace);
}

static void WriteOutput(string name, FerruleInstance value, string xmlSchema, string jsonSchema, string rootNamespace)
{
    var xml = FerruleXml.Serialize(0, xmlSchema, value, false, false, rootNamespace).StringValue;
    var json = FerruleJson.Serialize(jsonSchema, value);
    Console.Out.Write(name);
    Console.Out.Write('\0');
    Console.Out.Write(xml);
    Console.Out.Write('\0');
    Console.Out.Write(json);
    Console.Out.Write('\0');
}
"#;

fn run_xml_nil_case(
    case_dir: &Path,
    rust_target: &Path,
    project: &Project,
    source: &Instance,
    sample: &str,
) -> TestResult<()> {
    assert!(project.extra_targets.is_empty(), "{sample}: one XML target");
    let mut nil_paths = Vec::new();
    collect_corpus_nil_paths(source, &mut Vec::new(), &mut nil_paths);
    assert_eq!(
        nil_paths
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "Office/0/Fax",
            "Office/0/Address/0/state",
            "Office/0/Address/0/street",
            "Office/0/Address/0/zip",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        "{sample}: four intended XML nil source values"
    );
    let mut json_source = source.clone();
    replace_corpus_nil_with_null(&mut json_source);
    let source_json = format_json::to_string(&project.source, &json_source)?;
    let mut restored = format_json::from_str(&source_json, &project.source)?;
    for path in XML_NIL_PATHS {
        restore_corpus_nil(&mut restored, path);
    }
    let expected = engine::run(project, source)?;
    assert_eq!(
        engine::run(project, &restored)?,
        expected,
        "{sample}: typed XML nil restoration changed mapping output"
    );
    let expected_xml = format_xml::to_string_with_options(
        &project.target,
        &expected,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
        },
    )?;
    assert_eq!(
        expected_xml.matches("xsi:nil=\"true\"").count(),
        1,
        "{sample}: explicit nil target"
    );
    assert!(
        expected_xml.contains("<Fax>n/a</Fax>"),
        "{sample}: missing-value substitution"
    );
    assert!(
        !expected_xml.contains("<Address>"),
        "{sample}: nil-sensitive address filter"
    );

    let source_path = case_dir.join("source.json");
    let source_schema = case_dir.join("source-schema.json");
    let target_schema = case_dir.join("target-schema.json");
    let project_path = case_dir.join("project.json");
    std::fs::write(&source_path, source_json)?;
    std::fs::write(&source_schema, serde_json::to_vec(&project.source)?)?;
    std::fs::write(&target_schema, serde_json::to_vec(&project.target)?)?;
    std::fs::write(&project_path, serde_json::to_vec_pretty(project)?)?;

    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(rust_output.join("src/main.rs"), XML_NIL_RUST_HARNESS)?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let rust_run = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&source_schema)
        .arg(&target_schema)
        .arg(&source_path)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    assert_eq!(
        rust_run.stdout,
        expected_xml.as_bytes(),
        "{sample}: generated Rust XML nil output differs from engine"
    );

    let csharp_output = case_dir.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(harness.join("Program.cs"), XML_NIL_CSHARP_HARNESS)?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let csharp_run = dotnet_command(&csharp_output)
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
        .arg(&source_schema)
        .arg(&target_schema)
        .arg(&source_path)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    assert_eq!(
        csharp_run.stdout,
        expected_xml.as_bytes(),
        "{sample}: generated C# XML nil output differs from engine"
    );
    println!("{sample}: generated Rust and C# XML nil results match the interpreter");
    Ok(())
}

const XML_NIL_PATHS: &[&[&str]] = &[
    &["Office", "0", "Fax"],
    &["Office", "0", "Address", "0", "state"],
    &["Office", "0", "Address", "0", "street"],
    &["Office", "0", "Address", "0", "zip"],
];

fn collect_corpus_nil_paths(instance: &Instance, path: &mut Vec<String>, result: &mut Vec<String>) {
    match instance {
        Instance::Scalar(Value::XmlNil(_)) => result.push(path.join("/")),
        Instance::Scalar(_) => {}
        Instance::Group(fields) => {
            for (name, value) in fields {
                path.push(name.clone());
                collect_corpus_nil_paths(value, path, result);
                path.pop();
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for (index, value) in items.iter().enumerate() {
                path.push(index.to_string());
                collect_corpus_nil_paths(value, path, result);
                path.pop();
            }
        }
        Instance::DocumentSet(_) => panic!("XML nil sample has one source document"),
    }
}

fn replace_corpus_nil_with_null(instance: &mut Instance) {
    match instance {
        Instance::Scalar(value @ Value::XmlNil(_)) => *value = Value::Null,
        Instance::Scalar(_) => {}
        Instance::Group(fields) => {
            for (_, value) in fields {
                replace_corpus_nil_with_null(value);
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for value in items {
                replace_corpus_nil_with_null(value);
            }
        }
        Instance::DocumentSet(_) => panic!("XML nil sample has one source document"),
    }
}

fn restore_corpus_nil(instance: &mut Instance, path: &[&str]) {
    if path.is_empty() {
        *instance = Instance::Scalar(Value::xml_nil());
        return;
    }
    match instance {
        Instance::Group(fields) => {
            if let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == path[0]) {
                restore_corpus_nil(value, &path[1..]);
            } else {
                assert_eq!(path.len(), 1, "only a terminal nil field may be absent");
                fields.push((path[0].to_owned(), Instance::Scalar(Value::xml_nil())));
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            let index = path[0].parse::<usize>().expect("source item index");
            restore_corpus_nil(&mut items[index], &path[1..]);
        }
        _ => panic!("XML nil path traverses a non-container"),
    }
}

const XML_NIL_RUST_HARNESS: &str = r#"use codegen_runtime::{Instance, Value, parse_json, serialize_xml};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source_schema = std::fs::read_to_string(args.next().expect("source schema"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema"))?;
    let source_json = std::fs::read_to_string(args.next().expect("source JSON"))?;
    let mut source = parse_json(&source_schema, &source_json)?;
    for path in [
        &["Office", "0", "Fax"][..],
        &["Office", "0", "Address", "0", "state"][..],
        &["Office", "0", "Address", "0", "street"][..],
        &["Office", "0", "Address", "0", "zip"][..],
    ] {
        restore_nil(&mut source, path);
    }
    let output = execute(&source)?;
    let Value::String(xml) = serialize_xml(0, &target_schema, &output, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    print!("{xml}");
    Ok(())
}

fn restore_nil(instance: &mut Instance, path: &[&str]) {
    if path.is_empty() {
        *instance = Instance::Scalar(Value::xml_nil());
        return;
    }
    match instance {
        Instance::Group(fields) => {
            if let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == path[0]) {
                restore_nil(value, &path[1..]);
            } else {
                assert_eq!(path.len(), 1, "only a terminal nil field may be absent");
                fields.push((path[0].to_owned(), Instance::Scalar(Value::xml_nil())));
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            let index = path[0].parse::<usize>().expect("source item index");
            restore_nil(&mut items[index], &path[1..]);
        }
        _ => panic!("XML nil path traverses a non-container"),
    }
}
"#;

const XML_NIL_CSHARP_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var sourceSchema = File.ReadAllText(args[0]);
var targetSchema = File.ReadAllText(args[1]);
var source = FerruleJson.Parse(sourceSchema, File.ReadAllText(args[2]));
foreach (var path in new[]
{
    new[] { "Office", "0", "Fax" },
    new[] { "Office", "0", "Address", "0", "state" },
    new[] { "Office", "0", "Address", "0", "street" },
    new[] { "Office", "0", "Address", "0", "zip" },
})
{
    source = RestoreNil(source, path, 0);
}
var output = GeneratedMapping.Execute(source);
Console.Out.Write(FerruleXml.Serialize(0, targetSchema, output, false, false, null).StringValue);

static FerruleInstance RestoreNil(FerruleInstance instance, string[] path, int offset)
{
    if (offset == path.Length)
    {
        return new FerruleScalar(FerruleValue.XmlNil);
    }
    if (instance is FerruleGroup group)
    {
        var fields = group.Fields.ToList();
        var index = fields.FindIndex(field => field.Name == path[offset]);
        if (index < 0)
        {
            if (offset + 1 != path.Length)
            {
                throw new InvalidOperationException("Only a terminal nil field may be absent.");
            }
            fields.Add(new FerruleField(path[offset], new FerruleScalar(FerruleValue.XmlNil)));
        }
        else
        {
            fields[index] = new FerruleField(
                path[offset], RestoreNil(fields[index].Value, path, offset + 1));
        }
        return new FerruleGroup(fields);
    }
    if (instance is FerruleRepeated repeated)
    {
        var items = repeated.Items.ToArray();
        var index = int.Parse(path[offset], System.Globalization.CultureInfo.InvariantCulture);
        items[index] = RestoreNil(items[index], path, offset + 1);
        return new FerruleRepeated(items);
    }
    if (instance is FerruleMappedSequence mapped)
    {
        var items = mapped.Items.ToArray();
        var index = int.Parse(path[offset], System.Globalization.CultureInfo.InvariantCulture);
        items[index] = RestoreNil(items[index], path, offset + 1);
        return new FerruleMappedSequence(items);
    }
    throw new InvalidOperationException("XML nil path traverses a non-container.");
}
"#;

fn run_failure_rule_case(
    case_dir: &Path,
    rust_target: &Path,
    project: &Project,
    source: &Instance,
    sample: &str,
) -> TestResult<()> {
    assert!(project.extra_sources.is_empty(), "{sample}: one XML source");
    assert!(project.extra_targets.is_empty(), "{sample}: one XML target");
    assert_eq!(project.failure_rules.len(), 1, "{sample}: one failure rule");
    let expected_failure = engine::EngineError::MappingFailure {
        rule: 1,
        message: Some("Expense limit exceeded!".into()),
    };
    assert_eq!(
        engine::run(project, source).unwrap_err(),
        expected_failure,
        "{sample}: native XML input must fail before target construction"
    );
    let failing_json = format_json::to_string(&project.source, source)?;
    let transported = format_json::from_str(&failing_json, &project.source)?;
    assert_eq!(
        transported, *source,
        "{sample}: XML source must survive schema-shaped JSON transport"
    );
    assert_eq!(
        engine::run(project, &transported).unwrap_err(),
        expected_failure,
        "{sample}: source transport changed the controlled failure"
    );

    // Keep the original sample untouched. Moving its one over-limit expense
    // below the threshold exercises the same mapping's successful branch.
    let mut successful_document: serde_json::Value = serde_json::from_str(&failing_json)?;
    let expenses = successful_document["expense-item"]
        .as_array_mut()
        .expect("sample has repeated expense items");
    assert_eq!(expenses.len(), 4, "{sample}: four input expenses");
    assert_eq!(expenses[2]["expense"], 299.45);
    expenses[2]["expense"] = serde_json::json!(199.45);
    let successful_json = serde_json::to_string(&successful_document)?;
    let successful_source = format_json::from_str(&successful_json, &project.source)?;
    let expected = engine::run(project, &successful_source)?;
    let expected_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?;
    assert_eq!(expected_json["Person"]["FullName"], "Fred Landis");
    let mapped_expenses = expected_json["expense-item"]
        .as_array()
        .expect("successful output has expense items");
    assert_eq!(mapped_expenses.len(), 4, "{sample}: all expenses pass");
    assert_eq!(
        mapped_expenses
            .iter()
            .map(|item| item["expense"].as_f64())
            .collect::<Vec<_>>(),
        [Some(122.11), Some(122.12), Some(199.45), Some(13.22)],
        "{sample}: successful items retain source order"
    );
    let expected_xml = format_xml::to_string_with_options(
        &project.target,
        &expected,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
        },
    )?;
    let expected_outputs = vec![CorpusTargetOutput {
        name: String::new(),
        xml: expected_xml,
        value: expected_json,
    }];

    let project_path = case_dir.join("project.json");
    let source_schema_path = case_dir.join("source-schema.json");
    let target_schema_path = case_dir.join("target-schema.json");
    let failing_path = case_dir.join("failing.json");
    let successful_path = case_dir.join("successful.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(project)?)?;
    std::fs::write(&source_schema_path, serde_json::to_vec(&project.source)?)?;
    std::fs::write(&target_schema_path, serde_json::to_vec(&project.target)?)?;
    std::fs::write(&failing_path, failing_json)?;
    std::fs::write(&successful_path, successful_json)?;

    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(rust_output.join("src/main.rs"), FAILURE_RULE_RUST_HARNESS)?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let rust_run = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .arg(&source_schema_path)
        .arg(&target_schema_path)
        .arg(&failing_path)
        .arg(&successful_path)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    assert_eq!(
        parse_multi_target_outputs(&rust_run.stdout)?,
        expected_outputs,
        "{sample}: generated Rust successful XML/JSON differs from interpreter"
    );

    let csharp_output = case_dir.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(harness.join("Program.cs"), FAILURE_RULE_CSHARP_HARNESS)?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let csharp_run = dotnet_command(&csharp_output)
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
        .arg(&source_schema_path)
        .arg(&target_schema_path)
        .arg(&failing_path)
        .arg(&successful_path)
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    assert_eq!(
        parse_multi_target_outputs(&csharp_run.stdout)?,
        expected_outputs,
        "{sample}: generated C# successful XML/JSON differs from interpreter"
    );
    println!("{sample}: generated Rust and C# match controlled failure and successful output");
    Ok(())
}

const FAILURE_RULE_RUST_HARNESS: &str = r#"use codegen_runtime::{
    JsonBoundaryError, RuntimeError, Value, parse_json, serialize_json, serialize_xml,
};
use ferrule_generated_mapping::{execute, execute_json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source_schema = std::fs::read_to_string(args.next().expect("source schema"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema"))?;
    let failing_json = std::fs::read_to_string(args.next().expect("failing source"))?;
    let successful_json = std::fs::read_to_string(args.next().expect("successful source"))?;
    let failing_source = parse_json(&source_schema, &failing_json)?;
    let failure = RuntimeError::MappingFailure {
        rule: 1,
        message: Some("Expense limit exceeded!".into()),
    };
    assert_eq!(execute(&failing_source), Err(failure));
    assert_eq!(
        execute_json(&failing_json),
        Err(JsonBoundaryError::Execution(RuntimeError::MappingFailure {
            rule: 1,
            message: Some("Expense limit exceeded!".into()),
        })),
    );

    let successful_source = parse_json(&source_schema, &successful_json)?;
    let output = execute(&successful_source)?;
    let json = serialize_json(&target_schema, &output)?;
    assert_eq!(json, execute_json(&successful_json)?, "typed and JSON APIs agree");
    let Value::String(xml) = serialize_xml(0, &target_schema, &output, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    print!("\0{xml}\0{json}\0");
    Ok(())
}
"#;

const FAILURE_RULE_CSHARP_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var sourceSchema = File.ReadAllText(args[0]);
var targetSchema = File.ReadAllText(args[1]);
var failingJson = File.ReadAllText(args[2]);
var successfulJson = File.ReadAllText(args[3]);
var failingSource = FerruleJson.Parse(sourceSchema, failingJson);
CheckFailure(() => GeneratedMapping.Execute(failingSource));
CheckFailure(() => GeneratedMapping.ExecuteJson(failingJson));

var successfulSource = FerruleJson.Parse(sourceSchema, successfulJson);
var output = GeneratedMapping.Execute(successfulSource);
var json = FerruleJson.Serialize(targetSchema, output);
if (json != GeneratedMapping.ExecuteJson(successfulJson))
{
    throw new InvalidOperationException("Typed and JSON APIs disagree.");
}
var xml = FerruleXml.Serialize(0, targetSchema, output, false, false, null).StringValue;
Console.Out.Write('\0');
Console.Out.Write(xml);
Console.Out.Write('\0');
Console.Out.Write(json);
Console.Out.Write('\0');

static void CheckFailure(Action action)
{
    try
    {
        action();
        throw new InvalidOperationException("Expected the expense-limit failure.");
    }
    catch (FerruleRuntimeException exception)
    {
        if (exception.Error != FerruleRuntimeError.MappingFailure
            || exception.FailureRule != 1
            || exception.MappingFailureMessage != "Expense limit exceeded!")
        {
            throw new InvalidOperationException("Controlled failure changed.", exception);
        }
    }
}
"#;

struct CorpusDynamicXmlLoader {
    source_name: String,
    documents: Vec<(String, std::sync::Arc<Instance>)>,
    calls: std::cell::RefCell<Vec<(String, String)>>,
}

impl engine::DynamicSourceLoader for CorpusDynamicXmlLoader {
    fn load(&self, source: &str, path: &str) -> Result<std::sync::Arc<Instance>, String> {
        self.calls
            .borrow_mut()
            .push((source.to_owned(), path.to_owned()));
        if source != self.source_name {
            return Err(format!("unexpected dynamic source `{source}`"));
        }
        self.documents
            .iter()
            .find(|(allowed, _)| allowed == path)
            .map(|(_, document)| std::sync::Arc::clone(document))
            .ok_or_else(|| format!("dynamic source path is outside the two-file corpus: {path}"))
    }
}

fn run_dynamic_source_case(
    case_dir: &Path,
    rust_target: &Path,
    mapping_path: &Path,
    project: &Project,
    source: &Instance,
    sample: &str,
) -> TestResult<()> {
    assert!(project.extra_targets.is_empty(), "{sample}: one XML target");
    let [dynamic] = project.extra_sources.as_slice() else {
        panic!("{sample}: exactly one dynamic XML source");
    };
    assert!(
        dynamic.dynamic_path.is_some(),
        "{sample}: per-file source path"
    );
    assert!(dynamic.options.xml_document, "{sample}: dynamic XML source");
    let files = source
        .field("File")
        .and_then(Instance::as_repeated)
        .expect("file manifest has repeated File elements");
    let file_names = files
        .iter()
        .map(|file| match file.as_scalar() {
            Some(Value::String(name)) => name.as_str(),
            _ => panic!("{sample}: manifest filename must be a string"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        file_names,
        ["Nanonull-HQ.xml", "Nanonull-Branch.xml"],
        "{sample}: only the two intended local files are loaded"
    );

    let source_json = format_json::to_string(&project.source, source)?;
    let round_tripped = format_json::from_str(&source_json, &project.source)?;
    assert_eq!(
        round_tripped, *source,
        "{sample}: manifest XML to schema-shaped JSON transport"
    );
    let source_json_path = case_dir.join("source.json");
    std::fs::write(&source_json_path, &source_json)?;
    let source_schema_path = case_dir.join("source-schema.json");
    let dynamic_schema_path = case_dir.join("dynamic-schema.json");
    let target_schema_path = case_dir.join("target-schema.json");
    std::fs::write(&source_schema_path, serde_json::to_vec(&project.source)?)?;
    std::fs::write(&dynamic_schema_path, serde_json::to_vec(&dynamic.schema)?)?;
    std::fs::write(&target_schema_path, serde_json::to_vec(&project.target)?)?;

    let sample_root = mapping_path.parent().expect("sample has a directory");
    let mut documents = Vec::new();
    let mut payloads = Vec::new();
    for (index, name) in file_names.iter().enumerate() {
        let path = sample_root.join(name).canonicalize()?;
        assert!(
            path.starts_with(sample_root),
            "{sample}: confined source path"
        );
        let instance = format_xml::read(&path, &dynamic.schema)?;
        let json = format_json::to_string(&dynamic.schema, &instance)?;
        assert_eq!(
            format_json::from_str(&json, &dynamic.schema)?,
            instance,
            "{sample}: dynamic XML document survives JSON transport"
        );
        let payload_path = case_dir.join(format!("dynamic-{index}.json"));
        std::fs::write(&payload_path, json)?;
        let path = path
            .to_str()
            .expect("local corpus source paths are UTF-8")
            .to_owned();
        documents.push((path.clone(), std::sync::Arc::new(instance)));
        payloads.push((path, payload_path));
    }
    let loader = CorpusDynamicXmlLoader {
        source_name: dynamic.name.clone(),
        documents,
        calls: std::cell::RefCell::new(Vec::new()),
    };
    let expected_calls = payloads
        .iter()
        .map(|(path, _)| (dynamic.name.clone(), path.clone()))
        .collect::<Vec<_>>();
    let execution = engine::ExecutionContext::new(mapping_path).with_dynamic_source_loader(&loader);
    let native_expected = engine::run_with_context(project, source, &execution)?;
    assert_eq!(
        loader.calls.borrow().as_slice(),
        expected_calls.as_slice(),
        "{sample}: interpreter dynamic request order"
    );
    loader.calls.borrow_mut().clear();
    let expected = engine::run_with_context(project, &round_tripped, &execution)?;
    assert_eq!(
        loader.calls.borrow().as_slice(),
        expected_calls.as_slice(),
        "{sample}: transported-source dynamic request order"
    );
    assert_eq!(
        expected, native_expected,
        "{sample}: schema-shaped JSON changed interpreter output"
    );
    let expected_json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?;
    assert_eq!(expected_json["Name"], "Organization Chart");
    let offices = expected_json["Office"].as_array().expect("merged offices");
    assert_eq!(offices.len(), 2, "{sample}: one office from each file");
    assert_eq!(offices[0]["Name"], "Nanonull, Inc.");
    assert_eq!(offices[1]["Name"], "Nanonull Partners, Inc.");
    for (office, (path, _)) in offices.iter().zip(&payloads) {
        assert_eq!(
            office["Desc"],
            format!("read from file: {path}"),
            "{sample}: each dynamic document retains its own path"
        );
    }
    let expected_xml = format_xml::to_string_with_options(
        &project.target,
        &expected,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
        },
    )?;
    let expected_outputs = vec![CorpusTargetOutput {
        name: String::new(),
        xml: expected_xml,
        value: expected_json,
    }];

    let project_path = case_dir.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(project)?)?;
    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(rust_output.join("src/main.rs"), DYNAMIC_SOURCE_RUST_HARNESS)?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let mut rust_run = Command::new("cargo");
    rust_run
        .args(["run", "--quiet", "--"])
        .arg(&source_json_path)
        .arg(&source_schema_path)
        .arg(&dynamic_schema_path)
        .arg(&target_schema_path)
        .arg(mapping_path)
        .arg(&dynamic.name)
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target);
    for (path, payload) in &payloads {
        rust_run.arg(path).arg(payload);
    }
    let rust_run = rust_run.isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    assert_eq!(
        parse_multi_target_outputs(&rust_run.stdout)?,
        expected_outputs,
        "{sample}: generated Rust dynamic XML/JSON differs from interpreter"
    );

    let csharp_output = case_dir.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(harness.join("Program.cs"), DYNAMIC_SOURCE_CSHARP_HARNESS)?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let mut csharp_run = dotnet_command(&csharp_output);
    csharp_run
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
        .arg(&source_json_path)
        .arg(&source_schema_path)
        .arg(&dynamic_schema_path)
        .arg(&target_schema_path)
        .arg(mapping_path)
        .arg(&dynamic.name)
        .current_dir(&csharp_output);
    for (path, payload) in &payloads {
        csharp_run.arg(path).arg(payload);
    }
    let csharp_run = csharp_run.isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    assert_eq!(
        parse_multi_target_outputs(&csharp_run.stdout)?,
        expected_outputs,
        "{sample}: generated C# dynamic XML/JSON differs from interpreter"
    );
    println!("{sample}: generated Rust and C# dynamic loaders match the interpreter");
    Ok(())
}

const DYNAMIC_SOURCE_RUST_HARNESS: &str = r#"use std::cell::RefCell;
use std::path::PathBuf;
use codegen_runtime::{
    DynamicJsonSourceLoader, DynamicSourceLoader, ExecutionContext, Instance, Value,
    parse_json, serialize_json, serialize_xml,
};
use ferrule_generated_mapping::{
    execute_json_with_sources_context_and_dynamic_source_loader,
    execute_with_sources_context_and_dynamic_source_loader,
};

struct Loader {
    source_name: String,
    dynamic_schema: String,
    payloads: Vec<(String, Vec<u8>)>,
    calls: RefCell<Vec<String>>,
}

impl Loader {
    fn payload(&self, source: &str, path: &str) -> Result<Vec<u8>, String> {
        if source != self.source_name {
            return Err(format!("unexpected dynamic source {source}"));
        }
        self.calls.borrow_mut().push(path.to_owned());
        self.payloads
            .iter()
            .find(|(allowed, _)| allowed == path)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| format!("unconfined dynamic path {path}"))
    }
}

impl DynamicSourceLoader for Loader {
    fn load(&self, source: &str, path: &str) -> Result<Instance, String> {
        let bytes = self.payload(source, path)?;
        let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
        parse_json(&self.dynamic_schema, text).map_err(|error| error.to_string())
    }
}

impl DynamicJsonSourceLoader for Loader {
    fn load(&self, source: &str, path: &str) -> Result<Vec<u8>, String> {
        self.payload(source, path)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = std::fs::read_to_string(args.next().expect("source JSON"))?;
    let source_schema = std::fs::read_to_string(args.next().expect("source schema"))?;
    let dynamic_schema = std::fs::read_to_string(args.next().expect("dynamic schema"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema"))?;
    let mapping_path = PathBuf::from(args.next().expect("mapping path"));
    let source_name = args.next().expect("dynamic source name").to_string_lossy().into_owned();
    let pairs = args.collect::<Vec<_>>();
    assert_eq!(pairs.len(), 4, "exactly two dynamic source path/payload pairs");
    let mut payloads = Vec::new();
    for pair in pairs.chunks_exact(2) {
        payloads.push((
            pair[0].to_str().expect("UTF-8 dynamic path").to_owned(),
            std::fs::read(&pair[1])?,
        ));
    }
    let expected_paths = payloads.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>();
    let loader = Loader {
        source_name,
        dynamic_schema,
        payloads,
        calls: RefCell::new(Vec::new()),
    };
    let execution = ExecutionContext::new(&mapping_path);
    let source = parse_json(&source_schema, &input)?;
    let output = execute_with_sources_context_and_dynamic_source_loader(
        &source, &[], &execution, &loader,
    )?;
    assert_eq!(loader.calls.borrow().as_slice(), expected_paths.as_slice());
    loader.calls.borrow_mut().clear();
    let json = serialize_json(&target_schema, &output)?;
    assert_eq!(
        json,
        execute_json_with_sources_context_and_dynamic_source_loader(
            &input, &[], &execution, &loader,
        )?,
        "typed and JSON generated APIs agree",
    );
    assert_eq!(loader.calls.borrow().as_slice(), expected_paths.as_slice());
    let Value::String(xml) = serialize_xml(0, &target_schema, &output, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    print!("\0{xml}\0{json}\0");
    Ok(())
}
"#;

const DYNAMIC_SOURCE_CSHARP_HARNESS: &str = r#"using System.Text;
using Ferrule.Generated;
using Ferrule.Runtime;

var input = File.ReadAllText(args[0]);
var sourceSchema = File.ReadAllText(args[1]);
var dynamicSchema = File.ReadAllText(args[2]);
var targetSchema = File.ReadAllText(args[3]);
var execution = new FerruleExecutionContext(args[4]);
if (args.Length != 10)
{
    throw new ArgumentException("Exactly two dynamic source path/payload pairs are required.");
}
var payloads = new Dictionary<string, byte[]>(StringComparer.Ordinal);
var expectedPaths = new List<string>();
for (var index = 6; index < args.Length; index += 2)
{
    expectedPaths.Add(args[index]);
    if (!payloads.TryAdd(args[index], File.ReadAllBytes(args[index + 1])))
    {
        throw new InvalidOperationException("Duplicate dynamic source path.");
    }
}
var loader = new Loader(args[5], dynamicSchema, payloads);
var source = FerruleJson.Parse(sourceSchema, input);
var output = GeneratedMapping.ExecuteWithSourcesContextAndDynamicSourceLoader(
    source, Array.Empty<NamedInput>(), execution, loader);
if (!loader.Calls.SequenceEqual(expectedPaths))
{
    throw new InvalidOperationException("Typed dynamic load order changed.");
}
loader.Calls.Clear();
var json = FerruleJson.Serialize(targetSchema, output);
var jsonApi = GeneratedMapping.ExecuteJsonWithSourcesContextAndDynamicSourceLoader(
    input, Array.Empty<NamedJsonInput>(), execution, loader);
if (json != jsonApi || !loader.Calls.SequenceEqual(expectedPaths))
{
    throw new InvalidOperationException("JSON dynamic loads or output changed.");
}
var xml = FerruleXml.Serialize(0, targetSchema, output, false, false, null).StringValue;
Console.Out.Write('\0');
Console.Out.Write(xml);
Console.Out.Write('\0');
Console.Out.Write(json);
Console.Out.Write('\0');

sealed class Loader : IFerruleDynamicSourceLoader, IFerruleDynamicJsonSourceLoader
{
    private readonly string _sourceName;
    private readonly string _dynamicSchema;
    private readonly IReadOnlyDictionary<string, byte[]> _payloads;

    internal Loader(
        string sourceName,
        string dynamicSchema,
        IReadOnlyDictionary<string, byte[]> payloads)
    {
        _sourceName = sourceName;
        _dynamicSchema = dynamicSchema;
        _payloads = payloads;
    }

    internal List<string> Calls { get; } = new();

    FerruleInstance IFerruleDynamicSourceLoader.Load(string sourceName, string logicalPath) =>
        FerruleJson.Parse(_dynamicSchema, Encoding.UTF8.GetString(Read(sourceName, logicalPath)));

    byte[] IFerruleDynamicJsonSourceLoader.Load(string sourceName, string logicalPath) =>
        Read(sourceName, logicalPath);

    private byte[] Read(string sourceName, string logicalPath)
    {
        if (sourceName != _sourceName)
        {
            throw new InvalidOperationException("Unexpected dynamic source.");
        }
        Calls.Add(logicalPath);
        return _payloads.TryGetValue(logicalPath, out var payload)
            ? payload
            : throw new InvalidOperationException("Dynamic path is outside the two-file corpus.");
    }
}
"#;

#[derive(Debug, PartialEq)]
struct CorpusDocumentOutput {
    path: String,
    xml: String,
    value: serde_json::Value,
}

fn run_file_set_case(
    case_dir: &Path,
    rust_target: &Path,
    mapping_path: &Path,
    project: &Project,
    source: &Instance,
    sample: &str,
) -> TestResult<()> {
    let dynamic_output = sample == "MultipleInputToMultipleOutputFiles.mfd";
    assert!(project.extra_targets.is_empty(), "{sample}: one XML target");
    assert_eq!(
        matches!(
            project.root.iteration,
            ScopeIteration::DynamicDocuments { .. }
        ),
        dynamic_output,
        "{sample}: expected target construction mode"
    );
    if dynamic_output {
        assert!(
            project.target_path.is_none(),
            "{sample}: dynamic output paths"
        );
        assert!(
            project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::SourceDocumentPath)),
            "{sample}: output paths must depend on the current source document"
        );
    }
    let Instance::DocumentSet(members) = source else {
        panic!("{sample}: local XML file set must retain its document boundaries");
    };
    assert_eq!(members.len(), 2, "{sample}: two confined source documents");

    let source_schema = case_dir.join("source-schema.json");
    let target_schema = case_dir.join("target-schema.json");
    std::fs::write(&source_schema, serde_json::to_vec(&project.source)?)?;
    std::fs::write(&target_schema, serde_json::to_vec(&project.target)?)?;
    let sample_root = mapping_path.parent().expect("sample has a directory");
    let mut input_args = Vec::new();
    let mut round_tripped = Vec::new();
    for (index, member) in members.iter().enumerate() {
        let portable = Path::new(member.path());
        assert!(
            portable
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
            "{sample}: source member paths are portable"
        );
        let resolved = Path::new(member.source_path());
        assert!(
            resolved.is_absolute() && resolved.starts_with(sample_root),
            "{sample}: source member locations stay within the sample root"
        );
        assert_ne!(
            member.path(),
            member.source_path(),
            "{sample}: portable and resolved source paths stay distinct"
        );
        let json = format_json::to_string(&project.source, member.value())?;
        let parsed = format_json::from_str(&json, &project.source)?;
        round_tripped.push(
            ir::DocumentMember::new_source(member.path(), member.source_path(), parsed)
                .expect("confined member has valid paths"),
        );
        let input_path = case_dir.join(format!("source-member-{index}.json"));
        std::fs::write(&input_path, json)?;
        input_args.push((
            member.path().to_owned(),
            member.source_path().to_owned(),
            input_path,
        ));
    }

    let execution = engine::ExecutionContext::new(mapping_path);
    let expected = engine::run_with_context(project, source, &execution)?;
    assert_eq!(
        expected,
        engine::run_with_context(project, &Instance::DocumentSet(round_tripped), &execution)?,
        "{sample}: schema-shaped JSON transport changed file-set mapping output"
    );
    let xml_options = format_xml::XmlWriteOptions {
        declaration: false,
        indent: false,
        default_namespace: None,
    };
    let expected_outputs = if dynamic_output {
        let Instance::DocumentSet(expected_members) = &expected else {
            panic!("{sample}: dynamic target must return a document set");
        };
        assert_eq!(expected_members.len(), 2, "{sample}: two intended outputs");
        assert_eq!(
            expected_members
                .iter()
                .map(|member| member.path())
                .collect::<Vec<_>>(),
            ["Persons-Nanonull-Branch.xml", "Persons-Nanonull-HQ.xml"],
            "{sample}: portable dynamic paths retain source order"
        );
        expected_members
            .iter()
            .map(|member| -> TestResult<CorpusDocumentOutput> {
                Ok(CorpusDocumentOutput {
                    path: member.path().to_owned(),
                    xml: format_xml::to_string_with_options(
                        &project.target,
                        member.value(),
                        &xml_options,
                    )?,
                    value: serde_json::from_str(&format_json::to_string(
                        &project.target,
                        member.value(),
                    )?)?,
                })
            })
            .collect::<TestResult<Vec<_>>>()?
    } else {
        assert!(
            !matches!(expected, Instance::DocumentSet(_)),
            "{sample}: merged target must be one document"
        );
        let merged = CorpusDocumentOutput {
            path: String::new(),
            xml: format_xml::to_string_with_options(&project.target, &expected, &xml_options)?,
            value: serde_json::from_str(&format_json::to_string(&project.target, &expected)?)?,
        };
        assert_eq!(
            merged.value["Office"]
                .as_array()
                .expect("merged offices")
                .len(),
            2,
            "{sample}: both source documents contribute one office"
        );
        vec![merged]
    };

    let project_path = case_dir.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(project)?)?;
    let rust_output = case_dir.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(rust_output.join("src/main.rs"), FILE_SET_RUST_HARNESS)?;
    let rust_build = Command::new("cargo")
        .args(["build", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target)
        .isolated_output()?;
    assert!(
        rust_build.status.success(),
        "{sample}: generated Rust compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_build.stdout),
        String::from_utf8_lossy(&rust_build.stderr)
    );
    let mut rust_run_command = Command::new("cargo");
    rust_run_command
        .args(["run", "--quiet", "--"])
        .arg(&source_schema)
        .arg(&target_schema)
        .arg(mapping_path)
        .arg(if dynamic_output {
            "documents"
        } else {
            "single"
        })
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", rust_target);
    for (portable, resolved, json) in &input_args {
        rust_run_command.arg(portable).arg(resolved).arg(json);
    }
    let rust_run = rust_run_command.isolated_output()?;
    assert!(
        rust_run.status.success(),
        "{sample}: generated Rust execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust_run.stdout),
        String::from_utf8_lossy(&rust_run.stderr)
    );
    assert_eq!(
        parse_file_set_outputs(&rust_run.stdout)?,
        expected_outputs,
        "{sample}: generated Rust document paths or contents differ from engine"
    );

    let csharp_output = case_dir.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    let harness = csharp_output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(harness.join("Program.cs"), FILE_SET_CSHARP_HARNESS)?;
    let csharp_build = dotnet_command(&csharp_output)
        .args([
            "build",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp_build.status.success(),
        "{sample}: generated C# compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_build.stdout),
        String::from_utf8_lossy(&csharp_build.stderr)
    );
    let mut csharp_run_command = dotnet_command(&csharp_output);
    csharp_run_command
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
        .arg(&source_schema)
        .arg(&target_schema)
        .arg(mapping_path)
        .arg(if dynamic_output {
            "documents"
        } else {
            "single"
        })
        .current_dir(&csharp_output);
    for (portable, resolved, json) in &input_args {
        csharp_run_command.arg(portable).arg(resolved).arg(json);
    }
    let csharp_run = csharp_run_command.isolated_output()?;
    assert!(
        csharp_run.status.success(),
        "{sample}: generated C# execution failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp_run.stdout),
        String::from_utf8_lossy(&csharp_run.stderr)
    );
    assert_eq!(
        parse_file_set_outputs(&csharp_run.stdout)?,
        expected_outputs,
        "{sample}: generated C# document paths or contents differ from engine"
    );
    println!("{sample}: generated Rust and C# match the interpreter");
    Ok(())
}

fn parse_file_set_outputs(bytes: &[u8]) -> TestResult<Vec<CorpusDocumentOutput>> {
    let text = std::str::from_utf8(bytes)?;
    let mut fields = text.split('\0').collect::<Vec<_>>();
    assert_eq!(fields.pop(), Some(""), "missing document terminator");
    let (records, remainder) = fields.as_chunks::<3>();
    assert!(remainder.is_empty(), "incomplete document output");
    records
        .iter()
        .map(|fields| {
            Ok(CorpusDocumentOutput {
                path: fields[0].to_owned(),
                xml: fields[1].to_owned(),
                value: serde_json::from_str(fields[2])?,
            })
        })
        .collect()
}

const FILE_SET_RUST_HARNESS: &str = r#"use std::path::PathBuf;
use codegen_runtime::{DocumentMember, ExecutionContext, Instance, Value, parse_json, serialize_json, serialize_xml};
use ferrule_generated_mapping::execute_with_context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source_schema = std::fs::read_to_string(args.next().expect("source schema"))?;
    let target_schema = std::fs::read_to_string(args.next().expect("target schema"))?;
    let mapping_path = PathBuf::from(args.next().expect("mapping path"));
    let output_kind = args.next().expect("output kind");
    let inputs = args.collect::<Vec<_>>();
    assert_eq!(inputs.len() % 3, 0, "source member arguments are triples");
    let mut members = Vec::new();
    for input in inputs.chunks_exact(3) {
        let portable = input[0].to_str().expect("portable UTF-8 member path");
        let resolved = input[1].to_str().expect("resolved UTF-8 source path");
        let json = std::fs::read_to_string(&input[2])?;
        let value = parse_json(&source_schema, &json)?;
        members.push(DocumentMember::new_source(portable, resolved, value).expect("valid member paths"));
    }
    let source = Instance::DocumentSet(members);
    let execution = ExecutionContext::new(&mapping_path);
    let output = execute_with_context(&source, &execution)?;
    match output_kind.to_str().expect("UTF-8 output kind") {
        "documents" => {
            let Instance::DocumentSet(documents) = output else {
                panic!("expected dynamic document set output");
            };
            for document in documents {
                print_output(document.path(), document.value(), &target_schema)?;
            }
        }
        "single" => {
            assert!(!matches!(output, Instance::DocumentSet(_)), "expected one merged document");
            print_output("", &output, &target_schema)?;
        }
        _ => panic!("unsupported output kind"),
    }
    Ok(())
}

fn print_output(path: &str, value: &Instance, schema: &str) -> Result<(), Box<dyn std::error::Error>> {
    let Value::String(xml) = serialize_xml(0, schema, value, false, false, None)? else {
        unreachable!("XML serialization returns a string");
    };
    let json = serialize_json(schema, value)?;
    print!("{path}\0{xml}\0{json}\0");
    Ok(())
}
"#;

const FILE_SET_CSHARP_HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var sourceSchema = File.ReadAllText(args[0]);
var targetSchema = File.ReadAllText(args[1]);
var mappingPath = args[2];
var outputKind = args[3];
if ((args.Length - 4) % 3 != 0)
{
    throw new ArgumentException("Source member arguments are triples.");
}
var members = new List<FerruleDocument>();
for (var index = 4; index < args.Length; index += 3)
{
    members.Add(new FerruleDocument(
        args[index],
        FerruleJson.Parse(sourceSchema, File.ReadAllText(args[index + 2])),
        args[index + 1]));
}
var output = GeneratedMapping.Execute(
    new FerruleDocumentSet(members),
    new FerruleExecutionContext(mappingPath));
if (outputKind == "documents")
{
    if (output is not FerruleDocumentSet documents)
    {
        throw new InvalidOperationException("Expected dynamic document set output.");
    }
    foreach (var document in documents.Documents)
    {
        WriteOutput(document.Path, document.Value);
    }
}
else if (outputKind == "single")
{
    if (output is FerruleDocumentSet)
    {
        throw new InvalidOperationException("Expected one merged document.");
    }
    WriteOutput("", output);
}
else
{
    throw new ArgumentException("Unsupported output kind.");
}

void WriteOutput(string path, FerruleInstance value)
{
    var xml = FerruleXml.Serialize(0, targetSchema, value, false, false, null).StringValue;
    var json = FerruleJson.Serialize(targetSchema, value);
    Console.Out.Write(path);
    Console.Out.Write('\0');
    Console.Out.Write(xml);
    Console.Out.Write('\0');
    Console.Out.Write(json);
    Console.Out.Write('\0');
}
"#;

fn corpus_string_field<'a>(instance: &'a Instance, name: &str) -> &'a str {
    let Instance::Group(fields) = instance else {
        panic!("mapped item is not a group");
    };
    let Some((_, Instance::Scalar(Value::String(value)))) =
        fields.iter().find(|(field, _)| field == name)
    else {
        panic!("mapped {name} is not a string");
    };
    value
}
