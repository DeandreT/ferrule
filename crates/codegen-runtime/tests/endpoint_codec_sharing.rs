//! Shared JSON5, XML and X12 endpoint preparations return every result and
//! error of the call that prepared them, in their original order, and keep
//! each concurrent call's accounting separate.

use std::fmt::Debug;

use codegen_runtime::{
    Instance, Json5BoundaryError, Json5BoundaryResource, MAX_EMBEDDED_X12_SCHEMA_BYTES,
    MAX_EMBEDDED_XML_SCHEMA_BYTES, MAX_JSON_DOCUMENT_BYTES, MAX_X12_DOCUMENT_BYTES,
    MAX_XML_DOCUMENT_BYTES, RuntimeError, Value, map_json5_bytes_with, map_json5_with,
    parse_structured_xml, parse_structured_xml_bytes, parse_x12, parse_x12_bytes, parse_xml,
    serialize_x12, serialize_x12_bytes, serialize_xml, serialize_xml_document,
};
use ir::{ScalarType, SchemaNode};

type Call = Box<dyn Fn() -> String + Send + Sync>;

fn outcome<T: Debug, E: Debug>(result: Result<T, E>) -> String {
    format!("{result:?}")
}

fn unique(name: &str, tag: &str) -> SchemaNode {
    // A distinct optional field keeps each test's descriptors out of the
    // preparations other tests in this process have already shared.
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
            SchemaNode::scalar(format!("Tag{tag}"), ScalarType::String),
        ],
    )
}

