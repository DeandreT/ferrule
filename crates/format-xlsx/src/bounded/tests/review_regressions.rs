use super::*;

#[test]
fn fragmented_shared_index_cannot_undercharge_materialized_text() {
    let strings = format!(
        r#"<sst xmlns="{NS}"><si><t>{}</t></si><si><t>x</t></si></sst>"#,
        "L".repeat(512)
    );
    let bytes = archive(
        &parts(
            sheet(r#"<row r="1"><c r="A1" t="s"><v>0&#49;</v></c></row>"#),
            Some(&strings),
        ),
        CompressionMethod::Deflated,
    );
    // The old library reads the first Text event (0), not concatenated text (01).
    assert_eq!(
        crate::from_bytes(&bytes, &schema(), None, 1, &[], false).unwrap(),
        vec![row(Value::String("L".repeat(512)))]
    );
    assert!(matches!(
        read(&bytes, &options(|l| l.max_materialized_text_bytes = 800)),
        Err(XlsxFormatError::UnsupportedBoundedProfile(_))
    ));
}

#[test]
fn competing_local_relationship_ids_cannot_select_another_sheet() {
    let mut entries = parts(sheet(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#), None);
    entries[2].1 = entries[2]
        .1
        .replace(r#"r:id="sheet""#, r#"r:id="sheet" id="large""#);
    entries[3].1 = entries[3].1.replace("</Relationships>", r#"<Relationship Id="large" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#);
    let large = (1..=8)
        .map(|r| format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c></row>"#))
        .collect::<String>();
    entries.push(("xl/worksheets/sheet2.xml".into(), sheet(&large)));
    let bytes = archive(&entries, CompressionMethod::Deflated);
    assert_eq!(
        crate::from_bytes(&bytes, &schema(), None, 1, &[], false)
            .unwrap()
            .len(),
        8
    );
    assert!(matches!(
        read(&bytes, &options(|l| l.max_materialized_cells = 2)),
        Err(XlsxFormatError::InvalidBoundedPackage(_))
    ));
}

#[test]
fn rounded_fraction_and_underflow_cannot_become_selected_integers() {
    let schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Int)]);
    for value in [
        "1.0000000000000001",
        "0.99999999999999999",
        "1e-400",
        "-1e-400",
    ] {
        let bytes = fixture(&format!(r#"<row r="1"><c r="A1"><v>{value}</v></c></row>"#));
        assert!(
            matches!(
                from_bytes_bounded(&bytes, &schema, None, 1, &[], false, &XlsxLimits::default()),
                Err(XlsxFormatError::Parse { .. })
            ),
            "{value}"
        );
    }
}

#[test]
fn tiny_number_output_is_charged_before_xml_materialization() {
    let schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Float)]);
    let rows = vec![row(Value::Float(1e-300)); 1500];
    let limits = options(|l| l.max_entry_bytes = 520_000);
    let bytes = crate::to_bytes(&schema, &rows, None, 1, &[], false).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("xl/worksheets/sheet1.xml").unwrap().size() > 520_000);
    let options = FlatTableWriteOptions {
        sheet: None,
        start_row: 1,
        columns: &[],
        headers: &[],
        has_header: false,
    };
    assert!(matches!(
        super::super::write::preflight(&schema, &rows, options, &limits),
        Err(XlsxFormatError::LimitExceeded {
            limit: XlsxLimit::EntryBytes,
            ..
        })
    ));
}

#[test]
fn missing_sheet_selector_is_bounded_before_error_ownership() {
    let bytes = fixture("");
    let name = "m".repeat(1024);
    limit(
        from_bytes_bounded(
            &bytes,
            &schema(),
            Some(&name),
            1,
            &[],
            false,
            &options(|l| l.max_string_bytes = 256),
        )
        .unwrap_err(),
        XlsxLimit::StringBytes,
    );
}

#[test]
fn all_scalar_value_kinds_reject_entities_cdata_and_fragmentation() {
    for kind in ["n", "s", "b", "e", "str", "d", "inlineStr"] {
        for value in [
            "0&#49;",
            "&#48;1",
            "0<![CDATA[1]]>",
            "<![CDATA[01]]>",
            "0<!--split-->1",
            "0\r\n1",
        ] {
            let strings =
                format!(r#"<sst xmlns="{NS}"><si><t>zero</t></si><si><t>one</t></si></sst>"#);
            let bytes = archive(
                &parts(
                    sheet(&format!(
                        r#"<row r="1"><c r="A1" t="{kind}"><v>{value}</v></c></row>"#
                    )),
                    Some(&strings),
                ),
                CompressionMethod::Deflated,
            );
            assert!(
                matches!(
                    read(&bytes, &XlsxLimits::default()),
                    Err(XlsxFormatError::UnsupportedBoundedProfile(_))
                ),
                "{kind}: {value}"
            );
        }
    }
}

#[test]
fn rich_string_entities_and_empty_scalar_values_keep_legacy_parity() {
    let strings = format!(
        r#"<sst xmlns="{NS}"><si><r><t>A&amp;</t></r><r><t><![CDATA[B]]>&#xE9;</t></r></si></sst>"#
    );
    for cell in [
        r#"<c r="A1" t="s"><v>0</v></c>"#,
        r#"<c r="A1" t="inlineStr"><is><t>A&amp;<![CDATA[B]]>&#xE9;</t></is></c>"#,
        r#"<c r="A1" t="s"><v/></c>"#,
        r#"<c r="A1"><v/></c>"#,
    ] {
        let bytes = archive(
            &parts(
                sheet(&format!(r#"<row r="1">{cell}</row>"#)),
                Some(&strings),
            ),
            CompressionMethod::Deflated,
        );
        assert_eq!(
            read(&bytes, &XlsxLimits::default()).unwrap(),
            crate::from_bytes(&bytes, &schema(), None, 1, &[], false).unwrap()
        );
    }
}

#[test]
fn competing_attributes_reject_in_either_order_and_with_other_prefixes() {
    for attributes in [
        r#"r:id="sheet" id="large""#,
        r#"id="large" r:id="sheet""#,
        r#"r:id="sheet" x:id="large""#,
        r#"r:id="sheet" r:id="sheet""#,
    ] {
        let mut entries = parts(sheet(""), None);
        entries[2].1 = entries[2].1.replace(r#"r:id="sheet""#, attributes);
        let bytes = archive(&entries, CompressionMethod::Deflated);
        assert!(
            matches!(
                read(&bytes, &XlsxLimits::default()),
                Err(XlsxFormatError::InvalidBoundedPackage(_))
            ),
            "{attributes}"
        );
    }
}

#[test]
fn raw_structural_attributes_cannot_be_reinterpreted_by_unescaping() {
    for (index, before, after) in [
        (2, r#"r:id="sheet""#, r#"r:id="she&#101;t""#),
        (3, r#"Id="sheet""#, r#"Id="she&#101;t""#),
        (
            3,
            "Target=\"worksheets/sheet1.xml\"",
            "Target=\"worksheets/sheet&#49;.xml\"",
        ),
        (4, r#"r="1""#, r#"r="&#49;""#),
        (4, r#"r="A1""#, r#"r="A&#49;""#),
        (4, r#"t="s""#, r#"t="&#115;""#),
    ] {
        let strings = format!(r#"<sst xmlns="{NS}"><si><t>zero</t></si></sst>"#);
        let mut entries = parts(
            sheet(r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#),
            Some(&strings),
        );
        assert!(entries[index].1.contains(before));
        entries[index].1 = entries[index].1.replace(before, after);
        assert!(matches!(
            read(
                &archive(&entries, CompressionMethod::Deflated),
                &XlsxLimits::default()
            ),
            Err(XlsxFormatError::UnsupportedBoundedProfile(_))
        ));
    }
}

#[test]
fn exact_decimal_integer_forms_and_float_domain_remain_supported() {
    let int_schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Int)]);
    for (token, expected) in [
        ("1.0000000000000000", 1),
        ("100e-2", 1),
        ("+0001.00", 1),
        ("-0.0", 0),
        ("9007199254740992.0", 1i64 << 53),
        ("-9007199254740992e0", -(1i64 << 53)),
    ] {
        let bytes = fixture(&format!(r#"<row r="1"><c r="A1"><v>{token}</v></c></row>"#));
        assert_eq!(
            from_bytes_bounded(
                &bytes,
                &int_schema,
                None,
                1,
                &[],
                false,
                &XlsxLimits::default()
            )
            .unwrap(),
            vec![row(Value::Int(expected))],
            "{token}"
        );
    }
    let float_schema =
        SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Float)]);
    for token in ["1.0000000000000001", "1e-400", "-1e-400", "1e200"] {
        let bytes = fixture(&format!(r#"<row r="1"><c r="A1"><v>{token}</v></c></row>"#));
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
            crate::from_bytes(&bytes, &float_schema, None, 1, &[], false).unwrap()
        );
    }
}

#[test]
fn numeric_writer_charges_pinned_display_before_building() {
    let schema = SchemaNode::group("rows", vec![SchemaNode::scalar("value", ScalarType::Float)]);
    let options = FlatTableWriteOptions {
        sheet: None,
        start_row: 1,
        columns: &[],
        headers: &[],
        has_header: false,
    };
    for value in [
        1e-300,
        -1e-300,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
    ] {
        let rows = [row(Value::Float(value))];
        let token = value.to_string();
        let bytes = crate::to_bytes(&schema, &rows, None, 1, &[], false).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(),
            &mut text,
        )
        .unwrap();
        assert!(text.contains(&format!("<v>{token}</v>")));
        let limits = super::options(|l| l.max_string_bytes = token.len() - 1);
        assert!(matches!(
            super::super::write::preflight(&schema, &rows, options, &limits),
            Err(XlsxFormatError::LimitExceeded {
                limit: XlsxLimit::StringBytes,
                ..
            })
        ));
    }
}

#[test]
fn exact_integer_token_check_handles_scale_zeroes_and_extreme_exponents() {
    use super::super::numeric::exact_integer;
    for (token, value) in [
        ("0000100.00e-2", 1),
        (".0", 0),
        ("1.", 1),
        ("-0e99999999999999999999999999999999", 0),
        ("900719925474099200000000e-8", 1i64 << 53),
        ("-9007199254740992.0000", -(1i64 << 53)),
    ] {
        assert_eq!(exact_integer(token), Some(value), "{token}");
    }
    for token in [
        ".",
        "1..0",
        "1e",
        "1e--1",
        "1e-9223372036854775808",
        "1e999999999999999999999999",
        "9007199254740993",
        "100.00000000000000000000000000000000001",
        "1e-400",
    ] {
        assert_eq!(exact_integer(token), None, "{token}");
    }
}

#[test]
fn selector_limit_precedes_invalid_package_and_preserves_small_missing_name() {
    let limits = options(|l| l.max_string_bytes = 256);
    let large = "é".repeat(129);
    limit(
        from_bytes_bounded(&[], &schema(), Some(&large), 1, &[], false, &limits).unwrap_err(),
        XlsxLimit::StringBytes,
    );
    assert!(
        matches!(from_bytes_bounded(&fixture(""), &schema(), Some("Missing"), 1, &[], false, &limits), Err(XlsxFormatError::MissingWorksheet(name)) if name == "Missing")
    );
}
