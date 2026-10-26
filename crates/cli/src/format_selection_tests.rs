use super::*;
use ir::{ScalarType, Value};
use mapping::{ExternalHttpMode, HttpTimeoutSeconds};

const FAMILIES: [&str; 11] = [
    "xbrl",
    "idoc",
    "swift_mt",
    "external",
    "pdf",
    "flextext",
    "protobuf",
    "edi_kind",
    "xml_document",
    "json_document",
    "fixed_width",
];

fn add_family(options: &mut FormatOptions, name: &str) {
    match name {
        "xbrl" => {
            options.xbrl = Some(mapping::XbrlBoundaryOptions::external_source("test.xsd").unwrap())
        }
        "idoc" => {
            options.idoc = Some(
                mapping::IdocLayout::new(vec![
                    mapping::IdocSegmentLayout::new(
                        "HEADER",
                        vec![
                            mapping::IdocFieldLayout::new(
                                "value",
                                8.try_into().unwrap(),
                                10.try_into().unwrap(),
                            )
                            .unwrap(),
                        ],
                    )
                    .unwrap(),
                ])
                .unwrap(),
            )
        }
        "swift_mt" => {
            options.swift_mt = Some(
                mapping::SwiftMtLayout::new(vec![mapping::SwiftMessageLayout::new(
                    "MT950",
                    vec![mapping::SwiftFieldLayout::new(
                        "20",
                        vec!["MT950".into(), "20".into()],
                        false,
                        mapping::SwiftValueExpr::Capture {
                            path: vec![],
                            min: 1,
                            max: 16,
                            charset: mapping::SwiftCharset::Text,
                        },
                    )],
                )])
                .unwrap(),
            )
        }
        "external" => {
            options.external_source = Some(
                mapping::ExternalSourceOptions::http_post(
                    ExternalHttpMode::Manual,
                    HttpTimeoutSeconds::default(),
                    None,
                    None,
                    ExternalPayloadFormat::Json,
                    vec![],
                )
                .unwrap(),
            )
        }
        "pdf" => {
            options.pdf = Some(
                mapping::PdfLayout::new(
                    "Document",
                    mapping::PdfPageSelection::First,
                    vec![mapping::PdfCommand::Capture(mapping::PdfCapture {
                        name: "value".into(),
                        region: mapping::PdfRegion::full(),
                        algorithm: Default::default(),
                    })],
                )
                .unwrap(),
            )
        }
        "flextext" => {
            options.flextext = Some(
                mapping::FlexTextLayout::new(
                    "Document",
                    mapping::FlexCommand::DelimitedRecords {
                        name: "rows".into(),
                        dialect: mapping::DelimitedDialect::new(';', "\n", '"', '\\').unwrap(),
                        fields: vec![
                            mapping::DelimitedRecordField::new("value", ScalarType::String)
                                .unwrap(),
                        ],
                    },
                    mapping::FlexLineEnding::Lf,
                    false,
                )
                .unwrap(),
            )
        }
        // Selection must not parse a schema, even when it is malformed.
        "protobuf" => {
            options.protobuf = Some(ProtobufOptions {
                schema: "not a protobuf schema".into(),
                root_message: "Message".into(),
                schema_path: None,
                imports: vec![],
            })
        }
        "edi_kind" => options.edi_kind = Some(EdiBoundaryKind::X12),
        "xml_document" => options.xml_document = true,
        "json_document" => options.json_document = true,
        "fixed_width" => {
            options.fixed_width = Some(
                mapping::FixedWidthLayout::new(
                    vec![mapping::FixedFieldWidth::new(8).unwrap()],
                    ' ',
                    true,
                    true,
                )
                .unwrap(),
            )
        }
        _ => panic!("unknown test family"),
    }
}

