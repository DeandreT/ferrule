use super::*;
use crate::xsd;
use ir::{XML_ATTRIBUTES_FIELD, XML_LOCAL_NAME_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD};

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Tags",
                vec![
                    SchemaNode::group("Tag", vec![SchemaNode::scalar("Value", ScalarType::String)])
                        .repeating(),
                ],
            ),
        ],
    )
}

#[test]
fn write_then_read_roundtrips_nested_repeating_groups() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_format_xml_test_{}.xml",
        std::process::id()
    ));

    let instance = Instance::Group(vec![
        (
            "Name".into(),
            Instance::Scalar(Value::String("Jane".into())),
        ),
        (
            "Tags".into(),
            Instance::Group(vec![(
                "Tag".into(),
                Instance::Repeated(vec![
                    Instance::Group(vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String("a".into())),
                    )]),
                    Instance::Group(vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String("b".into())),
                    )]),
                ]),
            )]),
        ),
    ]);

    write(&path, &schema(), &instance).unwrap();
    let read_back = read(&path, &schema()).unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(read_back, instance);
}

#[test]
fn attributes_roundtrip_including_missing_optional_ones() {
    let schema = SchemaNode::group(
        "Books",
        vec![
            SchemaNode::scalar("count", ScalarType::Int).attribute(),
            SchemaNode::group(
                "Book",
                vec![
                    SchemaNode::scalar("isbn", ScalarType::String).attribute(),
                    SchemaNode::scalar("Title", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let instance = Instance::Group(vec![
        ("count".into(), Instance::Scalar(Value::Int(2))),
        (
            "Book".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    (
                        "isbn".into(),
                        Instance::Scalar(Value::String("978-1".into())),
                    ),
                    ("Title".into(), Instance::Scalar(Value::String("A".into()))),
                ]),
                Instance::Group(vec![
                    // Null attribute: omitted on write, read back as Null.
                    ("isbn".into(), Instance::Scalar(Value::Null)),
                    ("Title".into(), Instance::Scalar(Value::String("B".into()))),
                ]),
            ]),
        ),
    ]);

    let path = std::env::temp_dir().join(format!(
        "ferrule_format_xml_attr_test_{}.xml",
        std::process::id()
    ));
    write(&path, &schema, &instance).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#"<Books count="2">"#), "{text}");
    assert!(text.contains(r#"<Book isbn="978-1">"#), "{text}");

    let read_back = read(&path, &schema).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(read_back, instance);
}

#[test]
fn attribute_newlines_and_xml_metacharacters_roundtrip_exactly() {
    let schema = SchemaNode::group(
        "Book",
        vec![SchemaNode::scalar("Title", ScalarType::String).attribute()],
    );
    let title = "The Mystery of Edwin\nDrood & \"Others\"";
    let instance = Instance::Group(vec![(
        "Title".into(),
        Instance::Scalar(Value::String(title.into())),
    )]);

    let xml = to_string(&schema, &instance).unwrap();

    assert!(
        xml.contains("Edwin&#xA;Drood &amp; &quot;Others&quot;"),
        "{xml}"
    );
    assert_eq!(from_str(&xml, &schema).unwrap(), instance);
}

#[test]
fn absent_simple_content_does_not_capture_pretty_print_indentation() {
    let telecom = SchemaNode::group(
        "telecom",
        vec![
            SchemaNode::scalar("value", ScalarType::String).attribute(),
            SchemaNode::scalar("#text", ScalarType::String).text(),
        ],
    );
    let schema = SchemaNode::group("Root", vec![telecom]);
    let instance = Instance::Group(vec![(
        "telecom".into(),
        Instance::Group(vec![
            (
                "value".into(),
                Instance::Scalar(Value::String("1111111".into())),
            ),
            ("#text".into(), Instance::Scalar(Value::Null)),
        ]),
    )]);

    let xml = to_string(&schema, &instance).unwrap();
    let parsed = from_str(&xml, &schema).unwrap();

    assert!(xml.contains("<telecom value=\"1111111\"/>"));
    assert_eq!(
        parsed
            .field("telecom")
            .and_then(|value| value.field("#text"))
            .and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
}

#[test]
fn strings_preserve_whitespace_while_typed_values_accept_it() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("code", ScalarType::String).attribute(),
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    );
    let instance = from_str(
        "<Root code=\"  A  \"><Label>  padded  </Label><Count> 42 </Count></Root>",
        &schema,
    )
    .unwrap();

    assert_eq!(
        instance.field("code").and_then(Instance::as_scalar),
        Some(&Value::String("  A  ".into()))
    );
    assert_eq!(
        instance.field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("  padded  ".into()))
    );
    assert_eq!(
        instance.field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(42))
    );

    let rendered = to_string(&schema, &instance).unwrap();
    assert_eq!(from_str(&rendered, &schema).unwrap(), instance);
}

