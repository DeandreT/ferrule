use super::super::*;

#[test]
fn export_then_import_roundtrips() {
    let schema = SchemaNode::group(
        "Orders",
        vec![
            SchemaNode::scalar("Date", ScalarType::String),
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Qty", ScalarType::Int),
                    SchemaNode::scalar("Price", ScalarType::Float),
                    SchemaNode::scalar("Rush", ScalarType::Bool),
                    // Import collects attributes after elements, so the
                    // hand-built schema lists them last for equality.
                    SchemaNode::scalar("id", ScalarType::String).attribute(),
                ],
            )
            .repeating(),
            SchemaNode::group(
                "Price",
                vec![
                    SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::Float).text(),
                    SchemaNode::scalar("currency", ScalarType::String).attribute(),
                ],
            ),
        ],
    );
    let text = export(&schema).unwrap();
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_export_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(&path, text).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_rejects_group_nodes_with_scalar_xml_roles() {
    for schema in [
        SchemaNode::group(
            "Root",
            vec![SchemaNode::group("Metadata", Vec::new()).attribute()],
        ),
        SchemaNode::group(
            "Root",
            vec![SchemaNode::group("Content", Vec::new()).text()],
        ),
    ] {
        assert!(matches!(
            export(&schema),
            Err(XmlFormatError::UnsupportedSchemaRole { kind: "group", .. })
        ));
    }
}

#[test]
fn export_represents_generic_element_groups_as_xsd_wildcards() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group(
                ir::XML_ELEMENTS_FIELD,
                vec![
                    SchemaNode::scalar(ir::XML_LOCAL_NAME_FIELD, ScalarType::String),
                    SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                ],
            )
            .repeating(),
        ],
    );

    let xsd = export(&schema).unwrap();
    assert!(
        xsd.contains(r#"<xs:any minOccurs="0" maxOccurs="unbounded" processContents="lax"/>"#),
        "{xsd}"
    );
    assert!(!xsd.contains(r#"name="element()""#), "{xsd}");
}

#[test]
fn export_roundtrips_named_base_and_derived_group_alternatives() {
    let address = SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("city", ScalarType::String),
            SchemaNode::scalar("postcode", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "{urn:ferrule:address}BaseAddress".into(),
            members: vec!["name".into(), "city".into()],
            required: Vec::new(),
        },
        ir::GroupAlternative {
            name: "{urn:ferrule:address}PostalAddress".into(),
            members: vec!["name".into(), "city".into(), "postcode".into()],
            required: Vec::new(),
        },
    ])
    .unwrap();
    let schema = SchemaNode::group("Root", vec![address]);

    let xsd = export(&schema).unwrap();
    assert!(xsd.contains(r#"targetNamespace="urn:ferrule:address""#));
    assert!(xsd.contains(r#"<xs:complexType name="BaseAddress">"#));
    assert!(xsd.contains(r#"<xs:extension base="tns:BaseAddress">"#));
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_alternatives_{}.xsd",
        std::process::id()
    ));
    std::fs::write(&path, xsd).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();

    assert_eq!(imported.child("Address").unwrap().alternatives().len(), 2);
    assert_eq!(imported, schema);
}

#[test]
fn export_uses_an_abstract_common_base_for_sibling_alternatives() {
    let choice = SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("state", ScalarType::String),
            SchemaNode::scalar("postcode", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "Domestic".into(),
            members: vec!["name".into(), "state".into()],
            required: Vec::new(),
        },
        ir::GroupAlternative {
            name: "International".into(),
            members: vec!["name".into(), "postcode".into()],
            required: Vec::new(),
        },
    ])
    .unwrap();
    let schema = SchemaNode::group("Root", vec![choice]);

    let xsd = export(&schema).unwrap();
    assert!(xsd.contains(r#"name="DomesticBaseType" abstract="true""#));
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_sibling_alternatives_{}.xsd",
        std::process::id()
    ));
    std::fs::write(&path, xsd).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_rejects_alternatives_from_incompatible_namespaces() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "{urn:ferrule:first}First".into(),
            members: vec!["Value".into()],
            required: Vec::new(),
        },
        ir::GroupAlternative {
            name: "{urn:ferrule:second}Second".into(),
            members: vec!["Value".into()],
            required: Vec::new(),
        },
    ])
    .unwrap();
    assert!(matches!(
        export(&schema),
        Err(XmlFormatError::UnsupportedGroupAlternatives { ref group }) if group == "Root"
    ));
}