fn family(selected: SelectedFormat<'_>) -> &'static str {
    match selected {
        SelectedFormat::Xbrl(_) => "xbrl",
        SelectedFormat::Idoc(_) => "idoc",
        SelectedFormat::Swift(_) => "swift_mt",
        SelectedFormat::External(_) => "external",
        SelectedFormat::Pdf(_) => "pdf",
        SelectedFormat::FlexText(_) => "flextext",
        SelectedFormat::Protobuf(_) => "protobuf",
        SelectedFormat::Edi(Some(_)) => "edi_kind",
        SelectedFormat::Edi(None) => "inferred_edi",
        SelectedFormat::Xml(_) => "xml_document",
        SelectedFormat::Json(_, _) => "json_document",
        SelectedFormat::HttpXml(_) => "http",
        SelectedFormat::FixedWidth(_) => "fixed_width",
        SelectedFormat::Csv => "csv",
        SelectedFormat::Xlsx => "xlsx",
        SelectedFormat::Sqlite => "sqlite",
        SelectedFormat::MissingIdoc => "missing_idoc",
        SelectedFormat::MissingSwift => "missing_swift",
        SelectedFormat::MissingPdf => "missing_pdf",
        SelectedFormat::Unsupported(_) => "unsupported",
    }
}

#[test]
fn explicit_families_override_paths_without_parsing_layouts() {
    for name in FAMILIES {
        let mut options = FormatOptions::default();
        add_family(&mut options, name);
        for path in ["capture", "capture.csv", "capture.jsonl", "capture.unknown"] {
            let selected =
                format_selection::select(Path::new(path), &options, Direction::Input, false)
                    .unwrap();
            assert_eq!(family(selected), name, "{name}: {path}");
        }
    }
}

#[test]
fn retained_json5_dependency_cannot_be_overridden_by_another_format() {
    for name in FAMILIES {
        let mut options = FormatOptions {
            json5_interchange_dependency: Some(
                mapping::Json5InterchangeDependency::ComponentSetting { value: "1".into() },
            ),
            ..FormatOptions::default()
        };
        add_family(&mut options, name);
        for direction in [Direction::Input, Direction::Output] {
            let error = match format_selection::select(
                Path::new("override.json"),
                &options,
                direction,
                false,
            ) {
                Err(error) => error,
                Ok(_) => panic!("retained JSON5 dependency selected {name}"),
            };
            assert!(
                error
                    .downcast_ref::<mapping::Json5InterchangeDependency>()
                    .is_some(),
                "{name}: {error:#}"
            );
        }
    }
}

#[test]
fn every_explicit_pair_keeps_the_first_conflict_owner_across_routes() {
    let schema = SchemaNode::group("Root", vec![]);
    let document =
        payload::PayloadDocument::new(Path::new("input.capture"), b"not an instance").unwrap();
    for (index, first) in FAMILIES.iter().enumerate() {
        for second in &FAMILIES[index + 1..] {
            let mut options = FormatOptions::default();
            add_family(&mut options, first);
            add_family(&mut options, second);
            if (*first, *second) == ("external", "json_document") {
                assert_eq!(
                    family(
                        format_selection::select(
                            document.path(),
                            &options,
                            Direction::Input,
                            false
                        )
                        .unwrap()
                    ),
                    "external"
                );
                continue;
            }
            let selected =
                format_selection::select(document.path(), &options, Direction::Input, false);
            let message = selected.err().unwrap().to_string();
            let owner = if *first == "external" {
                "captured external source"
            } else {
                first
            };
            assert!(message.contains(owner), "{first} + {second}: {message}");
            assert!(message.ends_with("for input"), "{message}");
            let payload_error = payload::read_payload(document, &schema, &options).unwrap_err();
            assert_eq!(payload_error.to_string(), message);
            let file_error = read_instance_with_pdf_reader(
                document.path(),
                &schema,
                &options,
                None,
                &ExecutionPolicy::Trusted,
            )
            .unwrap_err();
            assert_eq!(file_error.to_string(), message);
        }
    }
}

#[test]
fn fallback_table_keeps_recognized_extensions_and_tabular_overrides() {
    for (path, tabular, expected) in [
        ("a.CSV", None, "csv"),
        ("a.txt", None, "csv"),
        ("a.XLSX", None, "xlsx"),
        ("a.sqlite3", None, "sqlite"),
        ("a.hl7", None, "inferred_edi"),
        ("a.idoc", None, "missing_idoc"),
        ("a.fin", None, "missing_swift"),
        ("a.pdf", None, "missing_pdf"),
        ("a.xbrl", Some(TabularBoundaryKind::Csv), "unsupported"),
        ("a.capture", Some(TabularBoundaryKind::Csv), "csv"),
        ("capture", Some(TabularBoundaryKind::Xlsx), "xlsx"),
        ("a.json", Some(TabularBoundaryKind::Csv), "json_document"),
    ] {
        let options = FormatOptions {
            tabular_kind: tabular,
            ..Default::default()
        };
        for direction in [Direction::Input, Direction::Output] {
            assert_eq!(
                family(
                    format_selection::select(Path::new(path), &options, direction, false).unwrap()
                ),
                expected,
                "{path}"
            );
        }
    }
    assert_eq!(
        format_selection::select(
            Path::new("capture"),
            &FormatOptions::default(),
            Direction::Input,
            false
        )
        .err()
        .unwrap()
        .to_string(),
        "capture has no file extension"
    );
}

#[test]
fn json_origin_preserves_explicit_document_and_fallback_lines() {
    for (path, document, json5, lines, expected, origin) in [
        (
            "a.jsonl",
            true,
            false,
            false,
            JsonMode::Document,
            Origin::Explicit,
        ),
        (
            "a.ndjson",
            false,
            false,
            false,
            JsonMode::Lines,
            Origin::Fallback,
        ),
        (
            "a.JSON5",
            true,
            false,
            false,
            JsonMode::Json5,
            Origin::Explicit,
        ),
        (
            "a.csv",
            false,
            false,
            true,
            JsonMode::Lines,
            Origin::Explicit,
        ),
        (
            "a.jsonl",
            false,
            true,
            false,
            JsonMode::Json5,
            Origin::Explicit,
        ),
    ] {
        let options = FormatOptions {
            json_document: document,
            json5,
            json_lines: lines,
            ..Default::default()
        };
        assert!(
            matches!(format_selection::select(Path::new(path), &options, Direction::Input, false).unwrap(),
            SelectedFormat::Json(actual, source) if actual == expected && source == origin)
        );
    }
    let options = FormatOptions {
        json_lines: true,
        ..Default::default()
    };
    assert_eq!(
        format_selection::select(Path::new("a.json5"), &options, Direction::Input, false)
            .err()
            .unwrap()
            .to_string(),
        "JSON5 cannot be combined with JSON Lines"
    );
}

#[test]
fn filesystem_http_fallback_precedes_fixed_width_but_payload_does_not() {
    let mut options = FormatOptions::default();
    add_family(&mut options, "fixed_width");
    options.delimiter = Some(',');
    let path = Path::new("https://example.invalid/capture");
    assert_eq!(
        family(format_selection::select(path, &options, Direction::Input, true).unwrap()),
        "http"
    );
    assert!(
        format_selection::select(path, &options, Direction::Input, false)
            .err()
            .unwrap()
            .to_string()
            .starts_with("`fixed_width`")
    );
    let mut options = FormatOptions::default();
    add_family(&mut options, "json_document");
    assert_eq!(
        family(format_selection::select(path, &options, Direction::Input, true).unwrap()),
        "json_document"
    );
    let mut options = FormatOptions::default();
    add_family(&mut options, "external");
    assert_eq!(
        family(
            format_selection::select(Path::new("a.csv"), &options, Direction::Input, true).unwrap()
        ),
        "external"
    );
    assert_eq!(
        family(
            format_selection::select(Path::new("a.csv"), &options, Direction::Output, false)
                .unwrap()
        ),
        "csv"
    );
}

#[test]
fn preview_keeps_its_captured_json_flag_preflight() {
    let mut options = FormatOptions::default();
    add_family(&mut options, "external");
    options.json_lines = true;
    let path = Path::new("captured.json5");
    assert_eq!(
        family(format_selection::select(path, &options, Direction::Input, false).unwrap()),
        "external"
    );
    assert_eq!(
        format_selection::select_preview(path, &options, Direction::Input)
            .err()
            .unwrap()
            .to_string(),
        "JSON5 cannot be combined with JSON Lines"
    );
}

#[test]
fn common_metadata_checks_precede_selected_conflicts() {
    let schema = SchemaNode::group("Root", vec![]);
    let options = FormatOptions {
        csv_utf8_bom: true,
        xml_document: true,
        json_document: true,
        ..Default::default()
    };
    let path = Path::new("missing.json");
    let file =
        read_instance_with_pdf_reader(path, &schema, &options, None, &ExecutionPolicy::Trusted)
            .unwrap_err();
    let payload = payload::read_payload(
        payload::PayloadDocument::new(path, b"bad").unwrap(),
        &schema,
        &options,
    )
    .unwrap_err();
    for error in [file, payload] {
        assert_eq!(
            error.to_string(),
            "`csv_utf8_bom` requires a CSV input boundary"
        );
    }
}

#[test]
fn inferred_edi_output_formats_values_before_detecting_the_dialect() -> anyhow::Result<()> {
    let directory = owned_host_test_directory("edi-selection")?;
    let path = directory.join("output.edi");
    let schema = SchemaNode::group(
        "UnknownDialect",
        vec![SchemaNode::scalar("date", ScalarType::String)],
    );
    let instance = Instance::Group(
        vec![(
            "date".into(),
            Instance::Scalar(Value::String("not-a-date".into())),
        )]
        .into(),
    );
    let options = FormatOptions {
        edi_lexical_formats: vec![
            mapping::EdiLexicalFormat::new(
                vec!["date".into()],
                mapping::EdiLexicalKind::CompactDate8,
            )
            .unwrap(),
        ],
        ..Default::default()
    };
    std::fs::write(&path, b"unchanged")?;
    let file =
        write_output(&path, &schema, &instance, &options, "2026-01-02T03:04:05Z").unwrap_err();
    let payload =
        payload::render_payload(&path, &schema, &instance, &options, "2026-01-02T03:04:05Z")
            .unwrap_err();
    for error in [file, payload] {
        assert_eq!(error.to_string(), "applying EDI output lexical formats");
    }
    assert_eq!(std::fs::read(&path)?, b"unchanged");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn recognized_fallbacks_keep_legacy_ignored_metadata() {
    let options = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        xlsx_sheet: Some("Ignored".into()),
        delimiter: Some(';'),
        lenient_segments: true,
        ..Default::default()
    };
    assert_eq!(
        family(
            format_selection::select(Path::new("a.json"), &options, Direction::Input, false)
                .unwrap()
        ),
        "json_document"
    );
    assert_eq!(
        family(
            format_selection::select(Path::new("a.csv"), &options, Direction::Output, false)
                .unwrap()
        ),
        "csv"
    );
    assert!(
        format_selection::select(Path::new("a.capture"), &options, Direction::Input, false)
            .err()
            .unwrap()
            .to_string()
            .starts_with("CSV fallback identity")
    );
}

#[test]
fn policy_and_payload_file_set_refusals_keep_their_priority() {
    let schema = SchemaNode::group("Root", vec![]);
    let options = FormatOptions {
        json_document: true,
        delimiter: Some(','),
        ..Default::default()
    };
    let policy =
        ExecutionPolicy::Restricted(RestrictedPolicy::new(ExecutionBudgets::new(100, 100, 100)));
    let error = read_instance_with_pdf_reader(
        Path::new("absent.capture"),
        &schema,
        &options,
        None,
        &policy,
    )
    .unwrap_err();
    assert!(error.downcast_ref::<PolicyRefusal>().is_some(), "{error:#}");
    let options = FormatOptions {
        local_xml_file_set: true,
        ..options
    };
    let error = payload::read_payload(
        payload::PayloadDocument::new(Path::new("a.capture"), b"bad").unwrap(),
        &schema,
        &options,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "`local_xml_file_set` requires multiple filesystem documents and is unavailable for a single payload"
    );
}

#[test]
fn conflicting_outputs_leave_existing_bytes_untouched() -> anyhow::Result<()> {
    let directory = owned_host_test_directory("format-selection")?;
    let path = directory.join("out.capture");
    let schema = SchemaNode::group("Root", vec![]);
    let instance = Instance::Group(Default::default());
    for first in FAMILIES
        .into_iter()
        .filter(|name| *name != "external" && *name != "fixed_width")
    {
        let mut options = FormatOptions::default();
        add_family(&mut options, first);
        add_family(&mut options, "fixed_width");
        std::fs::write(&path, b"unchanged")?;
        let error =
            write_output(&path, &schema, &instance, &options, "2026-01-02T03:04:05Z").unwrap_err();
        assert!(error.to_string().contains(first), "{first}: {error:#}");
        assert_eq!(std::fs::read(&path)?, b"unchanged");
        let error =
            payload::render_payload(&path, &schema, &instance, &options, "2026-01-02T03:04:05Z")
                .unwrap_err();
        assert!(error.to_string().contains(first), "{first}: {error:#}");
        assert_eq!(std::fs::read(&path)?, b"unchanged");
    }
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn explicit_jsonl_document_matches_payload_and_filesystem_bytes() -> anyhow::Result<()> {
    let directory = owned_host_test_directory("json-origin")?;
    let path = directory.join("document.jsonl");
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    )
    .repeating();
    let instance = Instance::Repeated(
        ["one", "two"]
            .map(|value| {
                Instance::Group(
                    vec![(
                        "value".into(),
                        Instance::Scalar(Value::String(value.into())),
                    )]
                    .into(),
                )
            })
            .into(),
    );
    for explicit in [false, true] {
        let options = FormatOptions {
            json_document: explicit,
            ..Default::default()
        };
        let (bytes, records) =
            payload::render_payload(&path, &schema, &instance, &options, "2026-01-02T03:04:05Z")?;
        assert_eq!(records, 2);
        assert_eq!(bytes.starts_with(b"["), explicit);
        write_output(&path, &schema, &instance, &options, "2026-01-02T03:04:05Z")?;
        assert_eq!(std::fs::read(&path)?, bytes);
        assert_eq!(
            payload::read_payload(
                payload::PayloadDocument::new(&path, &bytes)?,
                &schema,
                &options
            )?,
            instance
        );
        assert_eq!(
            read_instance_with_pdf_reader(
                &path,
                &schema,
                &options,
                None,
                &ExecutionPolicy::Trusted
            )?,
            instance
        );
    }
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
