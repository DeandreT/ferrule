use std::path::Path;

use cli::{PayloadDocument, PayloadRunOptions, TargetSelection};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, NamedTarget, Node};

#[path = "support/bounded_documents.rs"]
mod fixtures;
use fixtures::*;

#[test]
fn bounded_csv_and_fixed_input_errors_keep_row_and_field_and_retry_cleanly() -> anyhow::Result<()> {
    for options in [csv(','), fixed(1_048_577)] {
        let project = project(options.clone(), json(), true);
        let too_long = "x".repeat(1_048_577);
        let error = run(&project, too_long.as_bytes(), &[], None).unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<format_csv::CsvReadError>(),
                Some(format_csv::CsvReadError::Limit {
                    kind: format_csv::CsvReadLimit::CellBytes,
                    ..
                })
            ) || matches!(
                error.downcast_ref::<format_csv::FixedWidthReadError>(),
                Some(format_csv::FixedWidthReadError::Limit {
                    kind: format_csv::FixedWidthReadLimit::CellBytes,
                    ..
                })
            ),
            "{error:#}"
        );
    }
    let mut project = project(csv(','), csv(','), true);
    project.source = SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    project.target = project.source.clone();
    for bad in ["bad\n1\n2\n", "1\nbad\n2\n", "1\n2\nbad\n"] {
        let row = bad.lines().position(|line| line == "bad").unwrap();
        let error = run(&project, bad.as_bytes(), &[], None).unwrap_err();
        assert!(
            matches!(error.downcast_ref::<format_csv::CsvReadError>(), Some(format_csv::CsvReadError::Format(format_csv::CsvFormatError::Parse { row: actual, field, .. })) if *actual == row && field == "Value")
        );
        assert_eq!(
            run(&project, b"1\n2\n3\n", &[], None)?.artifacts[0].bytes,
            b"1\n2\n3\n"
        );
    }
    Ok(())
}

#[test]
fn xlsx_input_preflight_refuses_zip_comments_formulas_and_bytes_before_legacy_decode()
-> anyhow::Result<()> {
    let project = project(xlsx(), csv(','), true);
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .set_name("Data")?
        .write_string(2, 2, "valid")?;
    let valid = workbook.save_to_buffer()?;
    assert_eq!(
        run(&project, &valid, &[], None)?.artifacts[0].bytes,
        b"valid\n"
    );
    let mut commented = valid.clone();
    let len = commented.len();
    assert_eq!(&commented[len - 22..len - 18], b"PK\x05\x06");
    commented[len - 2..].copy_from_slice(&1_u16.to_le_bytes());
    commented.push(b'x');
    let error = run(&project, &commented, &[], None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<format_xlsx::XlsxFormatError>(),
            Some(
                format_xlsx::XlsxFormatError::UnsupportedBoundedProfile(_)
                    | format_xlsx::XlsxFormatError::InvalidBoundedPackage(_)
            )
        ),
        "{error:#}"
    );
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .set_name("Data")?
        .write_formula(2, 2, "=1+1")?;
    let error = run(&project, &workbook.save_to_buffer()?, &[], None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<format_xlsx::XlsxFormatError>(),
            Some(format_xlsx::XlsxFormatError::UnsupportedBoundedProfile(_))
        ),
        "{error:#}"
    );
    let too_large = vec![0; 16 * 1024 * 1024 + 1];
    let error = run(&project, &too_large, &[], None).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<format_xlsx::XlsxFormatError>(),
        Some(format_xlsx::XlsxFormatError::LimitExceeded {
            limit: format_xlsx::XlsxLimit::InputBytes,
            ..
        })
    ));
    assert_eq!(
        run(&project, &valid, &[], None)?.artifacts[0].bytes,
        b"valid\n"
    );
    Ok(())
}

#[test]
fn xlsx_output_preflight_refusal_is_typed_and_has_no_partial_result() -> anyhow::Result<()> {
    let project = project(csv(','), xlsx(), true);
    let oversized = "x".repeat(128 * 1024 + 1);
    let error = run(&project, oversized.as_bytes(), &[], None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<format_xlsx::XlsxFormatError>(),
            Some(format_xlsx::XlsxFormatError::LimitExceeded {
                limit: format_xlsx::XlsxLimit::StringBytes,
                ..
            })
        ),
        "{error:#}"
    );
    assert_eq!(run(&project, b"small\n", &[], None)?.artifacts.len(), 1);
    Ok(())
}

