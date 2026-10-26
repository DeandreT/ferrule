use std::io::{Cursor, Write};

use ir::{Instance, ScalarType, SchemaNode, Value};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::FlatTableWriteOptions;

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn schema() -> SchemaNode {
    SchemaNode::group(
        "rows",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    )
}

fn row(value: Value) -> Instance {
    Instance::Group(vec![("value".to_string(), Instance::Scalar(value))].into())
}

fn options(update: impl FnOnce(&mut XlsxLimitOptions)) -> XlsxLimits {
    let mut limits = XlsxLimitOptions::default();
    update(&mut limits);
    XlsxLimits::new(limits).unwrap()
}

fn sheet(inner: &str) -> String {
    format!(r#"<worksheet xmlns="{NS}"><sheetData>{inner}</sheetData></worksheet>"#)
}

fn parts(worksheet: String, strings: Option<&str>) -> Vec<(String, String)> {
    let mut entries = vec![
        ("[Content_Types].xml".into(), r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.into()),
        ("_rels/.rels".into(), r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="book" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into()),
        ("xl/workbook.xml".into(), format!(r#"<workbook xmlns="{NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Revenue" sheetId="1" r:id="sheet"/></sheets></workbook>"#)),
        ("xl/_rels/workbook.xml.rels".into(), r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="sheet" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.into()),
        ("xl/worksheets/sheet1.xml".into(), worksheet),
    ];
    if let Some(strings) = strings {
        entries.push(("xl/sharedStrings.xml".into(), strings.into()));
    }
    entries
}

fn archive(parts: &[(String, String)], compression: CompressionMethod) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in parts {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(compression),
        )
        .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn fixture(inner: &str) -> Vec<u8> {
    archive(&parts(sheet(inner), None), CompressionMethod::Deflated)
}

fn read(bytes: &[u8], limits: &XlsxLimits) -> Result<Vec<Instance>, XlsxFormatError> {
    from_bytes_bounded(bytes, &schema(), None, 1, &[], false, limits)
}

fn limit(error: XlsxFormatError, expected: XlsxLimit) {
    assert!(
        matches!(error, XlsxFormatError::LimitExceeded { limit, .. } if limit == expected),
        "{error:?}"
    );
}

#[test]
fn independently_authored_flat_fixture_matches_existing_reader() {
    let bytes = fixture(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>hello &amp; goodbye</t></is></c></row><row r="3"><c r="A3" t="inlineStr"><is><t xml:space="preserve"> é </t></is></c></row>"#,
    );
    assert_eq!(
        read(&bytes, &XlsxLimits::default()).unwrap(),
        crate::from_bytes(&bytes, &schema(), None, 1, &[], false).unwrap()
    );
    assert_eq!(
        read(&bytes, &XlsxLimits::default()).unwrap(),
        vec![
            row(Value::String("hello & goodbye".into())),
            row(Value::String(" é ".into()))
        ]
    );
}

#[test]
fn writer_reader_parity_preserves_flat_options_and_native_types() {
    let schema = SchemaNode::group(
        "rows",
        vec![
            SchemaNode::scalar("value", ScalarType::String),
            SchemaNode::scalar("count", ScalarType::Int),
            SchemaNode::scalar("price", ScalarType::Float),
            SchemaNode::scalar("active", ScalarType::Bool),
        ],
    );
    let rows = vec![Instance::Group(
        vec![
            (
                "value".into(),
                Instance::Scalar(Value::String("α <&>".into())),
            ),
            ("count".into(), Instance::Scalar(Value::Int(42))),
            ("price".into(), Instance::Scalar(Value::Float(1.25))),
            ("active".into(), Instance::Scalar(Value::Bool(true))),
        ]
        .into(),
    )];
    let headers = vec![
        "duplicate".into(),
        "duplicate".into(),
        "".into(),
        "Active".into(),
    ];
    let options = FlatTableWriteOptions {
        sheet: Some("Revenue"),
        start_row: 3,
        columns: &[2, 4, 6, 8],
        headers: &headers,
        has_header: true,
    };
    let old = crate::to_bytes_with_options(&schema, &rows, options).unwrap();
    let new =
        to_bytes_with_options_bounded(&schema, &rows, options, &XlsxLimits::default()).unwrap();
    for bytes in [old, new] {
        let bounded = from_bytes_bounded(
            &bytes,
            &schema,
            Some("Revenue"),
            3,
            &[2, 4, 6, 8],
            true,
            &XlsxLimits::default(),
        )
        .unwrap();
        assert_eq!(bounded, rows);
        assert_eq!(
            bounded,
            crate::from_bytes(&bytes, &schema, Some("Revenue"), 3, &[2, 4, 6, 8], true).unwrap()
        );
    }
}

#[test]
fn unmarked_typed_headers_and_all_string_headers_keep_existing_behavior() {
    let schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Int)]);
    let bytes = crate::to_bytes(&schema, &[row(Value::Int(8))], None, 1, &[], true).unwrap();
    assert_eq!(
        from_bytes_bounded(&bytes, &schema, None, 1, &[], false, &XlsxLimits::default()).unwrap(),
        vec![row(Value::Int(8))]
    );
    let bytes = crate::to_bytes(
        &super::tests::schema(),
        &[row(Value::String("hello".into()))],
        None,
        1,
        &[],
        true,
    )
    .unwrap();
    assert_eq!(read(&bytes, &XlsxLimits::default()).unwrap().len(), 2);
}

