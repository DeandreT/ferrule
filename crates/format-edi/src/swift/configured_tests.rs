//! SWIFT MT output from saved target options. All messages are self-authored.

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    EdiImpliedDecimal, FormatOptions, SwiftCharset, SwiftFieldLayout, SwiftMessageLayout,
    SwiftMtDirection, SwiftMtLayout, SwiftMtMessageHeaders, SwiftMtOutputOptions, SwiftValueExpr,
};

use super::{SwiftWriteError, from_bytes, to_bytes_with_options, validate_output};
use crate::EdiFormatError;

const BASIC: &str = "F01BANKBEBBAXXX0000000001";
const INPUT_TAIL: &str = "BANKDEFFXXXXN";
const OUTPUT_TAIL: &str = "1200240101BANKBEBBAXXX22221234562401011201N";

fn capture(path: &[&str], min: u16, max: u16, charset: SwiftCharset) -> SwiftValueExpr {
    SwiftValueExpr::Capture {
        path: path.iter().map(|part| part.to_string()).collect(),
        min,
        max,
        charset,
    }
}

fn layout() -> SwiftMtLayout {
    SwiftMtLayout::new(vec![SwiftMessageLayout::new(
        "MT950",
        vec![
            SwiftFieldLayout::new(
                "20",
                vec!["MT950".into(), "20".into()],
                false,
                capture(&[], 1, 16, SwiftCharset::Text),
            ),
            SwiftFieldLayout::new(
                "61",
                vec!["MT950".into(), "61".into()],
                true,
                SwiftValueExpr::Sequence {
                    parts: vec![
                        capture(&["Date"], 6, 6, SwiftCharset::Numeric),
                        capture(&["Amount"], 1, 15, SwiftCharset::Decimal),
                    ],
                },
            ),
        ],
    )])
    .unwrap()
}

fn schema() -> SchemaNode {
    let mut lines = SchemaNode::group(
        "61",
        vec![
            SchemaNode::scalar("Date", ScalarType::String),
            SchemaNode::scalar("Amount", ScalarType::Float),
        ],
    );
    lines.repeating = true;
    let mut message = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "Application Header",
                vec![
                    SchemaNode::group("Input", Vec::new()),
                    SchemaNode::group("Output", Vec::new()),
                ],
            ),
            SchemaNode::group(
                "MT950",
                vec![SchemaNode::scalar("20", ScalarType::String), lines],
            ),
        ],
    );
    message.repeating = true;
    SchemaNode::group("SWIFT", vec![message])
}

fn output(direction: SwiftMtDirection, tails: &[(&str, &str)]) -> SwiftMtOutputOptions {
    SwiftMtOutputOptions::new(
        direction,
        tails
            .iter()
            .map(|(basic, tail)| SwiftMtMessageHeaders {
                basic_header: basic.to_string(),
                application_header: tail.to_string(),
            })
            .collect(),
    )
    .unwrap()
}

