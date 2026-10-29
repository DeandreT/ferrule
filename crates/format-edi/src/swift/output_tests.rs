//! Bounded SWIFT MT output: complete expected bytes, read-back comparisons
//! and every refusal. All messages are self-authored.

use ir::{Decimal96, Instance, ScalarType, ScalarTypeSet, SchemaNode, Value};
use mapping::{SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtLayout, SwiftValueExpr};

use super::output::{OutputLimits, to_bytes_with_limits};
use super::{
    MAX_FIELDS_PER_MESSAGE, MAX_MESSAGES, SwiftHeaders, SwiftWriteError, from_bytes, to_bytes,
    write,
};
use crate::MAX_RUNTIME_INPUT_BYTES;

const BASIC: &str = "F01BANKBEBBAXXX0000000000";
const INPUT_TAIL: &str = "BANKDEFFXXXXN";
const OUTPUT_TAIL: &str = "1200240101BANKBEBBAXXX22221234562401011201N";

/// Asserts that `$result` is the refusal `$pattern`, printing it otherwise.
macro_rules! assert_refused {
    ($result:expr, $pattern:pat $(if $guard:expr)? $(,)?) => {{
        let error = $result;
        assert!(matches!(&error, $pattern $(if $guard)?), "unexpected outcome: {error:?}");
    }};
}

/// Headers for the message at `sequence`: each has its own basic-header
/// sequence number and output message input reference.
fn headers_at(sequence: usize) -> SwiftHeaders {
    SwiftHeaders::new(sequenced(BASIC, sequence))
        .and_then(|headers| headers.with_input(INPUT_TAIL))
        .and_then(|headers| headers.with_output(sequenced(OUTPUT_TAIL, sequence)))
        .unwrap()
}

/// Rewrites the sequence numbers that [`headers_at`] varies.
fn sequenced(text: &str, sequence: usize) -> String {
    text.replace(
        "BANKBEBBAXXX0000000000",
        &format!("BANKBEBBAXXX0000{sequence:06}"),
    )
    .replace("2222123456", &format!("2222{:06}", 123_456 + sequence))
}

fn headers(count: usize) -> Vec<SwiftHeaders> {
    (0..count).map(headers_at).collect()
}

