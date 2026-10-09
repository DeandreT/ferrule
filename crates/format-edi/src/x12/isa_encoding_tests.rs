use super::*;
use ir::ScalarType;

const EXPECTED_DEFAULT: &str = include_str!("fixtures/isa-4010-default.x12");
const EXPECTED_LF: &str = include_str!("fixtures/isa-4010-lf.x12");
const EXPECTED_CUSTOM: &str = include_str!("fixtures/isa-5010-custom.x12");
const EXPECTED_PARTIAL: &str = include_str!("fixtures/isa-4010-partial.x12");
const EXPECTED_AUTOCOMPLETE: &str = include_str!("fixtures/isa-4010-autocomplete.x12");

fn segment(name: &str, values: &[&str]) -> (SchemaNode, (String, Instance)) {
    let names = (1..=values.len())
        .map(|index| format!("{index:02}"))
        .collect::<Vec<_>>();
    (
        SchemaNode::group(
            name,
            names
                .iter()
                .map(|name| SchemaNode::scalar(name, ScalarType::String))
                .collect(),
        ),
        (
            name.into(),
            Instance::Group(
                names
                    .into_iter()
                    .zip(
                        values
                            .iter()
                            .map(|value| Instance::Scalar(Value::String((*value).into()))),
                    )
                    .collect::<Vec<_>>()
                    .into(),
            ),
        ),
    )
}

fn example() -> (SchemaNode, Instance) {
    let pairs = [
        segment(
            "ISA",
            &[
                "00",
                "",
                "00",
                "",
                "ZZ",
                "SENDER-01",
                "ZZ",
                "RECEIVER-01",
                "261009",
                "1030",
                "U",
                "00401",
                "000000321",
                "0",
                "P",
                ":",
            ],
        ),
        segment(
            "GS",
            &[
                "OW",
                "SENDER-01",
                "RECEIVER-01",
                "20261009",
                "1030",
                "00321",
                "X",
                "004010",
            ],
        ),
        segment("ST", &["940", "0009"]),
        segment("W05", &["N", "DEMO-ORDER-0042"]),
        segment("SE", &["3", "0009"]),
        segment("GE", &["1", "00321"]),
        segment("IEA", &["1", "000000321"]),
    ];
    let (schemas, fields): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    (
        SchemaNode::group("Interchange", schemas),
        Instance::Group(fields.into()),
    )
}

fn change(instance: &mut Instance, segment: &str, element: &str, value: &str) {
    let Instance::Group(fields) = instance else {
        panic!("example root");
    };
    let (_, Instance::Group(elements)) =
        fields.iter_mut().find(|(name, _)| name == segment).unwrap()
    else {
        panic!("example segment");
    };
    let (_, target) = elements
        .iter_mut()
        .find(|(name, _)| name == element)
        .unwrap();
    *target = Instance::Scalar(Value::String(value.into()));
}

fn evidence_root() -> std::path::PathBuf {
    std::env::var_os("FERRULE_X12_ISA_EVIDENCE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("ferrule_native_isa_{}", std::process::id()))
        })
}

fn retain(
    name: &str,
    schema: &SchemaNode,
    instance: &Instance,
    result: &Result<String, EdiFormatError>,
) {
    let root = evidence_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(format!("{name}-input.debug.txt")),
        format!("{schema:#?}\n{instance:#?}\n"),
    )
    .unwrap();
    std::fs::write(
        root.join(format!("{name}-result.debug.txt")),
        format!("{result:#?}\n"),
    )
    .unwrap();
    if let Ok(output) = result {
        std::fs::write(root.join(format!("{name}-output.x12")), output.as_bytes()).unwrap();
    }
}

fn assert_physical_isa(output: &str, element: u8, component: u8, terminator: u8) {
    let bytes = output.as_bytes();
    assert_eq!(&bytes[..3], b"ISA");
    for index in [
        3, 6, 17, 20, 31, 34, 50, 53, 69, 76, 81, 83, 89, 99, 101, 103,
    ] {
        assert_eq!(bytes[index], element, "element delimiter offset {index}");
    }
    assert_eq!(bytes[104], component);
    assert_eq!(bytes[105], terminator);
    assert!(bytes[..106].is_ascii());
}

