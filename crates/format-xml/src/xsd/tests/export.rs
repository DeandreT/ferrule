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
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "{urn:ferrule:address}PostalAddress".into(),
            members: vec!["name".into(), "city".into(), "postcode".into()],
            required: Vec::new(),
            constraints: Vec::new(),
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
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "International".into(),
            members: vec!["name".into(), "postcode".into()],
            required: Vec::new(),
            constraints: Vec::new(),
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
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "{urn:ferrule:second}Second".into(),
            members: vec!["Value".into()],
            required: Vec::new(),
            constraints: Vec::new(),
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
                constraints: Vec::new(),
            },
            ir::GroupAlternative {
                name: identity(derived),
                members: vec!["name".into(), extra.into()],
                required: Vec::new(),
                constraints: Vec::new(),
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
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: identity("Domestic"),
            members: vec!["name".into(), "state".into()],
            required: Vec::new(),
            constraints: Vec::new(),
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
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: identity("Domestic"),
            members: vec!["name".into(), "state".into()],
            required: Vec::new(),
            constraints: Vec::new(),
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
fn export_reuses_identical_recursive_anchors_at_multiple_paths() {
    let description = || {
        SchemaNode::group(
            "description",
            vec![
                SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::recursive_group("strong", "description").repeating(),
            ],
        )
    };
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("First", vec![description()]),
            SchemaNode::group("Second", vec![description().repeating()]),
        ],
    );

    let xsd = export(&schema).unwrap();

    assert_eq!(xsd.matches(r#"name="descriptionType""#).count(), 1, "{xsd}");
    assert_eq!(xsd.matches(r#"type="descriptionType""#).count(), 3, "{xsd}");
}

#[test]
fn export_rejects_conflicting_recursive_anchor_definitions() {
    let branch = |field| {
        SchemaNode::group(
            "Branch",
            vec![
                SchemaNode::scalar(field, ScalarType::String),
                SchemaNode::recursive_group("Child", "Branch"),
            ],
        )
    };
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("Left", vec![branch("Code")]),
            SchemaNode::group("Right", vec![branch("Name")]),
        ],
    );

    assert!(matches!(
        export(&schema),
        Err(XmlFormatError::UnsupportedRecursiveAnchor { anchor, .. }) if anchor == "Branch"
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

#[test]
fn qualified_schema_export_preserves_element_and_attribute_forms() {
    let namespace = "urn:ferrule:qualified";
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Qualified", ScalarType::String)
                .xml_qualified(namespace)
                .unwrap(),
            SchemaNode::scalar("Plain", ScalarType::String).xml_unqualified(),
            SchemaNode::scalar("qualifiedAttribute", ScalarType::String)
                .xml_qualified(namespace)
                .unwrap()
                .attribute(),
            SchemaNode::scalar("plainAttribute", ScalarType::String)
                .xml_unqualified()
                .attribute(),
        ],
    )
    .xml_qualified(namespace)
    .unwrap();

    let xsd = export(&schema).unwrap();
    assert!(
        xsd.contains(r#"targetNamespace="urn:ferrule:qualified""#),
        "{xsd}"
    );
    assert!(
        xsd.contains(r#"name="Qualified" type="xs:string" form="qualified""#),
        "{xsd}"
    );
    assert!(xsd.contains(r#"name="Plain" type="xs:string""#), "{xsd}");
    assert!(
        xsd.contains(r#"name="qualifiedAttribute" type="xs:string" form="qualified""#),
        "{xsd}"
    );

    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_qualified_export_{}.xsd",
        std::process::id()
    ));
    std::fs::write(&path, xsd).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_set_partitions_and_deduplicates_recursive_namespace_boundaries() {
    let qualified = |node: SchemaNode, namespace: &str| node.xml_qualified(namespace).unwrap();
    let shared = || {
        qualified(
            SchemaNode::group(
                "Shared",
                vec![qualified(
                    SchemaNode::scalar("Token", ScalarType::String),
                    "urn:ferrule:third",
                )],
            ),
            "urn:ferrule:foreign",
        )
    };
    let schema = qualified(
        SchemaNode::group(
            "Root",
            vec![
                qualified(
                    SchemaNode::group("Left", vec![shared()]),
                    "urn:ferrule:root",
                ),
                qualified(
                    SchemaNode::group("Right", vec![shared().repeating()]),
                    "urn:ferrule:root",
                ),
            ],
        ),
        "urn:ferrule:root",
    );

    let set = export_set(&schema, "mapping-source.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 2);
    assert_eq!(set.dependencies[0].filename, "mapping-source-ns1.xsd");
    assert_eq!(set.dependencies[1].filename, "mapping-source-ns2.xsd");
    assert!(
        set.root
            .contains(r#"schemaLocation="mapping-source-ns1.xsd""#)
    );
    assert!(set.root.contains(r#"ref="ns1:Shared""#));
    assert!(
        set.dependencies[0]
            .contents
            .contains(r#"schemaLocation="mapping-source-ns2.xsd""#)
    );
    assert!(set.dependencies[0].contents.contains(r#"ref="ns1:Token""#));

    let dir = std::env::temp_dir().join(format!("ferrule_xsd_export_set_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mapping-source.xsd"), &set.root).unwrap();
    for dependency in &set.dependencies {
        std::fs::write(dir.join(&dependency.filename), &dependency.contents).unwrap();
    }
    let imported = import(&dir.join("mapping-source.xsd")).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_set_supports_qualified_attribute_dependencies() {
    let attribute = SchemaNode::scalar("token", ScalarType::String)
        .attribute()
        .xml_qualified("urn:ferrule:metadata")
        .unwrap();
    let schema = SchemaNode::group("Root", vec![attribute])
        .xml_qualified("urn:ferrule:document")
        .unwrap();

    let set = export_set(&schema, "document.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    assert!(set.root.contains(r#"<xs:attribute ref="ns1:token"/>"#));
    assert!(
        set.dependencies[0]
            .contents
            .contains(r#"<xs:attribute name="token" type="xs:string"/>"#)
    );
}

#[test]
fn export_set_supports_multiple_declarations_in_one_foreign_namespace() {
    let qualified = |node: SchemaNode, namespace: &str| node.xml_qualified(namespace).unwrap();
    let schema = qualified(
        SchemaNode::group(
            "Root",
            vec![
                qualified(
                    SchemaNode::scalar("First", ScalarType::String),
                    "urn:ferrule:shared",
                ),
                qualified(
                    SchemaNode::scalar("Second", ScalarType::Int),
                    "urn:ferrule:shared",
                ),
            ],
        ),
        "urn:ferrule:root",
    );

    let set = export_set(&schema, "shared.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 2);
    assert!(set.root.contains(r#"schemaLocation="shared-ns1.xsd""#));
    assert!(set.root.contains(r#"schemaLocation="shared-ns2.xsd""#));
    assert!(set.root.contains(r#"ref="ns1:First""#));
    assert!(set.root.contains(r#"ref="ns1:Second""#));

    let dir = std::env::temp_dir().join(format!(
        "ferrule_xsd_shared_namespace_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shared.xsd"), &set.root).unwrap();
    for dependency in &set.dependencies {
        std::fs::write(dir.join(&dependency.filename), &dependency.contents).unwrap();
    }
    let imported = import(&dir.join("shared.xsd")).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn export_set_materializes_a_foreign_recursive_occurrence() {
    let recursive = SchemaNode::recursive_group("Emphasis", "Paragraph")
        .xml_qualified("urn:ferrule:markup")
        .unwrap();
    let paragraph = SchemaNode::group(
        "Paragraph",
        vec![
            SchemaNode::scalar("Text", ScalarType::String).xml_unqualified(),
            recursive,
        ],
    )
    .xml_qualified("urn:ferrule:document")
    .unwrap();
    let schema = SchemaNode::group("Document", vec![paragraph])
        .xml_qualified("urn:ferrule:document")
        .unwrap();

    let set = export_set(&schema, "recursive.xsd").unwrap();
    assert_eq!(set.dependencies.len(), 1);
    let dependency = &set.dependencies[0].contents;
    assert!(dependency.contains(r#"<xs:element name="Emphasis""#));
    assert!(dependency.contains(r#"<xs:element ref="tns:Emphasis""#));
    assert!(set.root.contains(r#"ref="ns1:Emphasis""#));
}

#[test]
fn export_set_rejects_conflicts_cycles_limits_and_unsafe_names() {
    let qualified = |node: SchemaNode, namespace: &str| node.xml_qualified(namespace).unwrap();
    let conflict = qualified(
        SchemaNode::group(
            "Root",
            vec![
                SchemaNode::group(
                    "Left",
                    vec![qualified(
                        SchemaNode::scalar("Shared", ScalarType::String),
                        "urn:ferrule:foreign",
                    )],
                ),
                SchemaNode::group(
                    "Right",
                    vec![qualified(
                        SchemaNode::scalar("Shared", ScalarType::Int),
                        "urn:ferrule:foreign",
                    )],
                ),
            ],
        ),
        "urn:ferrule:root",
    );
    assert!(matches!(
        export_set(&conflict, "root.xsd"),
        Err(XmlFormatError::ConflictingNamespaceDeclaration { name, .. })
            if name == "Shared"
    ));

    let cycle = qualified(
        SchemaNode::group(
            "Root",
            vec![qualified(
                SchemaNode::group(
                    "Other",
                    vec![qualified(
                        SchemaNode::scalar("Root", ScalarType::String),
                        "urn:ferrule:root",
                    )],
                ),
                "urn:ferrule:foreign",
            )],
        ),
        "urn:ferrule:root",
    );
    assert!(matches!(
        export_set(&cycle, "root.xsd"),
        Err(XmlFormatError::NamespaceDependencyCycle { name, .. }) if name == "Root"
    ));

    let many = qualified(
        SchemaNode::group(
            "Root",
            (0..=64)
                .map(|index| {
                    qualified(
                        SchemaNode::scalar(format!("Value{index}"), ScalarType::String),
                        &format!("urn:ferrule:namespace:{index}"),
                    )
                })
                .collect(),
        ),
        "urn:ferrule:root",
    );
    assert!(matches!(
        export_set(&many, "root.xsd"),
        Err(XmlFormatError::NamespaceArtifactLimit { limit: 64 })
    ));
    assert!(matches!(
        export_set(&SchemaNode::group("Root", Vec::new()), "../root.xsd"),
        Err(XmlFormatError::InvalidXsdArtifactName { .. })
    ));

    let ambiguous = qualified(
        SchemaNode::group(
            "Root",
            vec![
                qualified(
                    SchemaNode::scalar("Code", ScalarType::String),
                    "urn:ferrule:first",
                ),
                qualified(
                    SchemaNode::scalar("Code", ScalarType::String),
                    "urn:ferrule:second",
                ),
            ],
        ),
        "urn:ferrule:root",
    );
    assert!(matches!(
        export_set(&ambiguous, "root.xsd"),
        Err(XmlFormatError::AmbiguousNamespaceSiblings { name, .. }) if name == "Code"
    ));
}