fn headers_for(instance: &Instance) -> Vec<SwiftHeaders> {
    let messages = instance.field("Message").and_then(Instance::as_repeated);
    headers(messages.map_or(0, <[Instance]>::len))
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn null() -> Instance {
    Instance::Scalar(Value::Null)
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    let fields = fields
        .into_iter()
        .map(|(name, value)| (name.to_string(), value));
    Instance::Group(fields.collect::<Vec<_>>().into())
}

fn empty() -> Instance {
    group(Vec::new())
}

fn message(direction: &str, message_type: &str, body: Instance) -> Instance {
    let header = group(vec![(direction, empty())]);
    group(vec![("Application Header", header), (message_type, body)])
}

fn root(messages: Vec<Instance>) -> Instance {
    group(vec![("Message", Instance::Repeated(messages))])
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

fn capture(parts: &[&str], min: u16, max: u16, charset: SwiftCharset) -> SwiftValueExpr {
    let path = path(parts);
    SwiftValueExpr::Capture {
        path,
        min,
        max,
        charset,
    }
}

fn x(parts: &[&str], max: u16) -> SwiftValueExpr {
    capture(parts, 1, max, SwiftCharset::Text)
}

fn literal(value: &str) -> SwiftValueExpr {
    let value = value.into();
    SwiftValueExpr::Literal { value }
}

fn codes(parts: &[&str], values: &[&str]) -> SwiftValueExpr {
    let values = path(values);
    let path = path(parts);
    SwiftValueExpr::EnumCapture { path, values }
}

fn sequence(parts: Vec<SwiftValueExpr>) -> SwiftValueExpr {
    SwiftValueExpr::Sequence { parts }
}

fn optional(value: SwiftValueExpr) -> SwiftValueExpr {
    let value = Box::new(value);
    SwiftValueExpr::Optional { value }
}

fn field(parts: &[&str], tag: &str, repeating: bool, value: SwiftValueExpr) -> SwiftFieldLayout {
    SwiftFieldLayout::new(tag, path(parts), repeating, value)
}

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn node(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children)
}

fn repeating(mut node: SchemaNode) -> SchemaNode {
    node.repeating = true;
    node
}

fn statement_layout(line_repeats: bool) -> SwiftMessageLayout {
    let line = sequence(vec![
        capture(&["Date"], 6, 6, SwiftCharset::Numeric),
        codes(&["Mark"], &["C", "D"]),
        capture(&["Amount"], 1, 15, SwiftCharset::Decimal),
        optional(sequence(vec![literal("\r\n"), x(&["Details"], 34)])),
    ]);
    SwiftMessageLayout::new(
        "MT950",
        vec![
            field(&["MT950", "20"], "20", false, x(&[], 16)),
            field(&["MT950", "61"], "61", line_repeats, line),
        ],
    )
}

fn statement_schema(extra_line_children: Vec<SchemaNode>, extra: Vec<SchemaNode>) -> SchemaNode {
    let amount = SchemaNode::scalar("Amount", ScalarType::Float);
    let mut line = vec![string("Date"), string("Mark"), amount, string("Details")];
    line.extend(extra_line_children);
    let mut children = vec![string("20"), repeating(node("61", line))];
    children.extend(extra);
    node("MT950", children)
}

fn payment_layout() -> SwiftMessageLayout {
    let account = || {
        optional(sequence(vec![
            literal("/"),
            x(&["Account"], 34),
            literal("\r\n"),
        ]))
    };
    let identifier = capture(&["Identifier"], 8, 11, SwiftCharset::Alphanumeric);
    let amount = sequence(vec![
        capture(&["Date"], 6, 6, SwiftCharset::Numeric),
        capture(&["Currency"], 3, 3, SwiftCharset::Alphabetic),
        capture(&["Amount"], 1, 15, SwiftCharset::Decimal),
    ]);
    let customer = |tag| ["MT103", "Ordering Customer", tag];
    SwiftMessageLayout::new(
        "MT103",
        vec![
            field(&["MT103", "20"], "20", false, x(&[], 16)),
            field(
                &["MT103", "23B"],
                "23B",
                false,
                capture(&[], 4, 4, SwiftCharset::Alphanumeric),
            ),
            field(&["MT103", "32A"], "32A", false, amount),
            field(
                &customer("50A"),
                "50A",
                false,
                sequence(vec![account(), identifier]),
            ),
            field(
                &customer("50K"),
                "50K",
                false,
                sequence(vec![account(), x(&["Name"], 140)]),
            ),
            field(
                &["MT103", "71A"],
                "71A",
                false,
                codes(&["Code"], &["BEN", "OUR", "SHA"]),
            ),
            field(&["MT103", "72"], "72", false, x(&[], 210)),
        ],
    )
}

fn payment_schema() -> SchemaNode {
    let amount = SchemaNode::scalar("Amount", ScalarType::Float);
    let customer = node(
        "Ordering Customer",
        vec![
            node("50A", vec![string("Account"), string("Identifier")]),
            node("50K", vec![string("Account"), string("Name")]),
        ],
    );
    node(
        "MT103",
        vec![
            string("20"),
            string("23B"),
            node("32A", vec![string("Date"), string("Currency"), amount]),
            customer,
            node("71A", vec![string("Code")]),
            string("72"),
        ],
    )
}

fn schema_with(message_types: Vec<SchemaNode>) -> SchemaNode {
    let header = node(
        "Application Header",
        vec![node("Input", Vec::new()), node("Output", Vec::new())],
    );
    let mut children = vec![header];
    children.extend(message_types);
    node("SWIFT", vec![repeating(node("Message", children))])
}

fn fixture() -> (SchemaNode, SwiftMtLayout) {
    let schema = schema_with(vec![statement_schema(vec![], vec![]), payment_schema()]);
    let layout = SwiftMtLayout::new(vec![statement_layout(true), payment_layout()]).unwrap();
    (schema, layout)
}

fn line(date: &str, mark: &str, amount: Value, details: Option<&str>) -> Instance {
    group(vec![
        ("Date", text(date)),
        ("Mark", text(mark)),
        ("Amount", scalar(amount)),
        ("Details", details.map_or_else(null, text)),
    ])
}

fn amount_line(amount: Value) -> Instance {
    line("240101", "C", amount, None)
}

fn details_line(details: &str) -> Instance {
    line("240101", "C", Value::Float(1.0), Some(details))
}

fn statement(reference: Instance, lines: Vec<Instance>) -> Instance {
    group(vec![("20", reference), ("61", Instance::Repeated(lines))])
}

fn canonical_statement() -> Instance {
    let first = line("240101", "D", Value::Float(12.5), Some("fees\r\n\r\nmore"));
    let second = line("240102", "C", Value::Float(3.0), None);
    statement(text("REF-1"), vec![first, second])
}

const STATEMENT_BYTES: &str = "{1:F01BANKBEBBAXXX0000000000}{2:I950BANKDEFFXXXXN}{4:\r\n\
:20:REF-1\r\n\
:61:240101D12,5\r\nfees\r\n\r\nmore\r\n\
:61:240102C3,\r\n\
-}";

fn canonical_payment() -> Instance {
    let amount = group(vec![
        ("Date", text("240315")),
        ("Currency", text("EUR")),
        ("Amount", scalar(Value::Float(1250.75))),
    ]);
    let customer = group(vec![
        (
            "50A",
            group(vec![("Account", null()), ("Identifier", null())]),
        ),
        (
            "50K",
            group(vec![
                ("Account", text("12345678")),
                ("Name", text("JANE DOE\r\n1 MAIN STREET")),
            ]),
        ),
    ]);
    group(vec![
        ("20", text("PAY-7")),
        ("23B", text("CRED")),
        ("32A", amount),
        ("Ordering Customer", customer),
        ("71A", group(vec![("Code", text("SHA"))])),
        ("72", null()),
    ])
}

const PAYMENT_BYTES: &str = "{1:F01BANKBEBBAXXX0000000000}\
{2:O1031200240101BANKBEBBAXXX22221234562401011201N}{4:\r\n\
:20:PAY-7\r\n\
:23B:CRED\r\n\
:32A:240315EUR1250,75\r\n\
:50K:/12345678\r\nJANE DOE\r\n1 MAIN STREET\r\n\
:71A:SHA\r\n\
-}";

fn write_bytes(schema: &SchemaNode, instance: &Instance, layout: &SwiftMtLayout) -> String {
    let bytes = to_bytes(schema, instance, layout, &headers_for(instance)).unwrap();
    String::from_utf8(bytes).unwrap()
}

fn read_back(output: &str, schema: &SchemaNode, layout: &SwiftMtLayout) -> Instance {
    from_bytes(output.as_bytes(), schema, layout, false).unwrap()
}

fn refusal(schema: &SchemaNode, instance: &Instance, layout: &SwiftMtLayout) -> SwiftWriteError {
    to_bytes(schema, instance, layout, &headers_for(instance)).unwrap_err()
}

fn statement_refusal(body: Instance) -> SwiftWriteError {
    let (schema, layout) = fixture();
    refusal(
        &schema,
        &root(vec![message("Input", "MT950", body)]),
        &layout,
    )
}

fn line_refusal(line: Instance) -> SwiftWriteError {
    statement_refusal(statement(text("REF-1"), vec![line]))
}

/// A single `MT999` message type built from the given fields and schema.
fn custom(fields: Vec<SwiftFieldLayout>, children: Vec<SchemaNode>) -> (SchemaNode, SwiftMtLayout) {
    let schema = schema_with(vec![node("MT999", children)]);
    let layout = SwiftMtLayout::new(vec![SwiftMessageLayout::new("MT999", fields)]).unwrap();
    (schema, layout)
}

fn custom_body(body: Vec<(&str, Instance)>) -> Instance {
    root(vec![message("Input", "MT999", group(body))])
}

fn custom_refusal(
    fields: Vec<SwiftFieldLayout>,
    children: Vec<SchemaNode>,
    body: Vec<(&str, Instance)>,
) -> SwiftWriteError {
    let (schema, layout) = custom(fields, children);
    refusal(&schema, &custom_body(body), &layout)
}

/// Removes a per-test directory on drop, including after a panic.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_swift_output_{}_{}",
            std::process::id(),
            crate::test_support::thread_file_label()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn writes_complete_bytes_that_read_back() {
    let (schema, layout) = fixture();
    for (direction, message_type, body, expected) in [
        ("Input", "MT950", canonical_statement(), STATEMENT_BYTES),
        // Output direction, a choice group and a code list.
        ("Output", "MT103", canonical_payment(), PAYMENT_BYTES),
    ] {
        let instance = root(vec![message(direction, message_type, body)]);
        let output = write_bytes(&schema, &instance, &layout);
        assert_eq!(output, expected);
        assert_eq!(read_back(&output, &schema, &layout), instance);
    }
}

#[test]
fn refuses_ambiguous_message_selection() {
    let (schema, layout) = fixture();
    let header = |directions: &[&str]| {
        let directions = directions.iter().map(|direction| (*direction, empty()));
        ("Application Header", group(directions.collect()))
    };
    let selection = |fields| match refusal(&schema, &root(vec![group(fields)]), &layout) {
        SwiftWriteError::Selection {
            message: 1,
            item,
            found,
        } => (item, found),
        other => panic!("unexpected outcome: {other:?}"),
    };
    let statement = || ("MT950", canonical_statement());
    let payment = ("MT103", canonical_payment());
    let types = "message type";
    let directions = "application header direction";

    assert_eq!(
        selection(vec![header(&["Input"]), statement(), payment]),
        (types, 2)
    );
    assert_eq!(selection(vec![header(&["Input"])]), (types, 0));
    assert_eq!(
        selection(vec![header(&["Input", "Output"]), statement()]),
        (directions, 2)
    );
    assert_eq!(selection(vec![statement()]), (directions, 0));
    assert_eq!(selection(vec![header(&[]), statement()]), (directions, 0));
    let populated = group(vec![("Input", group(vec![("Receiver", text("X"))]))]);
    assert_refused! {
        refusal(
            &schema,
            &root(vec![group(vec![
                ("Application Header", populated),
                statement()
            ])]),
            &layout
        ),
        SwiftWriteError::InstanceShape { expected: "an empty group", .. }
    }
}

#[test]
fn writes_message_streams_in_order_without_separators() {
    let (schema, layout) = fixture();
    let instance = root(vec![
        message("Output", "MT103", canonical_payment()),
        message("Input", "MT950", canonical_statement()),
        message("Input", "MT950", statement(text("REF-2"), Vec::new())),
    ]);

    let output = write_bytes(&schema, &instance, &layout);

    let empty_statement =
        "{1:F01BANKBEBBAXXX0000000002}{2:I950BANKDEFFXXXXN}{4:\r\n:20:REF-2\r\n-}";
    let statement = sequenced(STATEMENT_BYTES, 1);
    assert_eq!(
        output,
        format!("{PAYMENT_BYTES}{statement}{empty_statement}")
    );
    assert_eq!(read_back(&output, &schema, &layout), instance);
    assert_eq!(write_bytes(&schema, &instance, &layout), output);
}

#[test]
fn rewrites_the_reader_fixture_with_configured_headers() {
    let (schema, layout) = fixture();
    let original = "{1:F01BANK}{2:I950DEST}{3:{121:abc}}{4:\r\n:20:REF-1\r\n\
        :61:240101D12,5\r\nfees\r\n\r\nmore\r\n:61:240102C3,\r\n-}{5:{CHK:123}}";
    let read = read_back(original, &schema, &layout);

    let output = write_bytes(&schema, &read, &layout);

    assert_eq!(output, STATEMENT_BYTES);
    assert_eq!(read_back(&output, &schema, &layout), read);
}

#[test]
fn every_written_fixture_survives_a_second_round_trip() {
    let (schema, layout) = fixture();
    for (direction, message_type, body) in [
        ("Input", "MT950", canonical_statement()),
        ("Output", "MT950", canonical_statement()),
        ("Input", "MT103", canonical_payment()),
        ("Output", "MT103", canonical_payment()),
    ] {
        let instance = root(vec![message(direction, message_type, body)]);
        let first = write_bytes(&schema, &instance, &layout);
        let read = read_back(&first, &schema, &layout);
        assert_eq!(read, instance);
        assert_eq!(write_bytes(&schema, &read, &layout), first);
    }
}

#[test]
fn normalizes_numeric_and_absent_values_to_their_reader_form() {
    let (schema, layout) = fixture();
    let decimal = Value::Decimal(Box::new(Decimal96::parse_exact("1250.075").unwrap()));
    let lines = vec![
        line("240101", "C", Value::Int(3), Some("")),
        line("240102", "C", decimal, None),
        line("240103", "D", Value::String("0.5".into()), None),
        line("240104", "D", Value::String("7,25".into()), None),
        line("240105", "C", Value::Float(-0.0), None),
        line("240106", "C", Value::String("8".into()), None),
    ];
    let instance = root(vec![message(
        "Input",
        "MT950",
        statement(scalar(Value::Int(42)), lines),
    )]);

    let output = write_bytes(&schema, &instance, &layout);

    assert_eq!(
        output,
        "{1:F01BANKBEBBAXXX0000000000}{2:I950BANKDEFFXXXXN}{4:\r\n:20:42\r\n\
:61:240101C3,\r\n:61:240102C1250,075\r\n:61:240103D0,5\r\n\
:61:240104D7,25\r\n:61:240105C0,\r\n:61:240106C8,\r\n-}"
    );
    let expected_lines = [
        ("240101", "C", 3.0),
        ("240102", "C", 1250.075),
        ("240103", "D", 0.5),
        ("240104", "D", 7.25),
        ("240105", "C", 0.0),
        ("240106", "C", 8.0),
    ]
    .into_iter()
    .map(|(date, mark, amount)| line(date, mark, Value::Float(amount), None))
    .collect();
    let expected = statement(text("42"), expected_lines);
    assert_eq!(
        read_back(&output, &schema, &layout),
        root(vec![message("Input", "MT950", expected)])
    );
}

#[test]
fn absent_fields_and_empty_sequences_are_omitted() {
    let (schema, layout) = fixture();
    let body = group(vec![
        ("20", text("PAY-8")),
        ("72", text("")),
        ("Ordering Customer", empty()),
    ]);
    let instance = root(vec![message("Input", "MT103", body)]);

    let output = write_bytes(&schema, &instance, &layout);

    assert_eq!(
        output,
        "{1:F01BANKBEBBAXXX0000000000}{2:I103BANKDEFFXXXXN}{4:\r\n:20:PAY-8\r\n-}"
    );
    let read = read_back(&output, &schema, &layout);
    let payment = read
        .field("Message")
        .and_then(Instance::as_repeated)
        .and_then(|messages| messages[0].field("MT103"))
        .unwrap();
    assert_eq!(payment.field("72"), Some(&null()));
    let identifier = payment
        .field("Ordering Customer")
        .and_then(|customer| customer.field("50A"))
        .and_then(|option| option.field("Identifier"));
    assert_eq!(identifier, Some(&null()));
}

#[test]
fn repeated_constant_fields_keep_their_occurrence_count() {
    let (schema, layout) = custom(
        vec![
            field(&["MT999", "20"], "20", false, x(&[], 16)),
            field(&["MT999", "77"], "77", true, literal("FIXED")),
        ],
        vec![string("20"), repeating(node("77", Vec::new()))],
    );
    let instance = custom_body(vec![
        ("20", text("R")),
        ("77", Instance::Repeated(vec![empty(), empty()])),
    ]);

    let output = write_bytes(&schema, &instance, &layout);

    assert_eq!(
        output,
        "{1:F01BANKBEBBAXXX0000000000}{2:I999BANKDEFFXXXXN}{4:\r\n\
:20:R\r\n:77:FIXED\r\n:77:FIXED\r\n-}"
    );
    assert_eq!(read_back(&output, &schema, &layout), instance);
}

#[test]
fn default_limits_match_the_reader() {
    let limits = OutputLimits::DEFAULT;
    assert_eq!(limits.output_bytes, MAX_RUNTIME_INPUT_BYTES);
    assert_eq!(limits.messages, MAX_MESSAGES);
    assert_eq!(limits.fields_per_message, MAX_FIELDS_PER_MESSAGE);
}

#[test]
fn refuses_a_direction_without_configured_application_header() {
    let (schema, layout) = fixture();
    let instance = root(vec![message("Output", "MT950", canonical_statement())]);
    let input_only = [SwiftHeaders::new(BASIC)
        .and_then(|headers| headers.with_input(INPUT_TAIL))
        .unwrap()];

    assert_refused! {
        to_bytes(&schema, &instance, &layout, &input_only),
        Err(SwiftWriteError::MissingApplicationHeader("Output"))
    }
}

#[test]
fn refuses_documents_without_messages_or_a_message_schema() {
    let (schema, layout) = fixture();
    assert_refused! {
        refusal(&schema, &root(Vec::new()), &layout),
        SwiftWriteError::NoMessages
    }
    assert_refused! {
        refusal(&schema, &empty(), &layout),
        SwiftWriteError::NoMessages
    }
    let single = group(vec![(
        "Message",
        message("Input", "MT950", canonical_statement()),
    )]);
    assert_refused! {
        refusal(&schema, &single, &layout),
        SwiftWriteError::InstanceShape { expected: "repeating values", .. }
    }
    let unrepeated = node("SWIFT", vec![node("Message", Vec::new())]);
    assert_refused! {
        refusal(&unrepeated, &root(Vec::new()), &layout),
        SwiftWriteError::UnsupportedSchema(_)
    }
    let types = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap();
    let union = schema_with(vec![node(
        "MT950",
        vec![SchemaNode::scalar_union("20", types)],
    )]);
    assert_refused! {
        refusal(&union, &root(Vec::new()), &layout),
        SwiftWriteError::UnsupportedSchema(message) if message.contains("scalar union")
    }
}

#[test]
fn refuses_message_types_the_layout_cannot_name() {
    let schema = schema_with(vec![
        statement_schema(vec![], vec![]),
        node("MT999", Vec::new()),
    ]);
    let layout = SwiftMtLayout::new(vec![statement_layout(true)]).unwrap();
    assert_refused! {
        refusal(&schema, &custom_body(Vec::new()), &layout),
        SwiftWriteError::UnknownMessage(name) if name == "MT999"
    }

    let schema = schema_with(vec![node("MT95", Vec::new())]);
    let layout = SwiftMtLayout::new(vec![SwiftMessageLayout::new("MT95", Vec::new())]).unwrap();
    assert_refused! {
        refusal(&schema, &root(vec![message("Input", "MT95", empty())]), &layout),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("three digits")
    }
}

#[test]
fn refuses_tags_the_reader_cannot_recognize_or_reach() {
    assert_refused! {
        custom_refusal(
            vec![field(&["MT999", "2a"], "2a", false, x(&[], 16))],
            vec![string("2a")],
            vec![("2a", text("X"))],
        ),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("`2a`")
    }
    let shadowed = vec![
        field(&["MT999", "20"], "20", false, x(&[], 16)),
        field(&["MT999", "Again"], "20", false, x(&[], 16)),
    ];
    let children = vec![string("20"), string("Again")];
    assert_refused! {
        custom_refusal(shadowed.clone(), children.clone(), vec![("Again", text("X"))]),
        SwiftWriteError::ShadowedTag { tag, path } if tag == "20" && path == "MT999/Again"
    }
    let (schema, layout) = custom(shadowed, children);
    let first_only = custom_body(vec![("20", text("X"))]);
    assert!(to_bytes(&schema, &first_only, &layout, &headers(1)).is_ok());
}

#[test]
fn refuses_constants_whose_presence_the_instance_cannot_record() {
    assert_refused! {
        custom_refusal(
            vec![field(&["MT999", "77"], "77", false, literal("FIXED"))],
            vec![node("77", Vec::new())],
            Vec::new(),
        ),
        SwiftWriteError::UnsupportedLayout(message) if message.contains("no value captures")
    }
    let signed = sequence(vec![
        optional(literal("N")),
        capture(&[], 1, 15, SwiftCharset::Decimal),
    ]);
    assert_refused! {
        custom_refusal(
            vec![field(&["MT999", "19A"], "19A", false, signed)],
            vec![SchemaNode::scalar("19A", ScalarType::Float)],
            vec![("19A", scalar(Value::Float(5.0)))],
        ),
        SwiftWriteError::AmbiguousOptional { tag } if tag == "19A"
    }
    let marked = |mark| {
        sequence(vec![
            literal(mark),
            capture(&[], 1, 4, SwiftCharset::Numeric),
        ])
    };
    let choices = vec![marked("C"), marked("D")];
    assert_refused! {
        custom_refusal(
            vec![field(&["MT999", "23"], "23", false, SwiftValueExpr::Alternatives { choices })],
            vec![string("23")],
            vec![("23", text("12"))],
        ),
        SwiftWriteError::AmbiguousChoice { tag } if tag == "23"
    }
}

#[test]
fn refuses_grammars_that_re_read_differently() {
    let split = sequence(vec![x(&["A"], 10), optional(x(&["B"], 10))]);
    assert_refused! {
        custom_refusal(
            vec![field(&["MT999", "99"], "99", false, split)],
            vec![node("99", vec![string("A"), string("B")])],
            vec![("99", group(vec![("A", text("AB")), ("B", text("CD"))]))],
        ),
        SwiftWriteError::AmbiguousField { tag } if tag == "99"
    }
}

#[test]
fn refuses_instance_shapes_outside_the_schema() {
    let single_line = amount_line(Value::Float(1.0));
    assert_refused! {
        statement_refusal(group(vec![("20", text("REF-1")), ("61", single_line)])),
        SwiftWriteError::InstanceShape { expected: "repeating values", .. }
    }
    assert_refused! {
        statement_refusal(group(vec![("20", text("A")), ("Extra", text("B"))])),
        SwiftWriteError::UnexpectedField { field, .. } if field == "Extra"
    }
    assert_refused! {
        statement_refusal(group(vec![("20", text("A")), ("20", text("B"))])),
        SwiftWriteError::DuplicateField { field, .. } if field == "20"
    }
    assert_refused! {
        statement_refusal(group(vec![("20", empty())])),
        SwiftWriteError::InstanceShape { expected: "a scalar", .. }
    }
}

#[test]
fn refuses_values_no_layout_field_reads() {
    let schema = schema_with(vec![statement_schema(
        vec![string("Unused")],
        vec![string("Note"), repeating(node("Notes", Vec::new()))],
    )]);
    let layout = SwiftMtLayout::new(vec![statement_layout(true)]).unwrap();
    let instance = |body| root(vec![message("Input", "MT950", body)]);
    let attempt = |body| refusal(&schema, &instance(body), &layout);

    assert_refused! {
        attempt(group(vec![("Note", text("x"))])),
        SwiftWriteError::Unrepresentable { path } if path == "MT950/Note"
    }
    assert_refused! {
        attempt(group(vec![("Notes", Instance::Repeated(vec![empty()]))])),
        SwiftWriteError::Unrepresentable { path } if path == "MT950/Notes"
    }
    let mut unused = amount_line(Value::Float(1.0));
    if let Instance::Group(fields) = &mut unused {
        fields.push(("Unused".into(), text("x")));
    }
    assert_refused! {
        attempt(statement(text("R"), vec![unused])),
        SwiftWriteError::Unconsumed { tag, path } if tag == "61" && path == "MT950/61/Unused"
    }
    let absent = group(vec![
        ("20", text("R")),
        ("Note", null()),
        ("Notes", Instance::Repeated(Vec::new())),
    ]);
    assert!(to_bytes(&schema, &instance(absent), &layout, &headers(1)).is_ok());
}

#[test]
fn refuses_missing_required_values_and_wrong_types() {
    let mut missing_date = amount_line(Value::Float(1.0));
    if let Instance::Group(fields) = &mut missing_date {
        fields[0].1 = null();
    }
    assert_refused! {
        line_refusal(missing_date),
        SwiftWriteError::MissingValue { tag, path } if tag == "61" && path == "MT950/61/Date"
    }
    for value in [Value::Bool(true), Value::Float(1.5)] {
        assert_refused! {
            statement_refusal(statement(scalar(value), Vec::new())),
            SwiftWriteError::ValueType { expected: "a string", got, .. } if *got != "string"
        }
    }
    assert_refused! {
        line_refusal(amount_line(Value::Bool(false))),
        SwiftWriteError::ValueType { expected: "a number", got: "bool", .. }
    }
}

#[test]
fn refuses_decimals_outside_the_unsigned_comma_format() {
    for amount in [
        Value::Float(-1.0),
        Value::Float(f64::NAN),
        Value::Float(f64::INFINITY),
        Value::Int(-3),
        Value::Decimal(Box::new(Decimal96::parse_exact("-0.5").unwrap())),
        Value::String("-3".into()),
        Value::String("1e5".into()),
        Value::String(",5".into()),
        Value::String("1.2.3".into()),
    ] {
        assert_refused! {
            line_refusal(amount_line(amount)),
            SwiftWriteError::InvalidDecimal { tag, path, .. }
                if tag == "61" && path == "MT950/61/Amount"
        }
    }
}

#[test]
fn refuses_characters_outside_the_capture_sets() {
    for (reference, character) in [("REF@1", '@'), ("REF{1", '{'), ("REFÉ", 'É')] {
        assert_refused! {
            statement_refusal(statement(text(reference), Vec::new())),
            SwiftWriteError::InvalidCharacter { character: found, charset: "SWIFT X", .. }
                if *found == character
        }
    }
    assert_refused! {
        line_refusal(line("2401O1", "C", Value::Float(1.0), None)),
        SwiftWriteError::InvalidCharacter { character: 'O', charset: "numeric", .. }
    }
    let (schema, layout) = fixture();
    let mut payment = canonical_payment();
    if let Instance::Group(fields) = &mut payment {
        fields[2].1 = group(vec![
            ("Date", text("240315")),
            ("Currency", text("eur")),
            ("Amount", scalar(Value::Float(1.0))),
        ]);
    }
    assert_refused! {
        refusal(
            &schema,
            &root(vec![message("Input", "MT103", payment)]),
            &layout
        ),
        SwiftWriteError::InvalidCharacter { character: 'e', charset: "alphabetic", .. }
    }
    for details in ["fees\nmore", "fees\rmore", "fees\r"] {
        assert_refused! {
            line_refusal(details_line(details)),
            SwiftWriteError::BareLineBreak { path, .. } if path == "MT950/61/Details"
        }
    }
}

#[test]
fn refuses_lengths_and_codes_outside_the_layout() {
    assert_refused! {
        statement_refusal(statement(text("REFERENCE-TOO-LON"), Vec::new())),
        SwiftWriteError::Length { min: 1, max: 16, actual: 17, .. }
    }
    assert_refused! {
        line_refusal(line("24011", "C", Value::Float(1.0), None)),
        SwiftWriteError::Length { min: 6, max: 6, actual: 5, .. }
    }
    assert_refused! {
        line_refusal(amount_line(Value::Float(12_345_678_901_234.5))),
        SwiftWriteError::Length { min: 1, max: 15, actual: 16, .. }
    }
    assert_refused! {
        line_refusal(line("240101", "X", Value::Float(1.0), None)),
        SwiftWriteError::NotInCodeList { value, .. } if value == "X"
    }
}

#[test]
fn refuses_repetition_the_layout_does_not_allow() {
    let schema = schema_with(vec![statement_schema(vec![], vec![])]);
    let layout = SwiftMtLayout::new(vec![statement_layout(false)]).unwrap();
    let lines = vec![
        amount_line(Value::Float(1.0)),
        amount_line(Value::Float(2.0)),
    ];
    let instance = root(vec![message("Input", "MT950", statement(text("R"), lines))]);
    assert_refused! {
        refusal(&schema, &instance, &layout),
        SwiftWriteError::Repetition { tag, count: 2 } if tag == "61"
    }
}

#[test]
fn refuses_line_structures_the_text_block_cannot_carry() {
    for (details, first) in [("fees\r\n-more", '-'), ("fees\r\n:20:more", ':')] {
        assert_refused! {
            line_refusal(details_line(details)),
            SwiftWriteError::LineStart { line: 3, first: found, .. } if *found == first
        }
    }
    assert_refused! {
        line_refusal(details_line("fees\r\n")),
        SwiftWriteError::TrailingLineBreak { tag } if tag == "61"
    }
}

#[test]
fn enforces_output_limits() {
    let (schema, layout) = fixture();
    let two = root(vec![
        message("Input", "MT950", canonical_statement()),
        message("Input", "MT950", canonical_statement()),
    ]);
    let size = STATEMENT_BYTES.len() * 2;
    let steps = OutputLimits::DEFAULT.render_steps;
    // (messages, fields per message, output bytes, steps per field occurrence)
    let limited = |(messages, fields_per_message, output_bytes, render_steps)| {
        let limits = OutputLimits {
            output_bytes,
            messages,
            fields_per_message,
            render_steps,
        };
        to_bytes_with_limits(&schema, &two, &layout, &headers(2), limits)
    };
    let refused = |limits| match limited(limits) {
        Err(SwiftWriteError::Limit(limit)) => limit,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert_eq!(refused((1, 3, size, steps)), "message count");
    assert_eq!(refused((2, 2, size, steps)), "field count");
    assert_eq!(refused((2, 3, size - 1, steps)), "output size");
    assert_eq!(refused((2, 3, size, 3)), "render work");
    let expected = format!("{STATEMENT_BYTES}{}", sequenced(STATEMENT_BYTES, 1));
    assert_eq!(limited((2, 3, size, steps)).unwrap(), expected.into_bytes());
    // The work budget applies per field occurrence, not to the whole stream:
    // twelve steps cover one statement line but not the whole stream.
    assert_eq!(
        limited((2, 3, size, 12)).unwrap(),
        limited((2, 3, size, steps)).unwrap()
    );
}

#[test]
fn refused_writes_leave_the_destination_untouched() {
    let directory = TempDir::new();
    let destination = directory.0.join("statement.fin");
    std::fs::write(&destination, "previous").unwrap();
    let (schema, layout) = fixture();

    let invalid = root(vec![message(
        "Input",
        "MT950",
        statement(text("REF@1"), Vec::new()),
    )]);
    assert_refused! {
        write(&destination, &schema, &invalid, &layout, &headers(1)),
        Err(SwiftWriteError::InvalidCharacter { .. })
    }
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), "previous");

    let valid = root(vec![message("Input", "MT950", canonical_statement())]);
    write(&destination, &schema, &valid, &layout, &headers(1)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        STATEMENT_BYTES
    );
    assert_eq!(
        super::read(&destination, &schema, &layout, false).unwrap(),
        valid
    );

    let missing = directory.0.join("missing").join("statement.fin");
    assert_refused! {
        write(&missing, &schema, &valid, &layout, &headers(1)),
        Err(SwiftWriteError::Io(_))
    }
}