/// The first call of each case prepares its descriptors; later and concurrent
/// calls must reproduce its outcome exactly.
fn assert_shared(cases: Vec<(&'static str, Call)>) {
    let expected: Vec<String> = cases.iter().map(|(_, call)| call()).collect();
    for ((name, call), expected) in cases.iter().zip(&expected) {
        assert_eq!(&call(), expected, "{name}");
    }
    std::thread::scope(|scope| {
        for offset in 0..8 {
            let cases = &cases;
            let expected = &expected;
            scope.spawn(move || {
                for round in 0..4 {
                    for index in 0..cases.len() {
                        let position = (index + offset * 3 + round) % cases.len();
                        let (name, call) = &cases[position];
                        assert_eq!(&call(), &expected[position], "{name}");
                    }
                }
            });
        }
    });
}

#[test]
fn json5_preparations_preserve_results_errors_and_order() {
    let Ok(source) = codegen_schema::json5_profile::encode_schema(&unique("Source", "json5"))
    else {
        panic!("JSON5 source descriptor encodes");
    };
    let Ok(target) = codegen_schema::json5_profile::encode_schema(&unique("Target", "json5"))
    else {
        panic!("JSON5 target descriptor encodes");
    };
    let invalid = String::from(r#"{"name":"Root","kind":{"kind":"scalar","ty":"int"}}"#);
    let identity = |input: &Instance| Ok(input.clone());
    let case = |name: &'static str, call: Call| (name, call);
    let (s, t) = (source.clone(), target.clone());
    let (s2, t2) = (source.clone(), target.clone());
    let (s3, t3) = (source.clone(), target.clone());
    let (s4, t4) = (source.clone(), target.clone());
    let (s5, t5) = (source.clone(), target.clone());
    let (s6, i6) = (source.clone(), invalid.clone());
    let (i7, t7) = (invalid.clone(), target.clone());
    let (s8, t8) = (source.clone(), target.clone());
    assert_shared(vec![
        case(
            "text",
            Box::new(move || outcome(map_json5_with(&s, &t, "{Code: 'a', Count: 0x10}", identity))),
        ),
        case(
            "bytes",
            Box::new(move || {
                outcome(map_json5_bytes_with(
                    &s2,
                    &t2,
                    b"{Code: 'a', /* c */ }",
                    identity,
                ))
            }),
        ),
        case(
            "syntax before mapping",
            Box::new(move || outcome(map_json5_with(&s3, &t3, "{Code: 'a',,}", identity))),
        ),
        case(
            "input shape",
            Box::new(move || outcome(map_json5_with(&s4, &t4, "{Code: 1}", identity))),
        ),
        case(
            "mapping failure and output shape",
            Box::new(move || {
                let failed = map_json5_with(&s5, &t5, "{}", |_| {
                    Err(RuntimeError::XmlSerialization {
                        node: 3,
                        message: "mapping failed".into(),
                    })
                });
                let shape = map_json5_with(&s5, &t5, "{}", |_| {
                    Ok(Instance::Scalar(Value::String("x".into())))
                });
                format!("{} {}", outcome(failed), outcome(shape))
            }),
        ),
        case(
            "invalid target after source",
            Box::new(move || outcome(map_json5_with(&s6, &i6, "{}", identity))),
        ),
        case(
            "invalid source first",
            Box::new(move || outcome(map_json5_with(&i7, &t7, "{", identity))),
        ),
        case(
            "encoding before schemas",
            Box::new(move || {
                format!(
                    "{} {}",
                    outcome(map_json5_bytes_with("{", &t8, &[0xFF], identity)),
                    outcome(map_json5_bytes_with(&s8, "{", &[0xFF], identity))
                )
            }),
        ),
    ]);
}

/// The output limit applies to the measured text before it is written: an
/// output of exactly the limit is returned complete, one more byte is refused
/// with its requested size, on both routes.
#[test]
fn json5_output_limit_applies_to_measured_text() {
    let Ok(descriptor) = codegen_schema::json5_profile::encode_schema(&unique("Limit", "json5"))
    else {
        panic!("JSON5 descriptor encodes");
    };
    let output = |length: usize| {
        move |_: &Instance| {
            Ok(Instance::Group(
                vec![(
                    "Code".into(),
                    Instance::Scalar(Value::String("x".repeat(length))),
                )]
                .into(),
            ))
        }
    };
    let Ok(empty) = map_json5_with(&descriptor, &descriptor, "{}", output(0)) else {
        panic!("empty output maps");
    };
    let exact = MAX_JSON_DOCUMENT_BYTES - empty.len();
    let text = map_json5_with(&descriptor, &descriptor, "{}", output(exact));
    assert!(
        matches!(&text, Ok(text) if text.len() == MAX_JSON_DOCUMENT_BYTES && text.ends_with("\"\n}\n"))
    );
    drop(text);
    let bytes = map_json5_bytes_with(&descriptor, &descriptor, b"{}", output(exact));
    assert!(matches!(&bytes, Ok(bytes) if bytes.len() == MAX_JSON_DOCUMENT_BYTES));
    drop(bytes);
    let refused = |result: Result<usize, Json5BoundaryError>| {
        matches!(result, Err(Json5BoundaryError::Limit {
            resource: Json5BoundaryResource::OutputDocumentBytes,
            requested,
            max,
        }) if requested == MAX_JSON_DOCUMENT_BYTES + 1 && max == MAX_JSON_DOCUMENT_BYTES)
    };
    assert!(refused(
        map_json5_with(&descriptor, &descriptor, "{}", output(exact + 1)).map(|text| text.len())
    ));
    assert!(refused(
        map_json5_bytes_with(&descriptor, &descriptor, b"{}", output(exact + 1))
            .map(|bytes| bytes.len())
    ));
}

#[test]
fn xml_preparations_preserve_results_errors_and_order() {
    let schema = unique("Order", "xml");
    let Ok(descriptor) = codegen_schema::encode(&schema, MAX_EMBEDDED_XML_SCHEMA_BYTES) else {
        panic!("XML descriptor encodes");
    };
    let mut repeating = unique("Order", "repeating").repeating();
    repeating.name = "Orders".into();
    let Ok(repeating) = codegen_schema::encode(&repeating, MAX_EMBEDDED_XML_SCHEMA_BYTES) else {
        panic!("repeating XML descriptor encodes");
    };
    let instance = Instance::Group(
        vec![
            ("Code".into(), Instance::Scalar(Value::String("A&B".into()))),
            ("Count".into(), Instance::Scalar(Value::Int(3))),
        ]
        .into(),
    );
    let invalid = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(Value::String("\u{1}".into())),
        )]
        .into(),
    );
    let oversized = vec![b' '; MAX_XML_DOCUMENT_BYTES + 1];
    let d = descriptor.clone();
    let i = instance.clone();
    let document = move || {
        let xml = serialize_xml_document(&d, &i, true, true, None, None);
        let namespaced = serialize_xml_document(&d, &i, false, false, Some("urn:o"), None);
        let reserved = serialize_xml_document(
            &d,
            &i,
            false,
            false,
            Some("http://www.w3.org/2000/xmlns/"),
            None,
        );
        let hints = serialize_xml_document(&d, &i, false, false, None, Some("{}"));
        let empty = serialize_xml_document(&d, &i, false, false, Some(""), None);
        format!(
            "{} {} {} {} {}",
            outcome(xml),
            outcome(namespaced),
            outcome(reserved),
            outcome(hints),
            outcome(empty)
        )
    };
    let (d2, invalid2) = (descriptor.clone(), invalid.clone());
    let (d3, i3) = (descriptor.clone(), instance.clone());
    let d4 = descriptor.clone();
    let r5 = repeating.clone();
    let i5 = instance.clone();
    assert_shared(vec![
        ("document", Box::new(document)),
        (
            "document output errors",
            Box::new(move || {
                format!(
                    "{} {}",
                    outcome(serialize_xml_document(
                        &d2, &invalid2, false, false, None, None
                    )),
                    outcome(serialize_xml(7, &d2, &invalid2, false, false, None)),
                )
            }),
        ),
        (
            "node",
            Box::new(move || outcome(serialize_xml(7, &d3, &i3, false, true, Some("urn:o")))),
        ),
        (
            "structured input",
            Box::new(move || {
                format!(
                    "{} {} {} {} {}",
                    outcome(parse_structured_xml(
                        &d4,
                        "<Order><Code>A</Code><Count>3</Count></Order>"
                    )),
                    outcome(parse_structured_xml_bytes(
                        &d4,
                        b"<Order><Count>x</Count></Order>"
                    )),
                    outcome(parse_structured_xml(&d4, "<Other/>")),
                    outcome(parse_structured_xml_bytes(&d4, &[0xFF])),
                    outcome(parse_structured_xml_bytes("{", &oversized)),
                )
            }),
        ),
        (
            "schema refusals",
            Box::new(move || {
                format!(
                    "{} {} {} {}",
                    outcome(serialize_xml_document(&r5, &i5, false, false, None, None)),
                    outcome(parse_xml(&r5, "<Orders/>", true, true)),
                    outcome(parse_xml(&r5, "<Orders/>", true, false)),
                    outcome(serialize_xml(7, "{", &i5, false, false, None)),
                )
            }),
        ),
    ]);
}