#[test]
fn zero_rows_null_and_empty_cells_are_supported() {
    for rows in [
        vec![],
        vec![row(Value::Null)],
        vec![row(Value::String(String::new()))],
    ] {
        for header in [false, true] {
            let bytes = to_bytes_bounded(
                &schema(),
                &rows,
                None,
                1,
                &[],
                header,
                &XlsxLimits::default(),
            )
            .unwrap();
            assert_eq!(
                read(&bytes, &XlsxLimits::default()).unwrap(),
                crate::from_bytes(&bytes, &schema(), None, 1, &[], false).unwrap()
            );
        }
    }
    let bytes = fixture(r#"<row r="1"><c r="A1"/></row>"#);
    assert!(read(&bytes, &XlsxLimits::default()).unwrap().is_empty());
}

#[test]
fn compressed_bomb_is_stopped_by_actual_entry_and_total_bytes() {
    let bytes = fixture(&format!(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>{}</t></is></c></row>"#,
        "x".repeat(128 * 1024)
    ));
    assert!(bytes.len() < 4096);
    limit(
        read(&bytes, &options(|l| l.max_entry_bytes = 4096)).unwrap_err(),
        XlsxLimit::EntryBytes,
    );
    limit(
        read(&bytes, &options(|l| l.max_expanded_bytes = 4096)).unwrap_err(),
        XlsxLimit::ExpandedBytes,
    );
    limit(
        read(&bytes, &options(|l| l.max_input_bytes = 10)).unwrap_err(),
        XlsxLimit::InputBytes,
    );
}

#[test]
fn lying_zip_expanded_length_does_not_bypass_stream_budget() {
    let mut bytes = fixture(&format!(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>{}</t></is></c></row>"#,
        "x".repeat(128 * 1024)
    ));
    // Change both matching size declarations, leaving the actual deflate stream.
    for signature in [b"PK\x03\x04", b"PK\x01\x02"] {
        let positions: Vec<_> = bytes
            .windows(4)
            .enumerate()
            .filter(|(_, b)| *b == signature)
            .map(|(i, _)| i)
            .collect();
        for p in positions {
            let (name_at, name_len_at, size_at) = if signature == b"PK\x03\x04" {
                (p + 30, p + 26, p + 22)
            } else {
                (p + 46, p + 28, p + 24)
            };
            let n = u16::from_le_bytes(bytes[name_len_at..name_len_at + 2].try_into().unwrap())
                as usize;
            if &bytes[name_at..name_at + n] == b"xl/worksheets/sheet1.xml" {
                bytes[size_at..size_at + 4].copy_from_slice(&1_u32.to_le_bytes());
            }
        }
    }
    limit(
        read(&bytes, &options(|l| l.max_entry_bytes = 4096)).unwrap_err(),
        XlsxLimit::EntryBytes,
    );
    assert!(matches!(
        read(&bytes, &XlsxLimits::default()),
        Err(XlsxFormatError::InvalidBoundedPackage(_))
    ));
}

#[test]
fn directory_entry_count_is_checked_before_zip_allocation() {
    let bytes = fixture("");
    limit(
        read(&bytes, &options(|l| l.max_entries = 4)).unwrap_err(),
        XlsxLimit::Entries,
    );
    let mut bytes = bytes;
    let end = bytes.len() - 22;
    bytes[end + 8..end + 12].copy_from_slice(&[254, 255, 254, 255]);
    limit(
        read(&bytes, &XlsxLimits::default()).unwrap_err(),
        XlsxLimit::Entries,
    );
}

#[test]
fn truncated_and_hostile_zip_offsets_fail_without_panicking() {
    let bytes = fixture("");
    for end in 0..bytes.len() {
        assert!(read(&bytes[..end], &XlsxLimits::default()).is_err());
    }
    let directory = bytes.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    for offset in [
        directory + 20,
        directory + 28,
        directory + 42,
        bytes.len() - 6,
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&bad, &XlsxLimits::default()).is_err());
    }
}