#[test]
fn complete_isa_unpadded_and_padded_inputs_match_independent_bytes() {
    let (schema, mut instance) = example();
    for (name, padded) in [("unpadded-default", false), ("padded-default", true)] {
        if padded {
            change(&mut instance, "ISA", "02", "          ");
            change(&mut instance, "ISA", "04", "          ");
            change(&mut instance, "ISA", "06", "SENDER-01      ");
            change(&mut instance, "ISA", "08", "RECEIVER-01    ");
        }
        let result = to_string(&schema, &instance);
        retain(name, &schema, &instance, &result);
        let output = result.unwrap();
        assert_eq!(output.as_bytes(), EXPECTED_DEFAULT.as_bytes());
        assert_physical_isa(&output, b'*', b':', b'~');
    }
}

#[test]
fn complete_isa_lf_input_and_custom_5010_preserve_selected_syntax() {
    let (schema, mut instance) = example();
    change(&mut instance, "ISA", "16", ">");
    let lf = Separators {
        component: '>',
        segment: '\n',
        repetition: None,
        ..Separators::default()
    };
    assert_physical_isa(EXPECTED_LF, b'*', b'>', b'\n');
    let input = from_str(EXPECTED_LF, &schema, false);
    std::fs::create_dir_all(evidence_root()).unwrap();
    std::fs::write(
        evidence_root().join("lf-input-result.debug.txt"),
        format!("{input:#?}\n"),
    )
    .unwrap();
    assert!(input.is_ok());
    let result = to_string_with_separators(&schema, &instance, lf);
    retain("lf-4010", &schema, &instance, &result);
    assert!(matches!(
        result,
        Err(EdiFormatError::InvalidX12Separators(_))
    ));
    change(&mut instance, "ISA", "11", "^");
    change(&mut instance, "ISA", "12", "00501");
    change(&mut instance, "GS", "08", "005010");
    let custom = Separators {
        element: '|',
        component: '>',
        segment: '!',
        repetition: Some('^'),
        release: None,
    };
    let result = to_string_with_syntax(&schema, &instance, custom, Some("00501"));
    retain("custom-5010", &schema, &instance, &result);
    let output = result.unwrap();
    assert_eq!(output.as_bytes(), EXPECTED_CUSTOM.as_bytes());
    assert_physical_isa(&output, b'|', b'>', b'!');
}

#[test]
fn complete_isa_refuses_unrepresentable_fields_and_preserves_destination() {
    for (name, field, value) in [
        ("authorization-overflow", "02", "ABCDEFGHIJK"),
        ("security-overflow", "04", "ABCDEFGHIJK"),
        ("sender-overflow", "06", "ABCDEFGHIJKLMNOP"),
        ("receiver-overflow", "08", "ABCDEFGHIJKLMNOP"),
        ("qualifier-short", "01", "0"),
        ("qualifier-long", "01", "000"),
        ("date-short", "09", "26100"),
        ("time-short", "10", "103"),
        ("control-short", "13", "321"),
        ("control-missing", "13", ""),
        ("control-long", "13", "0000000321"),
        ("standards-long", "11", "UU"),
        ("sender-unicode", "06", "CAFÉ"),
        ("authorization-unicode", "02", "é"),
        ("sender-control", "06", "ABC\tDEF"),
    ] {
        let (schema, mut instance) = example();
        change(&mut instance, "ISA", field, value);
        let result = to_string(&schema, &instance);
        retain(name, &schema, &instance, &result);
        assert!(
            matches!(result, Err(EdiFormatError::InvalidEnvelopeElement { .. })),
            "{name}: {result:?}"
        );
        let destination = evidence_root().join(format!("{name}-destination.x12"));
        std::fs::write(&destination, b"existing destination").unwrap();
        let result = write(&destination, &schema, &instance);
        std::fs::write(
            evidence_root().join(format!("{name}-file-result.debug.txt")),
            format!("{result:#?}\n"),
        )
        .unwrap();
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"existing destination"
        );
    }
}