#[test]
fn refuses_occurrences_of_several_repeating_fields() {
    let (schema, layout) = custom(
        vec![
            field(&["MT999", "61"], "61", true, x(&[], 16)),
            field(&["MT999", "86"], "86", true, x(&[], 16)),
        ],
        vec![repeating(string("61")), repeating(string("86"))],
    );
    let lines =
        |values: &[&str]| Instance::Repeated(values.iter().map(|value| text(value)).collect());
    // `:61:A :86:X :61:B :86:Y` and `:61:A :61:B :86:X :86:Y` read identically.
    for (first, second) in [(&["A", "B"][..], &["X", "Y"][..]), (&["A"], &["X"])] {
        let body = custom_body(vec![("61", lines(first)), ("86", lines(second))]);
        assert_refused! {
            refusal(&schema, &body, &layout),
            SwiftWriteError::InterleavedRepetition { message: 1, tags } if tags == "61, 86"
        }
    }
    let single = custom_body(vec![("61", lines(&["A", "B"])), ("86", lines(&[]))]);
    assert_eq!(
        write_bytes(&schema, &single, &layout),
        "{1:F01BANKBEBBAXXX0000000000}{2:I999BANKDEFFXXXXN}{4:\r\n:61:A\r\n:61:B\r\n-}"
    );
}