fn fields(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| SchemaNode::scalar(format!("E{index:02}"), ScalarType::String))
            .collect(),
    )
}

const WIRE: &str = "ISA*00*          *00*          *ZZ*ALPHA          *ZZ*BETA           *300615*0945*U*00401*000000123*0*T*:~\nGS*PO*ALPHA*BETA*20300615*0945*7*X*004010~\nST*850*0007~\nNTE*one~\nNTE*two~\nSE*4*0007~\nGE*1*7~\nIEA*1*000000123~\n";

#[test]
fn x12_preparations_preserve_results_errors_and_order() {
    let schema = SchemaNode::group(
        "Shared",
        vec![
            fields("ISA", 16),
            fields("GS", 8),
            fields("ST", 2),
            fields("NTE", 1).repeating(),
            fields("SE", 2),
            fields("GE", 2),
            fields("IEA", 2),
        ],
    );
    let Ok(schema) = codegen_schema::encode(&schema, MAX_EMBEDDED_X12_SCHEMA_BYTES) else {
        panic!("X12 schema encodes");
    };
    let descriptor = serde_json::json!({
        "schema": schema, "version": "004010", "separators": null, "lenient_segments": false,
        "constraints": [], "implied_decimals": [], "lexical_formats": [], "autocomplete": null,
    })
    .to_string();
    let Ok(instance) = parse_x12(&descriptor, WIRE) else {
        panic!("X12 fixture parses");
    };
    let oversized = vec![b'~'; MAX_X12_DOCUMENT_BYTES + 1];
    let (d, d2, d3) = (descriptor.clone(), descriptor.clone(), descriptor.clone());
    let (i2, i3) = (instance.clone(), instance);
    assert_shared(vec![
        (
            "parse",
            Box::new(move || {
                format!(
                    "{} {} {} {} {}",
                    outcome(parse_x12(&d, WIRE)),
                    outcome(parse_x12_bytes(&d, WIRE.as_bytes())),
                    outcome(parse_x12(&d, "ISA*")),
                    outcome(parse_x12(&d, &WIRE.replace("SE*4*", "SE*5*"))),
                    outcome(parse_x12_bytes(&d, &[0xFF])),
                )
            }),
        ),
        (
            "serialize",
            Box::new(move || {
                let invalid =
                    Instance::Group(vec![("ISA".into(), Instance::Group(vec![].into()))].into());
                format!(
                    "{} {} {}",
                    outcome(serialize_x12(&d2, &i2, None)),
                    outcome(serialize_x12_bytes(&d2, &i2, None)),
                    outcome(serialize_x12(&d2, &invalid, None)),
                )
            }),
        ),
        (
            "descriptor refusals",
            Box::new(move || {
                format!(
                    "{} {} {}",
                    outcome(parse_x12(r#"{"version":"004010"}"#, WIRE)),
                    outcome(serialize_x12("{", &i3, None)),
                    outcome(parse_x12_bytes("{", &oversized)),
                )
            }),
        ),
        (
            "parse after refusals",
            Box::new(move || outcome(parse_x12(&d3, WIRE))),
        ),
    ]);
}