fn options(direction: SwiftMtDirection) -> FormatOptions {
    let tail = match direction {
        SwiftMtDirection::Input => INPUT_TAIL,
        SwiftMtDirection::Output => OUTPUT_TAIL,
    };
    FormatOptions {
        swift_mt: Some(layout()),
        swift_mt_output: Some(output(direction, &[(BASIC, tail)])),
        ..FormatOptions::default()
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn empty() -> Instance {
    group(Vec::new())
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn statement(amount: Value) -> Instance {
    group(vec![
        ("20", text("REF-1")),
        (
            "61",
            Instance::Repeated(vec![group(vec![
                ("Date", text("240101")),
                ("Amount", Instance::Scalar(amount)),
            ])]),
        ),
    ])
}

fn root(message: Instance) -> Instance {
    group(vec![("Message", Instance::Repeated(vec![message]))])
}

fn mapped(header: Option<Instance>, amount: Value) -> Instance {
    let mut fields = Vec::new();
    if let Some(header) = header {
        fields.push(("Application Header", header));
    }
    fields.push(("MT950", statement(amount)));
    root(group(fields))
}

fn read_back(bytes: &[u8], options: &FormatOptions) -> Instance {
    let mut instance = from_bytes(bytes, &schema(), options.swift_mt.as_ref().unwrap(), false)
        .expect("written output re-reads");
    crate::apply_implied_decimals(&mut instance, &options.edi_implied_decimals).unwrap();
    instance
}

fn header(direction: &str) -> Instance {
    group(vec![(direction, empty())])
}

#[test]
fn an_exactly_mapped_direction_re_reads_to_the_mapped_instance() {
    for (direction, name, letter) in [
        (SwiftMtDirection::Input, "Input", "{2:I950BANKDEFFXXXXN}"),
        (SwiftMtDirection::Output, "Output", "{2:O9501200240101"),
    ] {
        let options = options(direction);
        let instance = mapped(Some(header(name)), Value::Float(12.5));
        let bytes = to_bytes_with_options(&schema(), &instance, &options).unwrap();
        let written = String::from_utf8(bytes.clone()).unwrap();
        assert!(written.starts_with(&format!("{{1:{BASIC}}}")), "{written}");
        assert!(written.contains(letter), "{written}");
        assert!(written.contains(":61:24010112,5\r\n"), "{written}");
        assert_eq!(read_back(&bytes, &options), instance);
    }
}

#[test]
fn configuration_decides_an_absent_empty_or_doubled_direction() {
    let options = options(SwiftMtDirection::Output);
    let expected = mapped(Some(header("Output")), Value::Float(3.0));
    for header in [
        None,
        Some(empty()),
        Some(group(vec![("Input", empty()), ("Output", empty())])),
        Some(group(vec![("Output", empty()), ("Input", empty())])),
    ] {
        let instance = mapped(header, Value::Float(3.0));
        let bytes = to_bytes_with_options(&schema(), &instance, &options).unwrap();
        assert_eq!(read_back(&bytes, &options), expected);
    }
}

#[test]
fn a_mapped_direction_that_contradicts_the_configuration_is_refused() {
    let options = options(SwiftMtDirection::Input);
    let instance = mapped(Some(header("Output")), Value::Float(3.0));
    let error = to_bytes_with_options(&schema(), &instance, &options).unwrap_err();
    assert!(
        matches!(
            error,
            EdiFormatError::SwiftDirection {
                message: 1,
                configured: "Input",
                mapped: "Output",
            }
        ),
        "{error:?}"
    );
}

#[test]
fn malformed_headers_stay_with_the_writer() {
    let options = options(SwiftMtDirection::Input);
    let duplicated = Instance::Group(
        vec![
            ("Input".to_string(), empty()),
            ("Input".to_string(), empty()),
        ]
        .into(),
    );
    let error = to_bytes_with_options(
        &schema(),
        &mapped(Some(duplicated), Value::Float(1.0)),
        &options,
    )
    .unwrap_err();
    assert!(
        matches!(&error, EdiFormatError::SwiftWrite(inner) if matches!(**inner, SwiftWriteError::DuplicateField { .. })),
        "{error:?}"
    );
}

#[test]
fn validation_refuses_missing_and_unusable_configuration() {
    let configured = options(SwiftMtDirection::Input);
    assert!(validate_output(&schema(), &configured).is_ok());

    let missing_headers = FormatOptions {
        swift_mt_output: None,
        ..configured.clone()
    };
    assert!(matches!(
        validate_output(&schema(), &missing_headers),
        Err(EdiFormatError::SwiftOutputConfiguration(_))
    ));
    let missing_layout = FormatOptions {
        swift_mt: None,
        ..configured.clone()
    };
    assert!(matches!(
        validate_output(&schema(), &missing_layout),
        Err(EdiFormatError::SwiftOutputConfiguration(_))
    ));

    // An input tail is not an output tail, and vice versa.
    let wrong_tail = FormatOptions {
        swift_mt_output: Some(output(SwiftMtDirection::Output, &[(BASIC, INPUT_TAIL)])),
        ..configured.clone()
    };
    let error = validate_output(&schema(), &wrong_tail).unwrap_err();
    assert!(
        matches!(&error, EdiFormatError::SwiftWrite(inner) if matches!(**inner, SwiftWriteError::InvalidHeader { block: "output application header", .. })),
        "{error:?}"
    );

    let repeated = FormatOptions {
        swift_mt_output: Some(output(
            SwiftMtDirection::Input,
            &[(BASIC, INPUT_TAIL), (BASIC, INPUT_TAIL)],
        )),
        ..configured.clone()
    };
    let error = validate_output(&schema(), &repeated).unwrap_err();
    assert!(
        matches!(&error, EdiFormatError::SwiftWrite(inner) if matches!(**inner, SwiftWriteError::RepeatedHeader { message: 2, block: "basic header" })),
        "{error:?}"
    );

    let mut input_only = schema();
    if let ir::SchemaKind::Group { children, .. } = &mut input_only.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
    {
        children.retain(|child| child.name == "Input");
    }
    assert!(matches!(
        validate_output(&input_only, &options(SwiftMtDirection::Output)),
        Err(EdiFormatError::SwiftOutputConfiguration(_))
    ));
}

#[test]
fn every_message_needs_its_own_header_set() {
    let options = options(SwiftMtDirection::Input);
    let message = group(vec![("MT950", statement(Value::Float(1.0)))]);
    let instance = group(vec![(
        "Message",
        Instance::Repeated(vec![message.clone(), message]),
    )]);
    let error = to_bytes_with_options(&schema(), &instance, &options).unwrap_err();
    assert!(
        matches!(&error, EdiFormatError::SwiftWrite(inner) if matches!(**inner, SwiftWriteError::HeaderCount { messages: 2, headers: 1 })),
        "{error:?}"
    );
}

fn implied(places: u8) -> FormatOptions {
    FormatOptions {
        edi_implied_decimals: vec![
            EdiImpliedDecimal::new(
                vec![
                    "Message".into(),
                    "MT950".into(),
                    "61".into(),
                    "Amount".into(),
                ],
                places,
            )
            .unwrap(),
        ],
        ..options(SwiftMtDirection::Input)
    }
}

#[test]
fn implied_decimals_write_the_scaled_wire_value_and_re_read_exactly() {
    let options = implied(2);
    for (value, wire) in [
        (Value::Float(12.5), "1250,"),
        (Value::Float(0.07), "7,"),
        (Value::Float(0.0), "0,"),
        (Value::Float(1.234), "123,4"),
        (Value::Int(3), "300,"),
    ] {
        let instance = mapped(Some(header("Input")), value.clone());
        let bytes = to_bytes_with_options(&schema(), &instance, &options).unwrap();
        let written = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            written.contains(&format!(":61:240101{wire}\r\n")),
            "{written}"
        );
        let expected = match value {
            Value::Int(number) => Value::Float(number as f64),
            other => other,
        };
        assert_eq!(
            read_back(&bytes, &options),
            mapped(Some(header("Input")), expected)
        );
    }
}

#[test]
fn implied_decimals_that_cannot_re_read_exactly_are_refused() {
    let options = implied(3);
    let mut refused = 0;
    for numerator in 1..400 {
        let value = f64::from(numerator) / 7.0;
        let instance = mapped(Some(header("Input")), Value::Float(value));
        match to_bytes_with_options(&schema(), &instance, &options) {
            Ok(bytes) => assert_eq!(
                read_back(&bytes, &options),
                instance,
                "{value} must re-read exactly when accepted"
            ),
            Err(EdiFormatError::SwiftImpliedDecimalValue { places: 3, .. }) => refused += 1,
            // Scaling can exceed the capture's length range.
            Err(EdiFormatError::SwiftWrite(inner))
                if matches!(*inner, SwiftWriteError::Length { .. }) => {}
            Err(other) => panic!("unexpected refusal for {value}: {other:?}"),
        }
    }
    assert!(refused > 0, "some seventh fractions double-round");
    let negative = mapped(Some(header("Input")), Value::Float(-1.5));
    assert!(matches!(
        to_bytes_with_options(&schema(), &negative, &options),
        Err(EdiFormatError::SwiftImpliedDecimalValue { .. })
    ));
}

#[test]
fn implied_decimals_must_name_a_decimal_capture() {
    let mut options = implied(2);
    options.edi_implied_decimals = vec![
        EdiImpliedDecimal::new(
            vec!["Message".into(), "MT950".into(), "61".into(), "Date".into()],
            2,
        )
        .unwrap(),
    ];
    assert!(matches!(
        validate_output(&schema(), &options),
        Err(EdiFormatError::SwiftImpliedDecimalLayout { .. })
    ));
}

#[test]
fn decimal_shifts_are_exact_text_moves() {
    use super::configured::shift_decimal;
    assert_eq!(shift_decimal("12.5", 2).as_deref(), Some("1250"));
    assert_eq!(shift_decimal("0.0001", 2).as_deref(), Some("0.01"));
    assert_eq!(shift_decimal("000", 1).as_deref(), Some("0"));
    assert_eq!(shift_decimal("7,25", 1).as_deref(), Some("72.5"));
    assert_eq!(shift_decimal("-1", 1), None);
    assert_eq!(shift_decimal(".5", 1), None);
}

/// The test layout with `extra` appended to its only message, renamed to
/// `message_type`.
fn layout_with(message_type: &str, extra: Vec<SwiftFieldLayout>) -> FormatOptions {
    let mut fields = layout().messages()[0].fields().to_vec();
    fields.extend(extra);
    FormatOptions {
        swift_mt: Some(
            SwiftMtLayout::new(vec![SwiftMessageLayout::new(message_type, fields)]).unwrap(),
        ),
        ..options(SwiftMtDirection::Input)
    }
}

fn layout_refusal(options: &FormatOptions) -> SwiftWriteError {
    match validate_output(&schema(), options) {
        Err(EdiFormatError::SwiftWrite(refusal)) => *refusal,
        other => panic!("expected a writer refusal, got {other:?}"),
    }
}

#[test]
fn validation_applies_the_writers_layout_refusals_to_every_message() {
    // A constant-only non-repeating field cannot record its presence.
    let constant = SwiftFieldLayout::new(
        "15",
        vec!["MT950".into(), "15".into()],
        false,
        SwiftValueExpr::Literal { value: "X".into() },
    );
    assert!(matches!(
        layout_refusal(&layout_with("MT950", vec![constant])),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("no value captures")
    ));

    assert!(matches!(
        layout_refusal(&layout_with("MT95A", Vec::new())),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("three digits")
    ));

    let lowercase = SwiftFieldLayout::new(
        "2a",
        vec!["MT950".into(), "2a".into()],
        false,
        capture(&[], 1, 4, SwiftCharset::Text),
    );
    assert!(matches!(
        layout_refusal(&layout_with("MT950", vec![lowercase])),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("uppercase letters and digits")
    ));

    // A reused tag depends on which fields the mapping fills; see
    // `reused_tags_are_refused_only_when_a_later_field_is_filled`.
    assert!(validate_output(&schema(), &layout_with("MT950", vec![again()])).is_ok());

    // A second, never-written message type is checked too.
    let mut options = options(SwiftMtDirection::Input);
    let mut messages = layout().messages().to_vec();
    messages.push(SwiftMessageLayout::new(
        "MT9",
        layout().messages()[0].fields().to_vec(),
    ));
    options.swift_mt = Some(SwiftMtLayout::new(messages).unwrap());
    assert!(matches!(
        layout_refusal(&options),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("`MT9`")
    ));
}

#[test]
fn validation_requires_a_repeating_message_group() {
    let mut single = schema();
    if let ir::SchemaKind::Group { children, .. } = &mut single.kind {
        children[0].repeating = false;
    }
    let refusal = match validate_output(&single, &options(SwiftMtDirection::Input)) {
        Err(EdiFormatError::SwiftWrite(refusal)) => *refusal,
        other => panic!("expected a writer refusal, got {other:?}"),
    };
    assert!(
        matches!(&refusal, SwiftWriteError::UnsupportedSchema(message) if message.contains("repeating `Message`")),
        "{refusal:?}"
    );
}

#[test]
fn header_refusals_report_every_message() {
    let output = output(
        SwiftMtDirection::Output,
        &[
            (BASIC, OUTPUT_TAIL),
            (BASIC, OUTPUT_TAIL),
            ("F01SHORT", OUTPUT_TAIL),
        ],
    );
    let refusals = super::header_refusals(&output);
    assert!(
        matches!(
            refusals.as_slice(),
            [
                (
                    2,
                    SwiftWriteError::RepeatedHeader {
                        message: 2,
                        block: "basic header"
                    }
                ),
                (
                    3,
                    SwiftWriteError::InvalidHeader {
                        block: "basic header",
                        ..
                    }
                ),
            ]
        ),
        "{refusals:?}"
    );
    let references = output_tails(&["F01BANKBEBBAXXX0000000001", "F01BANKBEBBAXXX0000000002"]);
    assert!(matches!(
        super::header_refusals(&references).as_slice(),
        [(
            2,
            SwiftWriteError::RepeatedHeader {
                message: 2,
                block: "message input reference"
            }
        )]
    ));
}

/// Distinct basic headers sharing one output message input reference.
fn output_tails(basics: &[&str]) -> SwiftMtOutputOptions {
    let pairs = basics
        .iter()
        .map(|basic| (*basic, OUTPUT_TAIL))
        .collect::<Vec<_>>();
    output(SwiftMtDirection::Output, &pairs)
}

/// A second field reusing tag `20`, like `:86:` at line and statement level.
fn again() -> SwiftFieldLayout {
    SwiftFieldLayout::new(
        "20",
        vec!["MT950".into(), "Again".into()],
        false,
        capture(&[], 1, 4, SwiftCharset::Text),
    )
}

#[test]
fn reused_tags_are_refused_only_when_a_later_field_is_filled() {
    let options = layout_with("MT950", vec![again()]);
    let layout = options.swift_mt.as_ref().unwrap();
    let first = vec!["MT950".to_string(), "20".to_string()];
    let later = vec!["MT950".to_string(), "Again".to_string()];
    let check = |filled: &[&Vec<String>]| {
        super::validate_filled_fields(layout, |path| filled.iter().any(|filled| *filled == path))
    };
    assert!(check(&[]).is_ok());
    assert!(check(&[&first]).is_ok());
    for filled in [&[&first, &later][..], &[&later]] {
        let refusal = check(filled).unwrap_err();
        assert!(
            matches!(&refusal, EdiFormatError::SwiftWrite(inner) if matches!(&**inner, SwiftWriteError::ShadowedTag { tag, path } if tag == "20" && path == "MT950/Again")),
            "{refusal:?}"
        );
    }

    // The writer agrees: only a written later field is refused.
    let mut schema = schema();
    if let ir::SchemaKind::Group { children, .. } = &mut schema.kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[0].kind
        && let ir::SchemaKind::Group { children, .. } = &mut children[1].kind
    {
        children.push(SchemaNode::scalar("Again", ScalarType::String));
    }
    let message = |again: Option<&str>| {
        let mut fields = vec![("20", text("REF-1"))];
        fields.extend(again.map(|value| ("Again", text(value))));
        root(group(vec![
            ("Application Header", header("Input")),
            ("MT950", group(fields)),
        ]))
    };
    assert!(to_bytes_with_options(&schema, &message(None), &options).is_ok());
    let refusal = to_bytes_with_options(&schema, &message(Some("X")), &options).unwrap_err();
    assert!(
        matches!(&refusal, EdiFormatError::SwiftWrite(inner) if matches!(**inner, SwiftWriteError::ShadowedTag { .. })),
        "{refusal:?}"
    );
}