#[test]
fn xml_booleans_accept_word_and_numeric_lexicals() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("enabled", ScalarType::Bool).attribute(),
            SchemaNode::scalar("Word", ScalarType::Bool),
            SchemaNode::scalar("Numeric", ScalarType::Bool),
        ],
    );
    let instance = from_str(
        "<Root enabled=\"0\"><Word>true</Word><Numeric>1</Numeric></Root>",
        &schema,
    )
    .unwrap();

    assert_eq!(
        instance.field("enabled").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        instance.field("Word").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        instance.field("Numeric").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );

    let scalar_schema = SchemaNode::scalar("Enabled", ScalarType::Bool);
    assert!(
        to_string(
            &scalar_schema,
            &Instance::Scalar(Value::String(" 1 ".into()))
        )
        .unwrap()
        .ends_with("<Enabled>true</Enabled>")
    );
    assert!(matches!(
        from_str("<Enabled>yes</Enabled>", &scalar_schema),
        Err(XmlFormatError::ScalarParse {
            ty: ScalarType::Bool,
            ..
        })
    ));
}

#[test]
fn writer_rejects_instance_shapes_that_cannot_form_one_document() {
    let repeated_root = Instance::Repeated(vec![
        Instance::Group(Vec::new()),
        Instance::Group(Vec::new()),
    ]);
    assert!(matches!(
        to_string(&schema(), &repeated_root),
        Err(XmlFormatError::Shape {
            ref name,
            expected: "one document root",
            got: "repeating elements",
        }) if name == "Root"
    ));

    let malformed_child = Instance::Group(vec![("Name".into(), Instance::Group(Vec::new()))]);
    assert!(matches!(
        to_string(&schema(), &malformed_child),
        Err(XmlFormatError::Shape {
            ref name,
            expected: "a scalar",
            got: "an element group",
        }) if name == "Name"
    ));
}

#[test]
fn mapped_sequence_writes_zero_one_or_many_non_repeating_child_groups() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::group(
            "Entry",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )],
    );
    let entry = |value: &str| {
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )])
    };
    for (items, expected) in [
        (Vec::new(), Vec::<&str>::new()),
        (vec![entry("one")], vec!["one"]),
        (vec![entry("one"), entry("two")], vec!["one", "two"]),
    ] {
        let instance = Instance::Group(vec![("Entry".into(), Instance::MappedSequence(items))]);
        let xml = to_string(&schema, &instance).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let values = document
            .descendants()
            .filter(|node| node.has_tag_name("Entry"))
            .filter_map(|node| {
                node.children()
                    .find(|child| child.has_tag_name("Value"))
                    .and_then(|child| child.text())
            })
            .collect::<Vec<_>>();
        assert_eq!(values, expected);
    }
}

