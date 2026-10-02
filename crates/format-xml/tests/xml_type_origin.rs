use ir::{
    GroupAlternative, Instance, ScalarType, SchemaNode, Value, XML_TYPE_FIELD, XmlTypeOrigin,
};

fn schema(namespace: Option<&str>) -> SchemaNode {
    let identity =
        |name: &str| namespace.map_or_else(|| name.to_string(), |ns| format!("{{{ns}}}{name}"));
    let mut schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Code", ScalarType::String).attribute(),
            SchemaNode::scalar("Extra", ScalarType::String).attribute(),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: identity("Base"),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: identity("Derived"),
            members: vec!["Code".into(), "Extra".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some(identity("Base"));
    schema
}

fn strip_origin(mut value: Instance) -> Instance {
    let Instance::Group(fields) = &mut value else {
        panic!("group expected")
    };
    fields.set_xml_type_origin(XmlTypeOrigin::Unknown).unwrap();
    value
}

#[test]
fn explicit_and_inferred_selected_types_retain_distinct_source_facts() {
    let schema = schema(None);
    let explicit = format_xml::from_str(r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="a" Extra="b"/>"#, &schema).unwrap();
    let inferred = format_xml::from_str(r#"<Root Code="a" Extra="b"/>"#, &schema).unwrap();
    assert_eq!(
        explicit.field(XML_TYPE_FIELD),
        inferred.field(XML_TYPE_FIELD)
    );
    assert_eq!(
        strip_origin(explicit.clone()),
        strip_origin(inferred.clone())
    );
    assert_ne!(explicit, inferred);
    assert_eq!(
        explicit.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Derived"))
    );
    assert_eq!(inferred.xml_type_origin(), Ok(XmlTypeOrigin::Absent));
    let output = format_xml::to_string(&schema, &explicit).unwrap();
    assert_eq!(format_xml::to_string(&schema, &inferred).unwrap(), output);
    let reread = format_xml::from_str(&output, &schema).unwrap();
    assert_eq!(
        reread.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Derived"))
    );
    assert_ne!(reread, inferred);
}

#[test]
fn declared_default_captures_absence_before_its_early_return() {
    let schema = schema(None);
    let absent = format_xml::from_str(r#"<Root Code="a"/>"#, &schema).unwrap();
    let explicit = format_xml::from_str(
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="a"/>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(absent.xml_type_origin(), Ok(XmlTypeOrigin::Absent));
    assert!(absent.field(XML_TYPE_FIELD).is_none());
    assert!(
        !format_xml::to_string(&schema, &absent)
            .unwrap()
            .contains("xsi:type")
    );
    assert_eq!(
        explicit.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Base"))
    );
    assert!(
        format_xml::to_string(&schema, &explicit)
            .unwrap()
            .contains("xsi:type=\"Base\"")
    );
}

#[test]
fn qualified_annotation_identity_is_independent_of_lexical_prefix() {
    let schema = schema(Some("urn:types"));
    for prefix in ["a", "longAlias"] {
        let xml = format!(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:{prefix}="urn:types" xsi:type="{prefix}:Derived" Code="a" Extra="b"/>"#
        );
        assert_eq!(
            format_xml::from_str(&xml, &schema)
                .unwrap()
                .xml_type_origin(),
            Ok(XmlTypeOrigin::Explicit("{urn:types}Derived"))
        );
    }
}

#[test]
fn nested_and_repeated_occurrences_own_their_annotation_facts() {
    let schema = SchemaNode::group("Envelope", vec![schema(None).repeating()]);
    let value = format_xml::from_str(r#"<Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><Root Code="a"/><Root xsi:type="Derived" Code="b" Extra="e"/></Envelope>"#, &schema).unwrap();
    assert_eq!(value.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    let items = value.field("Root").unwrap().as_repeated().unwrap();
    assert_eq!(items[0].xml_type_origin(), Ok(XmlTypeOrigin::Absent));
    assert_eq!(
        items[1].xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Derived"))
    );
    assert_eq!(
        format_xml::from_str(&format_xml::to_string(&schema, &value).unwrap(), &schema).unwrap(),
        value
    );
}

#[test]
fn legacy_constructed_values_keep_selected_type_writer_behavior() {
    let schema = schema(None);
    let value = Instance::Group(
        (vec![
            ("Code".into(), Instance::Scalar(Value::String("a".into()))),
            (
                XML_TYPE_FIELD.into(),
                Instance::Scalar(Value::String("Derived".into())),
            ),
        ])
        .into(),
    );
    assert_eq!(value.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    let output = format_xml::to_string(&schema, &value).unwrap();
    for origin in [
        XmlTypeOrigin::Absent,
        XmlTypeOrigin::Explicit("Base"),
        XmlTypeOrigin::Explicit("Derived"),
    ] {
        let Instance::Group(fields) = value.clone() else {
            unreachable!()
        };
        let annotated = Instance::Group(fields.with_xml_type_origin(origin).unwrap());
        assert_eq!(format_xml::to_string(&schema, &annotated).unwrap(), output);
    }
}

#[test]
fn unsupported_annotation_and_malformed_metadata_keep_typed_errors() {
    let schema = schema(None);
    for xml in [
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Missing"/>"#,
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="p:Derived"/>"#,
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="a" Extra="b"/>"#,
    ] {
        assert!(format_xml::from_str(xml, &schema).is_err());
    }
    assert!(matches!(
        ir::InstanceGroup::default().with_xml_type_origin(XmlTypeOrigin::Explicit("")),
        Err(ir::XmlTypeOriginError::InvalidPayload)
    ));
}
