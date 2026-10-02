use format_xml::{XmlFormatError, XmlReadOptions};
use ir::{
    GroupAlternative, Instance, ScalarType, SchemaKind, SchemaNode, Value, XmlNamespace,
    XmlTypeOrigin,
};

fn schema() -> SchemaNode {
    let attribute = |name: &str| {
        let mut field = SchemaNode::scalar(name, ScalarType::String).attribute();
        field.xml_namespace = Some(XmlNamespace::Unqualified);
        field
    };
    let mut schema = SchemaNode::group("Root", vec![attribute("Code"), attribute("Extra")])
        .with_alternatives(vec![
            GroupAlternative {
                name: "Base".into(),
                members: vec!["Code".into()],
                required: vec![],
                constraints: vec![],
            },
            GroupAlternative {
                name: "Derived".into(),
                members: vec!["Code".into(), "Extra".into()],
                required: vec![],
                constraints: vec![],
            },
        ])
        .unwrap();
    schema.xml_namespace = Some(XmlNamespace::Unqualified);
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some("Base".into());
    schema
}

fn options() -> XmlReadOptions {
    XmlReadOptions {
        allow_inactive_root_type_members: true,
    }
}

#[test]
fn ordinary_reader_rejects_inactive_explicit_type_member() {
    let text = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="c" Extra="e"/>"#;
    assert!(matches!(
        format_xml::from_str(text, &schema()),
        Err(XmlFormatError::NoMatchingAlternative { .. })
    ));
    assert!(matches!(
        format_xml::from_str_with_options(text, &schema(), &XmlReadOptions::default()),
        Err(XmlFormatError::NoMatchingAlternative { .. })
    ));
}

#[test]
fn opted_in_reader_retains_actual_type_and_declared_inactive_value() {
    let text = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="c" Extra="e"/>"#;
    let value = format_xml::from_str_with_options(text, &schema(), &options()).unwrap();
    assert_eq!(
        value.xml_type_origin().unwrap(),
        XmlTypeOrigin::Explicit("Base")
    );
    assert_eq!(
        ir::primary_root_scalar(Some(&value), &["Extra"]).unwrap(),
        Value::String("e".into())
    );
    assert!(!ir::primary_root_xml_type_equals(Some(&value), "Derived").unwrap());
}

#[test]
fn unmarked_selection_and_actual_absence_are_unchanged() {
    let text = r#"<Root Code="c" Extra="e"/>"#;
    let strict = format_xml::from_str(text, &schema()).unwrap();
    let relaxed = format_xml::from_str_with_options(text, &schema(), &options()).unwrap();
    assert_eq!(strict, relaxed);
    assert_eq!(relaxed.xml_type_origin().unwrap(), XmlTypeOrigin::Absent);
    assert!(!ir::primary_root_xml_type_equals(Some(&relaxed), "Derived").unwrap());
}

#[test]
fn unknown_annotations_and_invalid_qnames_remain_errors() {
    for (annotation, unknown) in [
        ("Unknown", true),
        ("missing:Base", false),
        ("Base Derived", false),
    ] {
        let text = format!(
            r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="{annotation}" Code="c" Extra="e"/>"#
        );
        for opt in [XmlReadOptions::default(), options()] {
            let error = format_xml::from_str_with_options(&text, &schema(), &opt).unwrap_err();
            assert!(if unknown {
                matches!(error, XmlFormatError::UnknownXmlType { .. })
            } else {
                matches!(error, XmlFormatError::InvalidXmlType { .. })
            });
        }
    }
}

#[test]
fn inactive_members_keep_fixed_value_validation() {
    let mut schema = schema();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    children[1].fixed = Some("fixed".into());
    let text = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="c" Extra="wrong"/>"#;
    assert!(matches!(
        format_xml::from_str_with_options(text, &schema, &options()),
        Err(XmlFormatError::FixedValue { .. })
    ));
}

#[test]
fn opted_in_reader_refuses_unsupported_shape_and_metadata() {
    let mut shapes = Vec::new();
    let mut root = schema();
    root.repeating = true;
    shapes.push(root);
    let mut root = schema();
    root.xml_namespace = None;
    shapes.push(root);
    let mut root = schema();
    root.xml_default_type = None;
    shapes.push(root);
    let mut root = schema();
    root.nullable = true;
    shapes.push(root);
    let mut root = schema();
    let SchemaKind::Group { children, .. } = &mut root.kind else {
        unreachable!()
    };
    children[1].attribute = false;
    shapes.push(root);
    let mut root = schema();
    let SchemaKind::Group { children, .. } = &mut root.kind else {
        unreachable!()
    };
    children[1] = SchemaNode::group("Extra", vec![]);
    shapes.push(root);
    let mut root = schema();
    let SchemaKind::Group { alternatives, .. } = &mut root.kind else {
        unreachable!()
    };
    alternatives[0].required.push("Code".into());
    shapes.push(root);
    for root in shapes {
        assert!(matches!(
            format_xml::from_str_with_options("<Root/>", &root, &options()),
            Err(XmlFormatError::UnsupportedInactiveRootTypeMembers { .. })
        ));
    }
}

