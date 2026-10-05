use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

fn compact_options() -> XmlWriteOptions {
    XmlWriteOptions {
        declaration: false,
        indent: false,
        ..XmlWriteOptions::default()
    }
}

fn code_schema() -> SchemaNode {
    SchemaNode::group("Root", vec![SchemaNode::scalar("Code", ScalarType::String)])
}

fn code_instance(value: &str) -> Instance {
    Instance::Group(vec![("Code".into(), Instance::Scalar(Value::String(value.into())))].into())
}

#[test]
fn native_writer_rejects_first_forbidden_character_at_exact_utf8_offset() {
    let schema = code_schema();
    for character in [
        '\0', '\u{1}', '\u{B}', '\u{C}', '\u{1F}', '\u{FFFE}', '\u{FFFF}',
    ] {
        let text = format!("雪😀{character}\u{1}");
        let error =
            to_string_with_options(&schema, &code_instance(&text), &compact_options()).unwrap_err();
        assert!(
            matches!(error, XmlFormatError::InvalidXmlCharacter { codepoint, byte_offset }
            if codepoint == character as u32 && byte_offset == "<Root><Code>雪😀".len())
        );
    }
}

#[test]
fn legal_text_attribute_and_xml_character_endpoints_preserve_exact_bytes() {
    let text = "  😀\t\n\r<&  ";
    let schema = code_schema();
    let xml = to_string_with_options(&schema, &code_instance(text), &compact_options()).unwrap();
    assert_eq!(xml, "<Root><Code>  😀\t\n&#xD;&lt;&amp;  </Code></Root>");
    assert_eq!(from_str(&xml, &schema).unwrap(), code_instance(text));

    let attribute = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Code", ScalarType::String).attribute()],
    );
    let xml = to_string_with_options(&attribute, &code_instance(text), &compact_options()).unwrap();
    assert_eq!(xml, "<Root Code=\"  😀&#x9;&#xA;&#xD;&lt;&amp;  \"/>");
    assert_eq!(from_str(&xml, &attribute).unwrap(), code_instance(text));

    let endpoints = "\u{20}\u{7F}\u{85}\u{D7FF}\u{E000}\u{FDD0}\u{FFFD}\u{10000}\u{10FFFF}&#x1;";
    let xml =
        to_string_with_options(&schema, &code_instance(endpoints), &compact_options()).unwrap();
    assert_eq!(
        xml,
        "<Root><Code>\u{20}\u{7F}\u{85}\u{D7FF}\u{E000}\u{FDD0}\u{FFFD}\u{10000}\u{10FFFF}&amp;#x1;</Code></Root>"
    );
    assert_eq!(from_str(&xml, &schema).unwrap(), code_instance(endpoints));
}

#[test]
fn initial_native_shape_and_hint_failures_precede_body_character_validation() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    );
    let invalid = Instance::Group(
        vec![
            (
                "Code".into(),
                Instance::Scalar(Value::String("\u{1}".into())),
            ),
            ("Count".into(), Instance::Group(Vec::new().into())),
        ]
        .into(),
    );
    assert!(
        matches!(to_string_with_options(&schema, &invalid, &compact_options()),
        Err(XmlFormatError::Shape { name, .. }) if name == "Count")
    );
    let mut options = compact_options();
    options.schema_hints = Some(ir::XmlSchemaHints::default());
    assert!(matches!(
        to_string_with_options(&schema, &invalid, &options),
        Err(XmlFormatError::InvalidSchemaHints(
            ir::XmlSchemaHintsError::Empty
        ))
    ));
    let called = Cell::new(false);
    let result = to_string_with_options_and_finalizer::<&str, _>(
        &schema,
        &invalid,
        &compact_options(),
        |xml, _preview| {
            called.set(true);
            Ok(xml)
        },
    );
    assert!(
        matches!(result, Err(XmlWriteFinalizationError::Format(XmlFormatError::Shape { name, .. })) if name == "Count")
    );
    assert!(!called.get());
}

#[test]
fn finalizer_cannot_return_new_illegal_characters_without_a_native_refusal() {
    let result = to_string_with_options_and_finalizer::<&str, _>(
        &code_schema(),
        &code_instance("legal"),
        &compact_options(),
        |mut xml, _preview| {
            xml.push('\u{1}');
            Ok(xml)
        },
    );
    assert!(
        matches!(result, Err(XmlWriteFinalizationError::Format(XmlFormatError::InvalidXmlCharacter { codepoint: 1, byte_offset }))
        if byte_offset == "<Root><Code>legal</Code></Root>".len())
    );
    let xml = to_string_with_options_and_finalizer::<&str, _>(
        &code_schema(),
        &code_instance("legal"),
        &compact_options(),
        |xml, _preview| Ok(xml),
    )
    .unwrap();
    assert_eq!(xml, "<Root><Code>legal</Code></Root>");
}