#[test]
fn requires_distinct_headers_for_each_message() {
    let (schema, layout) = fixture();
    let stream = |direction| {
        root(vec![
            message(direction, "MT950", canonical_statement()),
            message(direction, "MT950", canonical_statement()),
        ])
    };
    assert_refused! {
        to_bytes(&schema, &stream("Output"), &layout, &headers(1)),
        Err(SwiftWriteError::HeaderCount { messages: 2, headers: 1 })
    }
    assert_refused! {
        to_bytes(&schema, &stream("Input"), &layout, &[headers_at(0), headers_at(0)]),
        Err(SwiftWriteError::RepeatedHeader { message: 2, block: "basic header" })
    }
    let same_reference = SwiftHeaders::new(sequenced(BASIC, 1))
        .and_then(|headers| headers.with_input(INPUT_TAIL))
        .and_then(|headers| headers.with_output(OUTPUT_TAIL))
        .unwrap();
    let reused = [headers_at(0), same_reference];
    assert_refused! {
        to_bytes(&schema, &stream("Output"), &layout, &reused),
        Err(SwiftWriteError::RepeatedHeader { message: 2, block: "message input reference" })
    }
    // Input messages do not write the output tail, so it is not compared.
    assert!(to_bytes(&schema, &stream("Input"), &layout, &reused).is_ok());
}