#[test]
fn mapped_sequence_is_rejected_for_roots_scalars_and_repeating_schema_nodes() {
    let sequence = Instance::MappedSequence(vec![Instance::Group(Vec::new())]);
    assert!(matches!(
        to_string(&schema(), &sequence),
        Err(XmlFormatError::Shape {
            expected: "one document root",
            got: "a mapped element sequence",
            ..
        })
    ));

    let scalar_schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let scalar_sequence =
        Instance::Group(vec![("Value".into(), Instance::MappedSequence(Vec::new()))]);
    assert!(matches!(
        to_string(&scalar_schema, &scalar_sequence),
        Err(XmlFormatError::Shape {
            expected: "one non-repeating element group",
            got: "a mapped element sequence",
            ..
        })
    ));

    let repeating_schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::group("Entry", Vec::new()).repeating()],
    );
    let repeating_sequence =
        Instance::Group(vec![("Entry".into(), Instance::MappedSequence(Vec::new()))]);
    assert!(matches!(
        to_string(&repeating_schema, &repeating_sequence),
        Err(XmlFormatError::Shape {
            expected: "one non-repeating element group",
            got: "a mapped element sequence",
            ..
        })
    ));
}

#[test]
fn writer_rejects_incompatible_scalar_values() {
    let int_schema = SchemaNode::scalar("Count", ScalarType::Int);
    assert!(matches!(
        to_string(
            &int_schema,
            &Instance::Scalar(Value::String("not an integer".into())),
        ),
        Err(XmlFormatError::ValueType {
            ref name,
            expected: ScalarType::Int,
            got: "string",
        }) if name == "Count"
    ));

    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Count", ScalarType::Int).repeating()],
    );
    let instance = Instance::Group(vec![(
        "Count".into(),
        Instance::Repeated(vec![Instance::Scalar(Value::Null)]),
    )]);
    assert!(matches!(
        to_string(&schema, &instance),
        Err(XmlFormatError::ValueType {
            ref name,
            expected: ScalarType::Int,
            got: "null",
        }) if name == "Count"
    ));

    let float_schema = SchemaNode::scalar("Number", ScalarType::Float);
    for value in ["NaN", "inf", "1e999"] {
        assert!(matches!(
            from_str(&format!("<Number>{value}</Number>"), &float_schema),
            Err(XmlFormatError::ScalarParse {
                ref name,
                ty: ScalarType::Float,
                ..
            }) if name == "Number"
        ));
    }
}

#[test]
fn writer_coerces_exact_integral_decimal_strings() {
    let schema = SchemaNode::scalar("Amount", ScalarType::Int);
    for (lexical, canonical) in [
        ("1.000", "1"),
        (" 2.0e2 ", "200"),
        ("-9223372036854775808.000", "-9223372036854775808"),
    ] {
        assert_eq!(
            to_string(
                &schema,
                &Instance::Scalar(Value::String(lexical.to_string()))
            )
            .unwrap(),
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Amount>{canonical}</Amount>")
        );
    }
    for lexical in ["1.001", "1e-1", "9223372036854775808.0"] {
        assert!(matches!(
            to_string(
                &schema,
                &Instance::Scalar(Value::String(lexical.to_string()))
            ),
            Err(XmlFormatError::ValueType {
                expected: ScalarType::Int,
                got: "string",
                ..
            })
        ));
    }
}

#[test]
fn writer_rejects_unexpected_and_duplicate_group_fields() {
    let unexpected = Instance::Group(vec![(
        "Extra".into(),
        Instance::Scalar(Value::String("lost".into())),
    )]);
    assert!(matches!(
        to_string(&schema(), &unexpected),
        Err(XmlFormatError::UnexpectedField {
            ref group,
            ref field,
        }) if group == "Root" && field == "Extra"
    ));

    let duplicate = Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String("A".into()))),
        ("Name".into(), Instance::Scalar(Value::String("B".into()))),
    ]);
    assert!(matches!(
        to_string(&schema(), &duplicate),
        Err(XmlFormatError::DuplicateField {
            ref group,
            ref field,
        }) if group == "Root" && field == "Name"
    ));
}

#[test]
fn simple_content_text_and_attributes_roundtrip() {
    let schema = SchemaNode::group(
        "Catalog",
        vec![SchemaNode::group(
            "Price",
            vec![
                SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::Float).text(),
                SchemaNode::scalar("currency", ScalarType::String).attribute(),
            ],
        )],
    );
    let instance = Instance::Group(vec![(
        "Price".into(),
        Instance::Group(vec![
            (XML_TEXT_FIELD.into(), Instance::Scalar(Value::Float(12.5))),
            (
                "currency".into(),
                Instance::Scalar(Value::String("USD".into())),
            ),
        ]),
    )]);
    let path = std::env::temp_dir().join(format!(
        "ferrule_xml_simple_content_test_{}.xml",
        std::process::id()
    ));

    write(&path, &schema, &instance).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let read_back = read(&path, &schema).unwrap();
    std::fs::remove_file(&path).unwrap();

    assert!(text.contains("<Price currency=\"USD\">12.5</Price>"));
    assert_eq!(read_back, instance);
}

