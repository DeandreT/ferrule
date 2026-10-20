use format_csv::{
    CsvByteDecodingPolicy, CsvReadError, CsvReadLimit, CsvReadOptions, MAX_CSV_INPUT_BYTES,
    from_bytes_with_decoding_policy_bounded, from_bytes_with_options_bounded,
};
use ir::{Instance, ScalarType, SchemaNode, Value};

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
}

fn row(value: &str) -> Vec<Instance> {
    vec![Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )]
}

fn read(bytes: &[u8]) -> Result<Vec<Instance>, CsvReadError> {
    from_bytes_with_decoding_policy_bounded(
        bytes,
        &schema(),
        &CsvReadOptions::default(),
        CsvByteDecodingPolicy::BomAwareNonFlushingReplacement,
    )
}

fn utf16(text: &str, be: bool) -> Vec<u8> {
    let mut bytes = if be {
        vec![0xfe, 0xff]
    } else {
        vec![0xff, 0xfe]
    };
    bytes.extend(
        text.encode_utf16()
            .flat_map(|x| if be { x.to_be_bytes() } else { x.to_le_bytes() }),
    );
    bytes
}

fn utf32(text: &str, be: bool) -> Vec<u8> {
    let mut bytes = if be {
        vec![0, 0, 0xfe, 0xff]
    } else {
        vec![0xff, 0xfe, 0, 0]
    };
    bytes.extend(text.chars().flat_map(|x| {
        if be {
            (x as u32).to_be_bytes()
        } else {
            (x as u32).to_le_bytes()
        }
    }));
    bytes
}

#[test]
fn bom_profiles_preserve_complete_rows_and_supplementary_scalars() {
    let text = "Value\r\nSnow ☃🙂\r\n";
    let mut utf8_bom = vec![0xef, 0xbb, 0xbf];
    utf8_bom.extend_from_slice(text.as_bytes());
    for bytes in [
        text.as_bytes().to_vec(),
        utf8_bom,
        utf16(text, false),
        utf16(text, true),
        utf32(text, false),
        utf32(text, true),
    ] {
        assert_eq!(read(&bytes).unwrap(), row("Snow ☃🙂"));
    }
    for bytes in [
        vec![],
        vec![0xef, 0xbb, 0xbf],
        vec![0xff, 0xfe],
        vec![0xfe, 0xff],
        vec![0xff, 0xfe, 0, 0],
        vec![0, 0, 0xfe, 0xff],
    ] {
        assert!(read(&bytes).unwrap().is_empty());
    }
}

#[test]
fn malformed_interior_and_pending_terminal_state_are_different() {
    let mut cases = vec![];
    let mut bytes = b"Value\nV".to_vec();
    bytes.extend_from_slice(&[0xe2, 0x82]);
    cases.push((bytes.clone(), "V"));
    bytes.push(b'\n');
    cases.push((bytes, "V\u{fffd}"));
    let mut bytes = b"Value\nV".to_vec();
    bytes.extend_from_slice(&[0xc0, 0xaf, b'\n']);
    cases.push((bytes, "V\u{fffd}\u{fffd}"));
    let mut bytes = utf16("Value\nV", false);
    bytes.extend_from_slice(&[0x00, 0xd8]);
    cases.push((bytes.clone(), "V"));
    bytes.extend_from_slice(&[b'\n', 0]);
    cases.push((bytes, "V\u{fffd}"));
    let mut bytes = utf16("Value\nV", false);
    bytes.push(0x41);
    cases.push((bytes, "V"));
    let mut bytes = utf16("Value\nV", true);
    bytes.extend_from_slice(&[0xdc, 0, 0, b'\n']);
    cases.push((bytes, "V\u{fffd}"));
    let mut bytes = utf32("Value\nV", false);
    bytes.extend_from_slice(&[0, 0, 0x11, 0, b'\n', 0, 0, 0]);
    cases.push((bytes, "V\u{fffd}"));
    let mut bytes = utf32("Value\nV", true);
    bytes.extend_from_slice(&[0, 0, 0xd8, 0, 0, 0, 0, b'\n']);
    cases.push((bytes, "V\u{fffd}"));
    let mut bytes = utf32("Value\nV", true);
    bytes.extend_from_slice(&[0, 0x41]);
    cases.push((bytes, "V"));
    for (bytes, expected) in cases {
        assert_eq!(read(&bytes).unwrap(), row(expected));
        assert!(matches!(
            from_bytes_with_options_bounded(&bytes, &schema(), &CsvReadOptions::default()),
            Err(CsvReadError::Encoding(_))
        ));
    }
}