#[test]
fn complete_isa_refuses_release_escaped_reserved_data_and_non_ascii_syntax() {
    for (name, value) in [
        ("element-reserved", "S*ENDER"),
        ("component-reserved", "S:ENDER"),
        ("terminator-reserved", "S~ENDER"),
        ("release-literal", "S?ENDER"),
    ] {
        let (schema, mut instance) = example();
        change(&mut instance, "ISA", "06", value);
        let result = to_string_with_separators(
            &schema,
            &instance,
            Separators {
                release: Some('?'),
                ..Separators::default()
            },
        );
        retain(name, &schema, &instance, &result);
        assert!(
            matches!(result, Err(EdiFormatError::InvalidEnvelopeElement { .. })),
            "{name}: {result:?}"
        );
    }
    let (schema, instance) = example();
    let result = to_string_with_separators(
        &schema,
        &instance,
        Separators {
            element: '§',
            ..Separators::default()
        },
    );
    retain("non-ascii-syntax", &schema, &instance, &result);
    assert!(matches!(
        result,
        Err(EdiFormatError::InvalidX12Separators(_))
    ));
}

#[test]
fn isa_partial_schema_keeps_existing_positional_output() {
    let (schema, instance) = example();
    let SchemaKind::Group { mut children, .. } = schema.kind else {
        unreachable!();
    };
    let Instance::Group(mut fields) = instance else {
        unreachable!();
    };
    let SchemaKind::Group {
        children: elements, ..
    } = &mut children[0].kind
    else {
        unreachable!();
    };
    elements.truncate(12);
    let Instance::Group(elements) = &mut fields[0].1 else {
        unreachable!();
    };
    elements.truncate(12);
    let schema = SchemaNode::group("Interchange", vec![children.remove(0), children.remove(2)]);
    let instance = Instance::Group(vec![fields.remove(0), fields.remove(2)].into());
    let result = to_string(&schema, &instance);
    retain("partial-4010", &schema, &instance, &result);
    assert_eq!(result.unwrap().as_bytes(), EXPECTED_PARTIAL.as_bytes());
}

#[test]
fn complete_isa_autocomplete_keeps_its_existing_lexical_conversion() {
    let (schema, mut instance) = example();
    change(&mut instance, "ISA", "09", "2026-10-09");
    change(&mut instance, "ISA", "10", "10:30:00");
    change(&mut instance, "ISA", "13", "321");
    let result = to_string_with_syntax_and_autocomplete(
        &schema,
        &instance,
        Separators {
            release: Some('?'),
            ..Separators::default()
        },
        Some("00401"),
        Autocomplete {
            current_datetime: "2026-10-09T10:30:00Z",
            request_acknowledgement: false,
            transaction_set: Some("940"),
        },
    );
    retain("autocomplete-4010", &schema, &instance, &result);
    assert!(
        matches!(result, Err(EdiFormatError::InvalidEnvelopeElement { ref element, .. }) if element == "GS05")
    );
    change(&mut instance, "GS", "05", "10:30:00");
    let result = to_string_with_syntax_and_autocomplete(
        &schema,
        &instance,
        Separators {
            release: Some('?'),
            ..Separators::default()
        },
        Some("00401"),
        Autocomplete {
            current_datetime: "2026-10-09T10:30:00Z",
            request_acknowledgement: false,
            transaction_set: Some("940"),
        },
    );
    retain("autocomplete-valid-4010", &schema, &instance, &result);
    let output = result.unwrap();
    assert_eq!(output.as_bytes(), EXPECTED_AUTOCOMPLETE.as_bytes());
    assert_physical_isa(&output, b'*', b':', b'~');
}