#[test]
fn absent_optional_elements_preserve_scalar_and_group_presence() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Nick", ScalarType::String),
            SchemaNode::group(
                "Extra",
                vec![SchemaNode::scalar("Note", ScalarType::String)],
            ),
        ],
    );
    let path = std::env::temp_dir().join(format!(
        "ferrule_format_xml_optional_test_{}.xml",
        std::process::id()
    ));
    std::fs::write(&path, "<Root><Name>Jane</Name></Root>").unwrap();

    let instance = read(&path, &schema).unwrap();
    assert_eq!(instance.field("Nick"), Some(&Instance::Scalar(Value::Null)));
    assert_eq!(instance.field("Extra"), None);

    // Writing the Null and omitted group back does not invent empty
    // elements for either absent value.
    write(&path, &schema, &instance).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!text.contains("Nick"), "{text}");
    assert!(!text.contains("Extra"), "{text}");
}

#[test]
fn generic_element_group_reads_heterogeneous_children_in_document_order() {
    let generic = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    )
    .repeating();
    let schema = SchemaNode::group("Catalog", vec![SchemaNode::group("Items", vec![generic])]);

    let instance = from_str(
            "<Catalog><Items><Alpha><Label>first</Label></Alpha><Beta><Label>second</Label></Beta></Items></Catalog>",
            &schema,
        )
        .unwrap();
    let items = instance
        .field("Items")
        .and_then(|items| items.field(XML_ELEMENTS_FIELD))
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0]
            .field(XML_LOCAL_NAME_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("Alpha".into()))
    );
    assert_eq!(
        items[1].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("second".into()))
    );

    let xml = to_string(&schema, &instance).unwrap();
    assert!(xml.contains("<Alpha>"), "{xml}");
    assert!(xml.contains("<Beta>"), "{xml}");
    assert!(xml.find("<Alpha>") < xml.find("<Beta>"), "{xml}");
}

#[test]
fn generic_text_elements_use_the_mapped_runtime_name() {
    let generic = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_NODE_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating();
    let schema = SchemaNode::group("Record", vec![generic]);
    let instance = Instance::Group(vec![(
        XML_ELEMENTS_FIELD.into(),
        Instance::Repeated(vec![Instance::Group(vec![
            (
                XML_NODE_NAME_FIELD.into(),
                Instance::Scalar(Value::String("Code".into())),
            ),
            (
                XML_TEXT_FIELD.into(),
                Instance::Scalar(Value::String("A-17".into())),
            ),
        ])]),
    )]);

    let xml = to_string(&schema, &instance).unwrap();
    assert!(xml.contains("<Code>A-17</Code>"), "{xml}");
    assert_eq!(from_str(&xml, &schema).unwrap(), instance);
}