#[test]
fn export_roundtrips_different_derived_views_of_one_base_type() {
    fn address(name: &str, derived: &str, extra: &str) -> SchemaNode {
        let identity = |local: &str| format!("{{urn:ferrule:asymmetric-address}}{local}");
        SchemaNode::group(
            name,
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar(extra, ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            ir::GroupAlternative {
                name: identity("BaseAddress"),
                members: vec!["name".into()],
                required: Vec::new(),
            },
            ir::GroupAlternative {
                name: identity(derived),
                members: vec!["name".into(), extra.into()],
                required: Vec::new(),
            },
        ])
        .unwrap()
    }
    let schema = SchemaNode::group(
        "Root",
        vec![
            address("Shipping", "PostalAddress", "postcode"),
            address("Billing", "DomesticAddress", "state"),
        ],
    );
    let xsd = export(&schema).unwrap();
    assert!(xsd.contains(r#"name="PostalAddress""#), "{xsd}");
    assert!(xsd.contains(r#"name="DomesticAddress""#), "{xsd}");
    assert!(
        xsd.contains(r#"targetNamespace="urn:ferrule:asymmetric-address""#),
        "{xsd}"
    );
    assert!(xsd.contains("urn:ferrule:xsd:group-alternatives"), "{xsd}");
    assert_eq!(xsd.matches("<ferrule:type").count(), 4, "{xsd}");

    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_asymmetric_alternatives_{}.xsd",
        std::process::id()
    ));
    std::fs::write(&path, xsd).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_reuses_an_implicit_base_from_an_overlapping_derived_view() {
    let identity = |local: &str| format!("{{urn:ferrule:implicit-base}}{local}");
    let shipping = SchemaNode::group(
        "Shipping",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("state", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: identity("Address"),
            members: vec!["name".into()],
            required: Vec::new(),
        },
        ir::GroupAlternative {
            name: identity("Domestic"),
            members: vec!["name".into(), "state".into()],
            required: Vec::new(),
        },
    ])
    .unwrap();
    let billing = SchemaNode::group(
        "Billing",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::scalar("state", ScalarType::String),
            SchemaNode::scalar("postcode", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: identity("International"),
            members: vec!["name".into(), "postcode".into()],
            required: Vec::new(),
        },
        ir::GroupAlternative {
            name: identity("Domestic"),
            members: vec!["name".into(), "state".into()],
            required: Vec::new(),
        },
    ])
    .unwrap();
    let schema = SchemaNode::group("Root", vec![shipping, billing]);

    let xsd = export(&schema).unwrap();
    assert_eq!(xsd.matches(r#"name="Address""#).count(), 1, "{xsd}");
    assert!(xsd.contains(r#"name="International""#), "{xsd}");
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_implicit_alternative_base_{}.xsd",
        std::process::id()
    ));
    std::fs::write(&path, xsd).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_rejects_multiple_text_fields_and_preserves_string_mixed_content() {
    let multiple = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("First", ScalarType::String).text(),
            SchemaNode::scalar("Second", ScalarType::String).text(),
        ],
    );
    assert!(matches!(
        export(&multiple),
        Err(XmlFormatError::MultipleTextFields { count: 2, .. })
    ));

    let mixed = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Child", ScalarType::String),
        ],
    );
    let xsd = export(&mixed).unwrap();
    assert!(xsd.contains(r#"<xs:complexType mixed="true">"#));

    let typed_mixed = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::Int).text(),
            SchemaNode::scalar("Child", ScalarType::String),
        ],
    );
    assert!(matches!(
        export(&typed_mixed),
        Err(XmlFormatError::MixedContent { .. })
    ));
}

#[test]
fn export_rejects_conflicting_and_repeating_xml_roles() {
    let conflicting = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Value", ScalarType::String)
                .attribute()
                .text(),
        ],
    );
    assert!(matches!(
        export(&conflicting),
        Err(XmlFormatError::ConflictingSchemaRoles { .. })
    ));

    let repeating = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Code", ScalarType::String)
                .attribute()
                .repeating(),
        ],
    );
    assert!(matches!(
        export(&repeating),
        Err(XmlFormatError::RepeatingSchemaRole { .. })
    ));
}

#[test]
fn nillable_elements_import_and_export() {
    let path =
        std::env::temp_dir().join(format!("ferrule_xsd_nillable_{}.xsd", std::process::id()));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Root"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string" nillable="true"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let schema = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let value = schema.child("Value").unwrap();
    assert!(value.nillable);
    let exported = export(&schema).unwrap();
    assert!(exported.contains(r#"name="Value" type="xs:string" nillable="true""#));
}