#[test]
fn strict_companion_remains_the_existing_policy() {
    let bytes = b"Value\ntext\n";
    assert_eq!(
        from_bytes_with_options_bounded(bytes, &schema(), &CsvReadOptions::default()).unwrap(),
        from_bytes_with_decoding_policy_bounded(
            bytes,
            &schema(),
            &CsvReadOptions::default(),
            CsvByteDecodingPolicy::StrictUtf8
        )
        .unwrap()
    );
    assert!(matches!(
        from_bytes_with_decoding_policy_bounded(
            b"Value\n\xff",
            &schema(),
            &CsvReadOptions::default(),
            CsvByteDecodingPolicy::StrictUtf8
        ),
        Err(CsvReadError::Encoding(_))
    ));
}

#[test]
fn physical_and_decoded_byte_limits_are_distinct() {
    let bytes = vec![0xff; MAX_CSV_INPUT_BYTES + 1];
    assert!(
        matches!(read(&bytes), Err(CsvReadError::Limit { kind: CsvReadLimit::InputBytes, maximum: MAX_CSV_INPUT_BYTES, observed, .. }) if observed == bytes.len())
    );
    let bytes = vec![0xff; MAX_CSV_INPUT_BYTES / 3 + 1];
    assert!(
        matches!(read(&bytes), Err(CsvReadError::Limit { kind: CsvReadLimit::DecodedBytes, maximum: MAX_CSV_INPUT_BYTES, observed, .. }) if observed == bytes.len() * 3)
    );
    let invalid = SchemaNode::scalar("bad", ScalarType::String);
    assert!(matches!(
        from_bytes_with_decoding_policy_bounded(
            &vec![0; MAX_CSV_INPUT_BYTES + 1],
            &invalid,
            &CsvReadOptions::default(),
            CsvByteDecodingPolicy::BomAwareNonFlushingReplacement
        ),
        Err(CsvReadError::Limit {
            kind: CsvReadLimit::InputBytes,
            ..
        })
    ));
}

#[test]
fn decoded_limit_admits_exact_size_and_refuses_one_more_byte() {
    let mut bytes = b"Value\n".to_vec();
    let mut remaining = MAX_CSV_INPUT_BYTES - bytes.len();
    let mut lengths = Vec::new();
    while remaining >= 256 {
        bytes.extend_from_slice(&[0xff; 85]);
        bytes.push(b'\n');
        lengths.push(85);
        remaining -= 256;
    }
    assert_eq!(remaining, 250);
    bytes.extend_from_slice(&[0xff; 83]);
    bytes.push(b'\n');
    lengths.push(83);
    let rows = read(&bytes).unwrap();
    assert_eq!(rows.len(), lengths.len());
    for (row, length) in rows.iter().zip(lengths) {
        assert_eq!(row, &self::row(&"\u{fffd}".repeat(length))[0]);
    }
    bytes.push(b'x');
    assert!(matches!(read(&bytes), Err(CsvReadError::Limit {
        kind: CsvReadLimit::DecodedBytes, maximum: MAX_CSV_INPUT_BYTES, observed, ..
    }) if observed == MAX_CSV_INPUT_BYTES + 1));
}