#[test]
fn generic_elements_preserve_ordered_runtime_attributes() {
    let attributes = SchemaNode::group(
        XML_ATTRIBUTES_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating();
    let generic = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            attributes,
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating();
    let schema = SchemaNode::group("Record", vec![generic]);

    let instance = from_str(
        "<Record><Field name=\"FirstName\" type=\"string\"/></Record>",
        &schema,
    )
    .unwrap();
    let field = instance
        .field(XML_ELEMENTS_FIELD)
        .and_then(Instance::as_repeated)
        .and_then(|items| items.first())
        .unwrap();
    let attributes = field
        .field(XML_ATTRIBUTES_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(attributes.len(), 2);
    assert_eq!(
        attributes[0]
            .field(XML_LOCAL_NAME_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("name".into()))
    );
    assert_eq!(
        attributes[0]
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::String("FirstName".into()))
    );

    let xml = to_string(&schema, &instance).unwrap();
    assert!(
        xml.contains("<Field name=\"FirstName\" type=\"string\">"),
        "{xml}"
    );
}

#[test]
fn group_alternatives_emit_selected_xsi_type_and_integral_float() {
    let address = SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("state", ScalarType::String),
            SchemaNode::scalar("zip", ScalarType::Int),
            SchemaNode::scalar("postcode", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "{urn:ferrule:test}Domestic".into(),
            members: vec!["name".into(), "state".into(), "zip".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "{urn:ferrule:test}International".into(),
            members: vec!["name".into(), "postcode".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .unwrap();
    let read = from_str(
            r#"<Address xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:t="urn:ferrule:test" xsi:type="t:Domestic"><name>Ada</name></Address>"#,
            &address,
        )
        .unwrap();
    assert_eq!(
        read.field(XML_TYPE_FIELD).and_then(Instance::as_scalar),
        Some(&Value::String("{urn:ferrule:test}Domestic".into()))
    );
    let read_xml = to_string(&address, &read).unwrap();
    assert!(read_xml.contains("xsi:type=\"ft:Domestic\""), "{read_xml}");
    assert!(matches!(
        from_str(
            r#"<Address xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Missing"><name>Ada</name></Address>"#,
            &address,
        ),
        Err(XmlFormatError::UnknownXmlType { .. })
    ));
    let schema = SchemaNode::group("Root", vec![address]);
    let instance = Instance::Group(vec![(
        "Address".into(),
        Instance::Group(vec![
            ("name".into(), Instance::Scalar(Value::String("Ada".into()))),
            ("state".into(), Instance::Scalar(Value::String("WA".into()))),
            ("zip".into(), Instance::Scalar(Value::Float(98101.0))),
            ("postcode".into(), Instance::Scalar(Value::Null)),
        ]),
    )]);
    let xml = to_string(&schema, &instance).unwrap();
    assert!(xml.contains("xsi:type=\"ft:Domestic\""), "{xml}");
    assert!(xml.contains("xmlns:ft=\"urn:ferrule:test\""), "{xml}");
    assert!(xml.contains("<zip>98101</zip>"), "{xml}");
}

#[test]
fn unexpected_root_element_is_reported() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_format_xml_test_bad_{}.xml",
        std::process::id()
    ));
    std::fs::write(&path, "<Other/>").unwrap();

    let err = read(&path, &schema()).unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(err, XmlFormatError::UnexpectedRoot { .. }));
}

#[test]
fn xml_nil_is_distinct_from_absent_and_empty_elements() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Nil", ScalarType::String).nillable(),
            SchemaNode::scalar("Empty", ScalarType::String).nillable(),
            SchemaNode::scalar("Absent", ScalarType::String).nillable(),
        ],
    );
    let instance = from_str(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Nil xsi:nil="true"/><Empty/></Root>"#,
            &schema,
        )
        .unwrap();
    assert_eq!(
        instance.field("Nil").and_then(Instance::as_scalar),
        Some(&Value::xml_nil())
    );
    assert_eq!(
        instance.field("Empty").and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
    assert_eq!(
        instance.field("Absent").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );

    let xml = to_string(&schema, &instance).unwrap();
    assert!(
        xml.contains(
            r#"<Nil xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#
        ),
        "{xml}"
    );
    assert!(xml.contains("<Empty></Empty>"), "{xml}");
    assert!(!xml.contains("Absent"), "{xml}");
}

#[test]
fn xml_nil_requires_nillable_schema_and_no_content() {
    let plain = SchemaNode::scalar("Value", ScalarType::String);
    assert!(matches!(
        from_str(
            r#"<Value xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true"/>"#,
            &plain,
        ),
        Err(XmlFormatError::UnexpectedXmlNil { .. })
    ));
    let nillable = plain.nillable();
    assert!(matches!(
        from_str(
            r#"<Value xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:nil="true">text</Value>"#,
            &nillable,
        ),
        Err(XmlFormatError::XmlNilWithContent { .. })
    ));
}