#[test]
fn many_actual_entries_and_case_aliases_reject() {
    let mut entries = parts(sheet(""), None);
    for n in 2..=40 {
        entries.push((format!("xl/worksheets/sheet{n}.xml"), sheet("")));
    }
    let bytes = archive(&entries, CompressionMethod::Stored);
    limit(
        read(&bytes, &options(|l| l.max_entries = 20)).unwrap_err(),
        XlsxLimit::Entries,
    );
    entries.push(("xl/worksheets/Sheet1.xml".into(), sheet("")));
    let bytes = archive(&entries, CompressionMethod::Stored);
    assert!(matches!(
        read(&bytes, &XlsxLimits::default()),
        Err(XlsxFormatError::InvalidBoundedPackage(_))
    ));
}

#[test]
fn text_limits_count_combined_inline_runs_and_decoded_entities() {
    let text = "&#xE9;".repeat(80);
    let bytes = fixture(&format!(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><r><t>{text}</t></r><r><t>{text}</t></r></is></c></row>"#
    ));
    limit(
        read(&bytes, &options(|l| l.max_string_bytes = 200)).unwrap_err(),
        XlsxLimit::StringBytes,
    );
    let bytes =
        fixture(r#"<row r="1"><c r="A1" t="inlineStr"><is><t><b>bad</b></t></is></c></row>"#);
    assert!(matches!(
        read(&bytes, &XlsxLimits::default()),
        Err(XlsxFormatError::UnsupportedBoundedProfile(_))
    ));
}

#[test]
fn ambiguous_relationships_content_types_and_namespaces_reject() {
    let valid = parts(sheet(""), None);
    for (index, from, to) in [
        (
            0,
            "spreadsheetml.sheet.main+xml",
            "vnd.ms-excel.sheet.macroEnabled.main+xml",
        ),
        (2, "r:id=\"sheet\"", "r:id=\"missing\""),
        (
            3,
            "</Relationships>",
            r#"<Relationship Id="sheet" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (4, "<sheetData>", r#"<sheetData xmlns="urn:other">"#),
    ] {
        let mut entries = valid.clone();
        entries[index].1 = entries[index].1.replace(from, to);
        assert!(
            read(
                &archive(&entries, CompressionMethod::Deflated),
                &XlsxLimits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn output_projection_and_storage_budgets_reject_before_building() {
    let rows = vec![
        row(Value::String("hello".into())),
        row(Value::String("world".into())),
    ];
    for (limits, expected) in [
        (options(|l| l.max_cells = 1), XlsxLimit::Cells),
        (
            options(|l| l.max_materialized_cells = 1),
            XlsxLimit::MaterializedCells,
        ),
        (
            options(|l| l.max_materialized_text_bytes = 4),
            XlsxLimit::MaterializedTextBytes,
        ),
        (
            options(|l| l.max_shared_strings = 1),
            XlsxLimit::SharedStrings,
        ),
        (
            options(|l| l.max_expanded_bytes = 1024),
            XlsxLimit::ExpandedBytes,
        ),
    ] {
        limit(
            to_bytes_bounded(&schema(), &rows, None, 1, &[], false, &limits).unwrap_err(),
            expected,
        );
    }
}

#[test]
fn actual_sparse_extents_and_lying_dimensions_are_bounded() {
    for content in [
        sheet(
            r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="100000"><c r="ZZ100000"><v>2</v></c></row>"#,
        ),
        format!(
            r#"<worksheet xmlns="{NS}"><dimension ref="A1"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="100000"><c r="ZZ100000"><v>2</v></c></row></sheetData></worksheet>"#
        ),
    ] {
        let bytes = archive(&parts(content, None), CompressionMethod::Deflated);
        limit(
            read(&bytes, &XlsxLimits::default()).unwrap_err(),
            XlsxLimit::DenseCells,
        );
    }
    let bytes = archive(
        &parts(
            format!(
                r#"<worksheet xmlns="{NS}"><dimension ref="A1:XFD1048576"/><sheetData/></worksheet>"#
            ),
            None,
        ),
        CompressionMethod::Deflated,
    );
    limit(
        read(&bytes, &XlsxLimits::default()).unwrap_err(),
        XlsxLimit::Rows,
    );
    let bytes = fixture(r#"<row r="1"><c r="XFD1"><v>1</v></c></row>"#);
    limit(
        read(&bytes, &XlsxLimits::default()).unwrap_err(),
        XlsxLimit::Columns,
    );
    for reference in [
        "A0",
        "A9999999999999999999999",
        "ZZZZZZZZZZZ1",
        "a1",
        "$A$1",
    ] {
        assert!(
            read(
                &fixture(&format!(
                    r#"<row r="1"><c r="{reference}"><v>1</v></c></row>"#
                )),
                &XlsxLimits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn cell_projection_and_string_replication_limits_precede_materialization() {
    let bytes = fixture(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row>"#);
    limit(
        read(&bytes, &options(|l| l.max_cells = 1)).unwrap_err(),
        XlsxLimit::Cells,
    );
    let bytes = fixture(r#"<row r="100"><c r="A100"><v>1</v></c></row>"#);
    limit(
        read(&bytes, &options(|l| l.max_materialized_cells = 10)).unwrap_err(),
        XlsxLimit::MaterializedCells,
    );
    let bytes = archive(
        &parts(
            sheet(r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>0</v></c></row>"#),
            Some(&format!(
                r#"<sst xmlns="{NS}"><si><t>{}</t></si></sst>"#,
                "a".repeat(256)
            )),
        ),
        CompressionMethod::Deflated,
    );
    limit(
        read(&bytes, &options(|l| l.max_materialized_text_bytes = 400)).unwrap_err(),
        XlsxLimit::MaterializedTextBytes,
    );
}

#[test]
fn shared_string_actual_and_declared_counts_and_combined_rich_text_are_bounded() {
    for strings in [
        format!(r#"<sst xmlns="{NS}" uniqueCount="999999999"><si><t>x</t></si></sst>"#),
        format!(r#"<sst xmlns="{NS}" uniqueCount="1"><si><t>x</t></si><si><t>y</t></si></sst>"#),
    ] {
        let bytes = archive(
            &parts(sheet(""), Some(&strings)),
            CompressionMethod::Deflated,
        );
        limit(
            read(&bytes, &options(|l| l.max_shared_strings = 1)).unwrap_err(),
            XlsxLimit::SharedStrings,
        );
    }
    let strings = format!(
        r#"<sst xmlns="{NS}"><si><r><t>{0}</t></r><r><t>{0}</t></r></si></sst>"#,
        "x".repeat(100)
    );
    let bytes = archive(
        &parts(sheet(""), Some(&strings)),
        CompressionMethod::Deflated,
    );
    limit(
        read(&bytes, &options(|l| l.max_string_bytes = 150)).unwrap_err(),
        XlsxLimit::StringBytes,
    );
}

#[test]
fn xml_depth_nodes_text_and_entities_are_bounded() {
    let bytes = fixture(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row>"#);
    limit(
        read(&bytes, &options(|l| l.max_xml_depth = 4)).unwrap_err(),
        XlsxLimit::XmlDepth,
    );
    limit(
        read(&bytes, &options(|l| l.max_xml_nodes = 5)).unwrap_err(),
        XlsxLimit::XmlNodes,
    );
    limit(
        read(&bytes, &options(|l| l.max_xml_text_bytes = 10)).unwrap_err(),
        XlsxLimit::XmlTextBytes,
    );
    let bytes = archive(
        &parts(
            format!(
                r#"<!DOCTYPE worksheet [<!ENTITY evil "expanded">]>{}"#,
                sheet("")
            ),
            None,
        ),
        CompressionMethod::Deflated,
    );
    assert!(matches!(
        read(&bytes, &XlsxLimits::default()),
        Err(XlsxFormatError::UnsupportedBoundedProfile(_))
    ));
}

#[test]
fn duplicate_parts_and_mismatched_local_names_are_rejected() {
    let mut entries = parts(sheet(""), None);
    entries.push(("xl/worksheets/sheet2.xml".into(), sheet("")));
    let mut bytes = archive(&entries, CompressionMethod::Stored);
    let name = b"xl/worksheets/sheet2.xml";
    let positions: Vec<_> = bytes
        .windows(name.len())
        .enumerate()
        .filter(|(_, b)| *b == name)
        .map(|(i, _)| i)
        .collect();
    let original = bytes.clone();
    for p in positions {
        bytes[p + name.len() - 5] = b'1';
    }
    assert!(matches!(
        read(&bytes, &XlsxLimits::default()),
        Err(XlsxFormatError::InvalidBoundedPackage(_))
    ));
    let mut bytes = original;
    bytes[30] = b'x';
    assert!(matches!(
        read(&bytes, &XlsxLimits::default()),
        Err(XlsxFormatError::InvalidBoundedPackage(_))
    ));
}

#[test]
fn malformed_ambiguous_external_and_unsupported_profiles_reject() {
    for inner in [
        r#"<row r="1"><c r="A1"><v>1</v></c><c r="A1"><v>2</v></c></row>"#,
        r#"<row r="1"><c r="A1"><v>1</v><v>2</v></c></row>"#,
        r#"<row r="1"><c r="A1"><f>1+1</f><v>2</v></c></row>"#,
        r#"<row r="1"><c r="A1" t="s"><v>99999999</v></c></row>"#,
        r#"<row r="1"><c><v>1</v></c></row>"#,
        r#"<row r="1"><c r="A1"><v>1</c></row>"#,
        r#"<row r="1"><c r="A1" r="B1"><v>1</v></c></row>"#,
    ] {
        assert!(
            read(&fixture(inner), &XlsxLimits::default()).is_err(),
            "{inner}"
        );
    }
    for encoding in ["UTF-16", "ISO-8859-1"] {
        let bytes = archive(
            &parts(
                format!(
                    r#"<?xml version="1.0" encoding="{encoding}"?>{}"#,
                    sheet("")
                ),
                None,
            ),
            CompressionMethod::Deflated,
        );
        assert!(matches!(
            read(&bytes, &XlsxLimits::default()),
            Err(XlsxFormatError::UnsupportedBoundedProfile(_))
        ));
    }
    let mut entries = parts(sheet(""), None);
    entries[3].1 = entries[3]
        .1
        .replace("Target=", "TargetMode=\"External\" Target=");
    assert!(matches!(
        read(
            &archive(&entries, CompressionMethod::Deflated),
            &XlsxLimits::default()
        ),
        Err(XlsxFormatError::UnsupportedBoundedProfile(_))
    ));
}

#[test]
fn wrong_sheet_types_nonfinite_and_precision_failures_stay_typed() {
    let bytes = fixture(r#"<row r="1"><c r="A1" t="inlineStr"><is><t>bad</t></is></c></row>"#);
    assert!(matches!(
        from_bytes_bounded(
            &bytes,
            &schema(),
            Some("Missing"),
            1,
            &[],
            false,
            &XlsxLimits::default()
        ),
        Err(XlsxFormatError::MissingWorksheet(_))
    ));
    let int_schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Int)]);
    assert!(matches!(
        from_bytes_bounded(
            &bytes,
            &int_schema,
            None,
            1,
            &[],
            false,
            &XlsxLimits::default()
        ),
        Err(XlsxFormatError::Parse { .. })
    ));
    for value in [
        Value::Int((1 << 53) + 1),
        Value::Float(f64::INFINITY),
        Value::Bool(true),
    ] {
        assert!(matches!(
            to_bytes_bounded(
                &int_schema,
                &[row(value)],
                None,
                1,
                &[],
                false,
                &XlsxLimits::default()
            ),
            Err(XlsxFormatError::ValueType { .. })
        ));
    }
    for value in ["NaN", "inf"] {
        assert!(matches!(
            read(
                &fixture(&format!(r#"<row r="1"><c r="A1"><v>{value}</v></c></row>"#)),
                &XlsxLimits::default()
            ),
            Err(XlsxFormatError::UnsupportedBoundedProfile(_))
        ));
    }
}

#[test]
fn selected_integers_reject_rounding_but_float_domain_is_unchanged() {
    let int_schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Int)]);
    for value in ["9007199254740993", "9007199254740993e0", "1e200"] {
        let bytes = fixture(&format!(r#"<row r="1"><c r="A1"><v>{value}</v></c></row>"#));
        assert!(matches!(
            from_bytes_bounded(
                &bytes,
                &int_schema,
                None,
                1,
                &[],
                false,
                &XlsxLimits::default()
            ),
            Err(XlsxFormatError::Parse { .. })
        ));
    }
    let float_schema =
        SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Float)]);
    for value in [1e200, 1e-300, 12.25] {
        let bytes = to_bytes_bounded(
            &float_schema,
            &[row(Value::Float(value))],
            None,
            1,
            &[],
            false,
            &XlsxLimits::default(),
        )
        .unwrap();
        assert_eq!(
            from_bytes_bounded(
                &bytes,
                &float_schema,
                None,
                1,
                &[],
                false,
                &XlsxLimits::default()
            )
            .unwrap(),
            vec![row(Value::Float(value))]
        );
    }
}

#[test]
fn writer_limits_and_atomic_valid_failure_corrected_retry() {
    let good = vec![row(Value::String("valid".into()))];
    let first = to_bytes_bounded(
        &schema(),
        &good,
        None,
        1,
        &[],
        false,
        &XlsxLimits::default(),
    )
    .unwrap();
    limit(
        to_bytes_bounded(
            &schema(),
            &good,
            None,
            1,
            &[],
            false,
            &options(|l| l.max_output_bytes = 100),
        )
        .unwrap_err(),
        XlsxLimit::OutputBytes,
    );
    limit(
        to_bytes_bounded(
            &schema(),
            &good,
            None,
            1,
            &[],
            false,
            &options(|l| l.max_string_bytes = 4),
        )
        .unwrap_err(),
        XlsxLimit::StringBytes,
    );
    limit(
        to_bytes_bounded(
            &schema(),
            &good,
            None,
            1,
            &[],
            false,
            &options(|l| l.max_entry_bytes = 100),
        )
        .unwrap_err(),
        XlsxLimit::EntryBytes,
    );
    limit(
        to_bytes_bounded(
            &schema(),
            &[good[0].clone(), good[0].clone()],
            None,
            1,
            &[],
            false,
            &options(|l| l.max_rows = 1),
        )
        .unwrap_err(),
        XlsxLimit::Rows,
    );
    let second = to_bytes_bounded(
        &schema(),
        &good,
        None,
        1,
        &[],
        false,
        &XlsxLimits::default(),
    )
    .unwrap();
    assert_eq!(read(&first, &XlsxLimits::default()).unwrap(), good);
    assert!(
        read(
            &fixture(r#"<row r="1"><c r="A1"><v>NaN</v></c></row>"#),
            &XlsxLimits::default()
        )
        .is_err()
    );
    assert_eq!(read(&second, &XlsxLimits::default()).unwrap(), good);
}

#[test]
fn invalid_limit_configuration_cannot_reach_io() {
    assert!(matches!(
        XlsxLimits::new(XlsxLimitOptions {
            max_entries: 0,
            ..Default::default()
        }),
        Err(XlsxFormatError::InvalidLimit(XlsxLimit::Entries))
    ));
    assert!(matches!(
        XlsxLimits::new(XlsxLimitOptions {
            max_xml_depth: 257,
            ..Default::default()
        }),
        Err(XlsxFormatError::InvalidLimit(XlsxLimit::XmlDepth))
    ));
    assert!(matches!(
        XlsxLimits::new(XlsxLimitOptions {
            max_rows: usize::MAX,
            ..Default::default()
        }),
        Err(XlsxFormatError::InvalidLimit(XlsxLimit::Rows))
    ));
}