#[test]
fn file_and_string_options_read_the_same_tree() {
    let text = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Code="c" Extra="e"/>"#;
    let path = std::env::temp_dir().join(format!(
        "ferrule-inactive-root-reader-{}.xml",
        std::process::id()
    ));
    std::fs::write(&path, text).unwrap();
    let loaded = format_xml::read_with_options(&path, &schema(), &options()).unwrap();
    std::fs::remove_file(path).unwrap();
    let expected = format_xml::from_str_with_options(text, &schema(), &options()).unwrap();
    assert_eq!(loaded, expected);
    assert!(matches!(loaded, Instance::Group(_)));
}

#[test]
fn qualified_aliases_retain_canonical_known_type_and_attribute_identity() {
    let mut schema = schema();
    schema.xml_namespace = XmlNamespace::qualified("urn:typed-root");
    schema.xml_default_type = Some("{urn:typed-root}Base".into());
    let SchemaKind::Group {
        alternatives,
        children,
        ..
    } = &mut schema.kind
    else {
        unreachable!()
    };
    for alternative in alternatives {
        alternative.name = format!("{{urn:typed-root}}{}", alternative.name);
    }
    children[1].xml_namespace = XmlNamespace::qualified("urn:typed-root");
    let text = r#"<r:Root xmlns:r="urn:typed-root" xmlns:t="urn:typed-root" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="t:Base" Code="c" r:Extra="e"/>"#;
    assert!(matches!(
        format_xml::from_str(text, &schema),
        Err(XmlFormatError::NoMatchingAlternative { .. })
    ));
    let value = format_xml::from_str_with_options(text, &schema, &options()).unwrap();
    assert_eq!(
        value.xml_type_origin().unwrap(),
        XmlTypeOrigin::Explicit("{urn:typed-root}Base")
    );
    assert_eq!(
        ir::primary_root_scalar(Some(&value), &["Extra"]).unwrap(),
        Value::String("e".into())
    );
    let wrong_attribute = text.replace("r:Extra=", "Extra=");
    let value = format_xml::from_str_with_options(&wrong_attribute, &schema, &options()).unwrap();
    assert_eq!(
        ir::primary_root_scalar(Some(&value), &["Extra"]).unwrap(),
        Value::Null
    );
}

#[test]
fn flat_member_and_alternative_limits_are_exact() {
    let mut schema = schema();
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &mut schema.kind
    else {
        unreachable!()
    };
    for i in 2..32 {
        let mut child = SchemaNode::scalar(format!("Field{i}"), ScalarType::String).attribute();
        child.xml_namespace = Some(XmlNamespace::Unqualified);
        children.push(child);
        alternatives[1].members.push(format!("Field{i}"));
    }
    assert!(ir::xml_inactive_root_type_members_are_supported(&schema));
    let text = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Base" Field31="last"/>"#;
    let value = format_xml::from_str_with_options(text, &schema, &options()).unwrap();
    assert_eq!(
        ir::primary_root_scalar(Some(&value), &["Field31"]).unwrap(),
        Value::String("last".into())
    );
    let mut too_many = schema.clone();
    let SchemaKind::Group { children, .. } = &mut too_many.kind else {
        unreachable!()
    };
    let mut child = SchemaNode::scalar("Field32", ScalarType::String).attribute();
    child.xml_namespace = Some(XmlNamespace::Unqualified);
    children.push(child);
    assert!(!ir::xml_inactive_root_type_members_are_supported(&too_many));
    let SchemaKind::Group { alternatives, .. } = &mut schema.kind else {
        unreachable!()
    };
    for i in 2..32 {
        alternatives.push(GroupAlternative {
            name: format!("Type{i}"),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        });
    }
    assert!(ir::xml_inactive_root_type_members_are_supported(&schema));
    let SchemaKind::Group { alternatives, .. } = &mut schema.kind else {
        unreachable!()
    };
    alternatives.push(GroupAlternative {
        name: "Type32".into(),
        members: vec!["Code".into()],
        required: vec![],
        constraints: vec![],
    });
    assert!(!ir::xml_inactive_root_type_members_are_supported(&schema));
}

#[test]
fn required_attribute_metadata_does_not_change_the_reader_presence_contract() {
    let mut schema = schema();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    children[1].xml_attribute_required = true;
    let text = r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Derived" Code="c"/>"#;
    let strict = format_xml::from_str(text, &schema).unwrap();
    let relaxed = format_xml::from_str_with_options(text, &schema, &options()).unwrap();
    assert_eq!(strict, relaxed);
    assert_eq!(
        ir::primary_root_scalar(Some(&relaxed), &["Extra"]).unwrap(),
        Value::Null
    );
}