#[test]
fn explicit_xml_namespaces_match_and_render_expanded_names() {
    let qualified = |name: &str, namespace: &str| {
        SchemaNode::scalar(name, ScalarType::String)
            .xml_qualified(namespace)
            .unwrap()
    };
    let schema = SchemaNode::group(
        "Root",
        vec![
            qualified("Code", "urn:ferrule:document"),
            SchemaNode::scalar("Plain", ScalarType::String).xml_unqualified(),
            qualified("token", "urn:ferrule:metadata").attribute(),
        ],
    )
    .xml_qualified("urn:ferrule:document")
    .unwrap();
    let xml = r#"<Root xmlns="urn:ferrule:document" xmlns:o="urn:ferrule:other" xmlns:m="urn:ferrule:metadata" m:token="A1"><o:Code>foreign</o:Code><Code>selected</Code><Plain xmlns="">plain</Plain></Root>"#;

    let instance = from_str(xml, &schema).unwrap();
    assert_eq!(
        instance.field("Code").and_then(Instance::as_scalar),
        Some(&Value::String("selected".into()))
    );
    assert_eq!(
        instance.field("Plain").and_then(Instance::as_scalar),
        Some(&Value::String("plain".into()))
    );
    assert_eq!(
        instance.field("token").and_then(Instance::as_scalar),
        Some(&Value::String("A1".into()))
    );

    let rendered = to_string(&schema, &instance).unwrap();
    assert!(
        rendered.contains(r#"<Root xmlns="urn:ferrule:document""#),
        "{rendered}"
    );
    assert!(
        rendered.contains(r#"xmlns:fns1="urn:ferrule:metadata" fns1:token="A1""#),
        "{rendered}"
    );
    assert!(
        rendered.contains(r#"<Plain xmlns="">plain</Plain>"#),
        "{rendered}"
    );
    assert_eq!(from_str(&rendered, &schema).unwrap(), instance);

    assert!(matches!(
        from_str(
            r#"<o:Root xmlns:o="urn:ferrule:other"><o:Code>wrong</o:Code></o:Root>"#,
            &schema,
        ),
        Err(XmlFormatError::UnexpectedRoot { expected, found })
            if expected == "{urn:ferrule:document}Root"
                && found == "{urn:ferrule:other}Root"
    ));
}

#[test]
fn legacy_xml_names_remain_local_name_matches() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let instance = from_str(
        r#"<x:Root xmlns:x="urn:ferrule:legacy"><x:Value>kept</x:Value></x:Root>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(
        instance.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("kept".into()))
    );
}

#[test]
fn recursive_groups_round_trip_and_export_as_root_references() {
    let schema = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::recursive_group("directory", "directory").repeating(),
        ],
    );
    let instance = recursive_directory("root", Some(recursive_directory("child", None)));

    let xml = to_string(&schema, &instance).unwrap();
    assert!(xml.contains("<directory>\n    <name>child</name>"), "{xml}");
    assert_eq!(from_str(&xml, &schema).unwrap(), instance);

    let xsd = xsd::export(&schema).unwrap();
    assert!(
        xsd.contains("<xs:element ref=\"directory\" minOccurs=\"0\" maxOccurs=\"unbounded\"/>"),
        "{xsd}"
    );
}

#[test]
fn recursive_xml_writes_are_depth_bounded() {
    let schema = SchemaNode::group(
        "directory",
        vec![SchemaNode::recursive_group("directory", "directory")],
    );
    let mut instance = Instance::Group(Vec::new());
    for _ in 0..=MAX_XML_RECURSION_DEPTH {
        instance = Instance::Group(vec![("directory".into(), instance)]);
    }
    assert!(matches!(
        to_string(&schema, &instance),
        Err(XmlFormatError::RecursionLimit {
            limit: MAX_XML_RECURSION_DEPTH
        })
    ));
}

fn recursive_directory(name: &str, child: Option<Instance>) -> Instance {
    Instance::Group(vec![
        (
            "name".into(),
            Instance::Scalar(Value::String(name.to_string())),
        ),
        (
            "directory".into(),
            Instance::Repeated(child.into_iter().collect()),
        ),
    ])
}