#[test]
fn protobuf_input_uses_existing_wire_budget_and_empty_input_is_present() -> anyhow::Result<()> {
    let project = project(protobuf(), protobuf(), false);
    assert!(run(&project, b"", &[], None)?.artifacts[0].bytes.is_empty());
    let too_many = [0x10, 0].repeat(1_000_001);
    let error = run(&project, &too_many, &[], None).unwrap_err();
    assert!(
        error
            .downcast_ref::<format_protobuf::ProtobufError>()
            .is_some()
    );
    assert!(format!("{error:#}").contains("wire field count"));
    assert_eq!(
        run(&project, b"\x0a\x02ok", &[], None)?.artifacts[0].bytes,
        b"\x0a\x02ok"
    );
    Ok(())
}

#[test]
fn protobuf_encode_cap_is_real_and_later_format_error_wins_before_retry() -> anyhow::Result<()> {
    let mut project = project(
        json(),
        FormatOptions {
            protobuf: Some(mapping::ProtobufOptions {
                schema: "syntax = \"proto2\"; message Packet { optional string v = 1; }".into(),
                root_message: "Packet".into(),
                schema_path: None,
                imports: vec![],
            }),
            ..Default::default()
        },
        false,
    );
    project.target = SchemaNode::group("Packet", vec![SchemaNode::scalar("v", ScalarType::String)]);
    project.root.bindings = vec![Binding {
        target_field: "v".into(),
        node: 0,
    }];
    // Input budget counts one name byte plus this string. The encoded tag and
    // length add five bytes, crossing only the 64 MiB output limit.
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("x".repeat(cli::MAX_PAYLOAD_DOCUMENT_BYTES - 2)),
        },
    );
    let error = run(&project, b"{}", &[], None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<format_protobuf::BoundedEncodeError>(),
            Some(format_protobuf::BoundedEncodeError::LimitExceeded {
                limit: format_protobuf::EncodeLimit::OutputBytes,
                maximum: cli::MAX_PAYLOAD_DOCUMENT_BYTES
            })
        ),
        "{error:#}"
    );
    assert!(format!("{error:#}").contains("host-result.sqlite"));
    assert!(!format!("{error:#}").contains("document.pb"));
    project.extra_targets.push(NamedTarget {
        name: "bad".into(),
        path: Some("original.bad".into()),
        schema: schema(),
        options: FormatOptions::default(),
        root: scope(None, 1),
    });
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("ok".into()),
        },
    );
    let error = run(&project, b"{}", &[], None).unwrap_err();
    assert!(
        format!("{error:#}").contains("explicit saved format options"),
        "{error:#}"
    );
    assert!(
        error
            .downcast_ref::<format_protobuf::BoundedEncodeError>()
            .is_none()
    );
    // The invalid unselected target doesn't participate in a selected retry.
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("ok".into()),
        },
    );
    assert_eq!(
        run(&project, b"{}", &[], Some(TargetSelection::Primary))?.artifacts[0].bytes,
        b"\x0a\x02ok"
    );
    Ok(())
}

#[test]
fn bounded_xbrl_input_and_output_limits_are_retained() -> anyhow::Result<()> {
    let (schema, options, input) = xbrl();
    let mut project = copy_project(schema, options);
    let too_large = vec![b' '; 16 * 1024 * 1024 + 1];
    let error = run(&project, &too_large, &[], None).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<format_xbrl::XbrlFormatError>(),
        Some(format_xbrl::XbrlFormatError::LimitExceeded {
            limit: format_xbrl::XbrlLimit::InputBytes,
            ..
        })
    ));
    let value = format_xbrl::from_bytes_bounded(
        input.as_bytes(),
        &project.source,
        project.source_options.xbrl.as_ref().unwrap(),
        &Default::default(),
    )?;
    project.source_options = json();
    let large = input.replace("zero", &"x".repeat(1024 * 1024 + 1));
    let value_large = format_xbrl::from_str_with_options(
        &large,
        &project.source,
        project.target_options.xbrl.as_ref().unwrap(),
    )?;
    let json_large = format_json::to_string(&project.source, &value_large)?;
    let error = run(&project, json_large.as_bytes(), &[], None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<format_xbrl::XbrlFormatError>(),
            Some(format_xbrl::XbrlFormatError::LimitExceeded {
                limit: format_xbrl::XbrlLimit::StringBytes,
                ..
            })
        ),
        "{error:#}"
    );
    let json_value = format_json::to_string(&project.source, &value)?;
    assert_eq!(
        run(&project, json_value.as_bytes(), &[], None)?
            .artifacts
            .len(),
        1
    );
    Ok(())
}