#[test]
fn refuses_present_root_values_other_than_messages() {
    let (mut schema, layout) = fixture();
    if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
        children.push(string("Batch"));
        children.push(node("Info", vec![string("Note")]));
    }
    let with = |name, value| {
        let messages = Instance::Repeated(vec![message("Input", "MT950", canonical_statement())]);
        group(vec![("Message", messages), (name, value)])
    };
    assert_refused! {
        refusal(&schema, &with("Batch", text("B1")), &layout),
        SwiftWriteError::Unrepresentable { path } if path == "SWIFT/Batch"
    }
    assert_refused! {
        refusal(&schema, &with("Info", group(vec![("Note", text("n"))])), &layout),
        SwiftWriteError::Unrepresentable { path } if path == "SWIFT/Info"
    }
    for absent in [
        with("Batch", null()),
        with("Info", group(vec![("Note", text(""))])),
    ] {
        assert_eq!(write_bytes(&schema, &absent, &layout), STATEMENT_BYTES);
    }
}

#[test]
fn ignores_empty_sibling_message_type_groups() {
    let (schema, layout) = fixture();
    let header = || group(vec![("Input", empty())]);
    let unselected = group(vec![("20", null()), ("Ordering Customer", empty())]);
    let with_sibling = group(vec![
        ("Application Header", header()),
        ("MT950", canonical_statement()),
        ("MT103", unselected),
    ]);
    assert_eq!(
        write_bytes(&schema, &root(vec![with_sibling]), &layout),
        STATEMENT_BYTES
    );
    let lone_empty = root(vec![message("Input", "MT950", empty())]);
    assert_eq!(
        write_bytes(&schema, &lone_empty, &layout),
        "{1:F01BANKBEBBAXXX0000000000}{2:I950BANKDEFFXXXXN}{4:\r\n-}"
    );
    let both_empty = group(vec![
        ("Application Header", header()),
        ("MT950", empty()),
        ("MT103", empty()),
    ]);
    assert_refused! {
        refusal(&schema, &root(vec![both_empty]), &layout),
        SwiftWriteError::Selection { item: "message type", found: 0, .. }
    }
}