#[test]
fn single_use_preview_keeps_native_body_scan_after_the_original_policy() {
    let schema = code_schema();
    let instance = code_instance("\u{1}");
    let mut options = compact_options();
    options.schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("literal.xsd".into()),
        locations: Vec::new(),
    });
    let called = Cell::new(0);
    let original = String::from("borrowed policy payload");
    let result = to_string_with_options_and_finalizer::<&str, _>(
        &schema,
        &instance,
        &options,
        |_xml, preview| {
            assert!(preview.check_unhinted_root_header(|preview| {
                called.set(called.get() + 1);
                assert_eq!(preview, "<Root><Code>\u{1}</Code></Root>");
                Ok::<bool, &str>(true)
            })?);
            Err(XmlWriteFinalizationError::Policy(original.as_str()))
        },
    );
    assert!(
        matches!(result, Err(XmlWriteFinalizationError::Policy(error)) if std::ptr::eq(error.as_ptr(), original.as_ptr()) && error == original.as_str())
    );
    assert_eq!(called.get(), 1);
}

#[test]
fn preview_predicate_error_retains_a_borrowed_non_error_policy_value() {
    let marker = String::from("preview policy");
    let result = to_string_with_options_and_finalizer::<&str, _>(
        &code_schema(),
        &code_instance("\u{1}"),
        &compact_options(),
        |_xml, preview| {
            preview.check_unhinted_root_header(|_text| Err::<bool, &str>(marker.as_str()))?;
            unreachable!("the original preview policy aborts finalization")
        },
    );
    assert!(
        matches!(result, Err(XmlWriteFinalizationError::Policy(error)) if std::ptr::eq(error.as_ptr(), marker.as_ptr()) && error == marker.as_str())
    );
}

#[test]
fn original_native_control_character_document_keeps_the_exact_negative_bytes() {
    let mut adjustment = SchemaNode::scalar("Adjustment", ScalarType::Float);
    adjustment.nillable = true;
    let schema = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::scalar("Id", ScalarType::Int).attribute(),
            SchemaNode::scalar("Marker", ScalarType::String),
            SchemaNode::group(
                "Line",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String).attribute(),
                    SchemaNode::scalar("DriverId", ScalarType::Int),
                    SchemaNode::scalar("Amount", ScalarType::Float),
                    SchemaNode::scalar("Scaled", ScalarType::Float),
                    adjustment,
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let line = |name: &str, id, amount, scaled, adjustment, text: &str| {
        Instance::Group(
            vec![
                ("Name".into(), Instance::Scalar(Value::String(name.into()))),
                ("DriverId".into(), Instance::Scalar(Value::Int(id))),
                ("Amount".into(), Instance::Scalar(Value::Float(amount))),
                ("Scaled".into(), Instance::Scalar(Value::Float(scaled))),
                ("Adjustment".into(), Instance::Scalar(adjustment)),
                ("Text".into(), Instance::Scalar(Value::String(text.into()))),
            ]
            .into(),
        )
    };
    let instance = Instance::Group(
        vec![
            ("Id".into(), Instance::Scalar(Value::Int(10))),
            (
                "Marker".into(),
                Instance::Scalar(Value::String("\u{1}".into())),
            ),
            (
                "Line".into(),
                Instance::Repeated(vec![
                    line("a & one", 101, -0.0, -0.0, Value::xml_nil(), "  雪 😀  "),
                    line(
                        "b \"two\"",
                        102,
                        9007199254740992.0,
                        4503599627370496.0,
                        Value::Float(1.25),
                        "",
                    ),
                ]),
            ),
        ]
        .into(),
    );
    let options = XmlWriteOptions {
        default_namespace: Some("urn:document".into()),
        schema_hints: Some(ir::XmlSchemaHints {
            no_namespace_location: None,
            locations: vec![ir::XmlSchemaLocation {
                namespace: "urn:document".into(),
                location: "documents.xsd".into(),
            }],
        }),
        ..XmlWriteOptions::default()
    };
    let original = include_str!("fixtures/invalid_character_native_document.xml");
    assert_eq!(original.len(), 642);
    assert_eq!(
        render_with_options(&schema, &instance, &options).unwrap(),
        original
    );
    assert!(matches!(
        to_string_with_options(&schema, &instance, &options),
        Err(XmlFormatError::InvalidXmlCharacter {
            codepoint: 1,
            byte_offset: 189
        })
    ));
}

#[test]
fn native_file_writes_refuse_invalid_characters_before_touching_the_destination() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "ferrule_strict_xml_writer_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory)
        .expect("fresh evidence directory must not replace an existing path");
    let sentinel = directory.join("sentinel.xml");
    let absent = directory.join("absent.xml");
    std::fs::write(&sentinel, b"original destination bytes").unwrap();
    let schema = code_schema();
    let invalid = code_instance("\u{1}");
    for with_options in [false, true] {
        let result = if with_options {
            write_with_options(&sentinel, &schema, &invalid, &compact_options())
        } else {
            write(&sentinel, &schema, &invalid)
        };
        assert!(matches!(
            result,
            Err(XmlFormatError::InvalidXmlCharacter { codepoint: 1, .. })
        ));
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"original destination bytes"
        );
    }
    assert!(matches!(
        write(&absent, &schema, &invalid),
        Err(XmlFormatError::InvalidXmlCharacter { codepoint: 1, .. })
    ));
    assert!(!absent.exists());
    std::fs::remove_dir_all(&directory).unwrap();
}