#[test]
fn flextext_native_input_budget_is_not_replaced_by_fixed_width() -> anyhow::Result<()> {
    let layout = mapping::FlexTextLayout::new(
        "document",
        mapping::FlexCommand::DelimitedRecords {
            name: "rows".into(),
            dialect: mapping::DelimitedDialect::new(';', "\n", '"', '\\')?,
            fields: vec![mapping::DelimitedRecordField::new(
                "Value",
                ScalarType::String,
            )?],
        },
        mapping::FlexLineEnding::Lf,
        false,
    )?;
    let project = copy_project(
        layout.schema(),
        FormatOptions {
            flextext: Some(layout),
            ..Default::default()
        },
    );
    let too_large = vec![b'x'; format_flextext::MAX_INPUT_BYTES + 1];
    let error = run(&project, &too_large, &[], None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<format_flextext::FlexTextError>(),
            Some(format_flextext::FlexTextError::InputTooLarge)
        ),
        "{error:#}"
    );
    assert_eq!(run(&project, b"ok", &[], None)?.artifacts[0].bytes, b"ok");
    Ok(())
}

#[test]
fn default_and_bounded_tabular_profiles_keep_legacy_filename_dispatch() -> anyhow::Result<()> {
    let mut project = project(FormatOptions::default(), FormatOptions::default(), false);
    project.source_path = Some("saved.json".into());
    project.target_path = Some("saved.json".into());
    let input = PayloadDocument::new(Path::new("upload.json"), b"{}")?;
    for options in [
        PayloadRunOptions::new(input),
        PayloadRunOptions::new(input).with_bounded_tabular(),
        PayloadRunOptions::new(input)
            .with_bounded_documents()
            .with_bounded_tabular(),
    ] {
        assert!(
            cli::run_project_value_payloads(&project, Path::new("project.json"), &options).is_ok()
        );
    }
    assert!(
        cli::run_project_value_payloads(
            &project,
            Path::new("project.json"),
            &PayloadRunOptions::new(input).with_bounded_documents()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn pure_document_profile_keeps_outer_document_run_and_input_count_ceilings() -> anyhow::Result<()> {
    let bytes = vec![0; cli::MAX_PAYLOAD_DOCUMENT_BYTES + 1];
    assert!(PayloadDocument::new(Path::new("too-large.json"), &bytes).is_err());
    let large = PayloadDocument::new(
        Path::new("large.json"),
        &bytes[..cli::MAX_PAYLOAD_DOCUMENT_BYTES],
    )?;
    let extra = cli::NamedPayloadInput::new("large", large)?;
    let extras = [extra; 4];
    let primary = PayloadDocument::new(Path::new("primary.json"), b"{}")?;
    let project = project(json(), json(), false);
    let error = cli::run_project_value_payloads(
        &project,
        Path::new("project.json"),
        &PayloadRunOptions::new(primary)
            .with_extra_sources(&extras)
            .with_bounded_documents(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("256 MiB total limit"),
        "{error:#}"
    );
    let empty =
        cli::NamedPayloadInput::new("empty", PayloadDocument::new(Path::new("empty.json"), b"")?)?;
    let extras = vec![empty; 4097];
    let error = cli::run_project_value_payloads(
        &project,
        Path::new("project.json"),
        &PayloadRunOptions::new(primary)
            .with_extra_sources(&extras)
            .with_bounded_documents(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("4096 additional input documents"),
        "{error:#}"
    );
    Ok(())
}