#[test]
fn refuses_braces_and_block_ends_in_layout_constants() {
    let attempt = |constant| {
        custom_refusal(
            vec![field(
                &["MT999", "20"],
                "20",
                false,
                sequence(vec![x(&[], 16), literal(constant)]),
            )],
            vec![string("20")],
            vec![("20", text("REF"))],
        )
    };
    for (constant, character) in [("{", '{'), ("-}", '}'), ("}{4:", '}')] {
        assert_refused! {
            attempt(constant),
            SwiftWriteError::InvalidCharacter { character: found, charset: "SWIFT X", path, .. }
                if *found == character && path == "MT999/20"
        }
    }
    assert_refused! {
        attempt("\r\n-"),
        SwiftWriteError::LineStart { line: 2, first: '-', .. }
    }
}

#[test]
fn oversized_constants_fail_while_rendering() {
    let constant = "A".repeat(4096);
    let (schema, layout) = custom(
        vec![field(
            &["MT999", "20"],
            "20",
            false,
            sequence(vec![x(&[], 16), literal(&constant)]),
        )],
        vec![string("20")],
    );
    let instance = custom_body(vec![("20", text("REF"))]);
    let limits = OutputLimits {
        output_bytes: 200,
        ..OutputLimits::DEFAULT
    };
    assert_refused! {
        to_bytes_with_limits(&schema, &instance, &layout, &headers(1), limits),
        Err(SwiftWriteError::Limit("output size"))
    }
    assert!(
        write_bytes(&schema, &instance, &layout).ends_with(&format!(":20:REF{constant}\r\n-}}"))
    );
}

#[test]
fn decimal_spelling_is_never_rounded_and_negative_zero_is_unsigned() {
    assert_refused! {
        line_refusal(amount_line(Value::Float(0.1 + 0.2))),
        SwiftWriteError::Length { max: 15, actual: 19, .. }
    }
    // `Decimal96` stores zero unsigned, so `-0.00` cannot carry a sign.
    let negative_zero = Decimal96::parse_exact("-0.00").unwrap();
    assert!(!negative_zero.is_negative());
    let (schema, layout) = fixture();
    let line = amount_line(Value::Decimal(Box::new(negative_zero)));
    let instance = root(vec![message(
        "Input",
        "MT950",
        statement(text("R"), vec![line]),
    )]);

    let output = write_bytes(&schema, &instance, &layout);

    assert!(output.contains(":61:240101C0,\r\n"), "{output}");
    let expected = statement(text("R"), vec![amount_line(Value::Float(0.0))]);
    assert_eq!(
        read_back(&output, &schema, &layout),
        root(vec![message("Input", "MT950", expected)])
    );
}
