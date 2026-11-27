use crate::preview_table::PreviewTable;
use crate::run_report::MAX_PREVIEW_BYTES;
use editor_ui::mapping_checks::{Assertion, KeyPart};

#[test]
fn editor_workflow_reference_and_reordered_preview_use_the_same_csv_boundary() {
    let options = mapping::FormatOptions {
        delimiter: Some(';'),
        csv_quote: Some('\''),
        ..Default::default()
    };
    let schema = ir::SchemaNode::group(
        "row",
        vec![
            ir::SchemaNode::scalar("id", ir::ScalarType::String),
            ir::SchemaNode::scalar("Desc", ir::ScalarType::String),
        ],
    );
    let decode = |bytes: &[u8]| PreviewTable::parse(bytes, &options, &schema);
    let reference = decode(b"id;Desc\n'00;1';'PAINT ''BRUSH'''\n002;TAPE\n").unwrap();
    let check = Assertion::capture_cell(
        vec![KeyPart {
            column: "id".into(),
            value: "00;1".into(),
        }],
        "Desc".into(),
        Some(&reference.headers),
        &reference.rows,
    )
    .unwrap();
    for (input, expected_row) in [
        ("id;Desc\n'00;1';'paint ''brush'''\n002;TAPE\n", 0),
        ("id;Desc\n002;TAPE\n'00;1';'paint ''brush'''\n", 1),
    ] {
        let actual = decode(input.as_bytes()).unwrap();
        assert!(actual.has_headers);
        assert_eq!(
            check
                .failed_cell(Some(&actual.headers), &actual.rows)
                .unwrap(),
            Some((expected_row, "Desc"))
        );
        assert_eq!(
            actual.rows[expected_row][actual.column("Desc").unwrap()],
            "paint 'brush'"
        );
    }

    // One valid record ending exactly at the byte limit; the extra byte is
    // valid CSV too, so the refusal distinguishes size from malformed input.
    let mut at_limit = b"id;Desc\n001;".to_vec();
    at_limit.resize(MAX_PREVIEW_BYTES - 1, b'x');
    at_limit.push(b'\n');
    let complete = decode(&at_limit).expect("exact limit admits the complete data row");
    assert_eq!(complete.rows.len(), 1);
    assert_eq!(complete.rows[0][1].len(), MAX_PREVIEW_BYTES - 13);
    at_limit.push(b'\n');
    assert!(decode(&at_limit).is_err());
}
