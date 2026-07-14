use super::*;
use ir::SchemaKind;

#[test]
fn imports_utf16_schemas_with_or_without_a_bom() {
    let text = r#"<?xml version="1.0" encoding="UTF-16"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Root" type="xs:string"/>
        </xs:schema>"#;
    for (label, big_endian, bom) in [
        ("le-bom", false, true),
        ("be-bom", true, true),
        ("le-signature", false, false),
        ("be-signature", true, false),
    ] {
        let path = std::env::temp_dir().join(format!(
            "ferrule_xsd_utf16_{label}_{}.xsd",
            std::process::id()
        ));
        let mut bytes = if bom {
            if big_endian {
                vec![0xfe, 0xff]
            } else {
                vec![0xff, 0xfe]
            }
        } else {
            Vec::new()
        };
        for unit in text.encode_utf16() {
            bytes.extend(if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        std::fs::write(&path, bytes).unwrap();

        let schema = import_root(&path, Some("Root")).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(schema.name, "Root", "{label}");
    }
}

#[test]
fn max_occurs_recognizes_arbitrarily_large_non_negative_integers() {
    for value in ["2", "0002", "+2", "4294967296"] {
        assert!(non_negative_integer_exceeds_one(value), "{value}");
    }
    for value in ["", "+", "0", "1", "0001", "-2", "two"] {
        assert!(!non_negative_integer_exceeds_one(value), "{value}");
    }
}

#[test]
fn imports_inline_simple_type_restriction_base() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_inline_simple_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Root"><xs:complexType><xs:sequence>
            <xs:element name="Quantity"><xs:simpleType>
              <xs:restriction base="xs:positiveInteger"/>
            </xs:simpleType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    let schema = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(matches!(
        schema.child("Quantity").map(|child| &child.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
}

#[test]
fn imports_named_derived_complex_types() {
    let path =
        std::env::temp_dir().join(format!("ferrule_xsd_named_type_{}.xsd", std::process::id()));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="Base">
    <xs:sequence><xs:element name="name" type="xs:string"/></xs:sequence>
  </xs:complexType>
  <xs:complexType name="Domestic">
    <xs:complexContent><xs:extension base="Base">
      <xs:sequence><xs:element name="zip" type="xs:integer"/></xs:sequence>
    </xs:extension></xs:complexContent>
  </xs:complexType>
</xs:schema>"#,
    )
    .unwrap();
    let schema = import_type(&path, "Domestic").unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(schema.child("name").is_some());
    assert!(matches!(
        schema.child("zip").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
}

#[test]
fn expanded_type_names_keep_their_namespace_during_resolution() {
    let dir =
        std::env::temp_dir().join(format!("ferrule_xsd_expanded_type_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("root.xsd");
    std::fs::write(
        &root,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:root">
  <xs:import namespace="urn:derived" schemaLocation="derived.xsd"/>
  <xs:complexType name="Domestic">
    <xs:sequence><xs:element name="wrong" type="xs:string"/></xs:sequence>
  </xs:complexType>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("derived.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:derived">
  <xs:complexType name="Domestic">
    <xs:sequence><xs:element name="zip" type="xs:integer"/></xs:sequence>
  </xs:complexType>
</xs:schema>"#,
    )
    .unwrap();

    let schema = import_type(&root, "{urn:derived}Domestic").unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(schema.child("wrong").is_none());
    assert!(matches!(
        schema.child("zip").map(|child| &child.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
}

#[test]
fn imports_nested_repeating_groups() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("ferrule_xsd_test_{}.xsd", std::process::id()));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Orders">
<xs:complexType>
  <xs:sequence>
    <xs:element name="Date" type="xs:date"/>
    <xs:sequence minOccurs="0" maxOccurs="unbounded">
      <xs:element name="Order">
        <xs:complexType>
          <xs:sequence>
            <xs:element name="Order_ID" type="xs:string"/>
            <xs:element name="Items">
              <xs:complexType>
                <xs:sequence maxOccurs="unbounded">
                  <xs:element name="Item">
                    <xs:complexType>
                      <xs:sequence>
                        <xs:element name="Price" type="xs:decimal"/>
                      </xs:sequence>
                    </xs:complexType>
                  </xs:element>
                </xs:sequence>
              </xs:complexType>
            </xs:element>
          </xs:sequence>
        </xs:complexType>
      </xs:element>
    </xs:sequence>
  </xs:sequence>
</xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(schema.name, "Orders");
    assert!(!schema.repeating);

    let date = schema.child("Date").unwrap();
    assert!(!date.repeating);
    assert!(matches!(
        date.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));

    let order = schema.child("Order").unwrap();
    assert!(order.repeating);

    let item = order.child("Items").unwrap().child("Item").unwrap();
    assert!(item.repeating);
    let price = item.child("Price").unwrap();
    assert!(matches!(
        price.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
}

#[test]
fn resolves_top_level_element_refs_and_degrades_cycles() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("ferrule_xsd_ref_test_{}.xsd", std::process::id()));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Company">
<xs:complexType>
  <xs:sequence>
    <xs:element name="Name" type="xs:string"/>
    <xs:element ref="Office" minOccurs="0" maxOccurs="unbounded"/>
  </xs:sequence>
</xs:complexType>
  </xs:element>
  <xs:element name="Office">
<xs:complexType>
  <xs:sequence>
    <xs:element name="City" type="xs:string"/>
    <xs:element ref="Office" minOccurs="0"/>
  </xs:sequence>
</xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import_root(&path, Some("Company")).unwrap();
    std::fs::remove_file(&path).unwrap();

    let office = schema.child("Office").unwrap();
    assert!(office.repeating);
    assert!(matches!(
        office.child("City").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    // The self-reference inside Office degrades to a string scalar.
    assert!(matches!(
        office.child("Office").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
}

#[test]
fn bounds_expansion_of_reused_type_graphs() {
    use std::fmt::Write as _;

    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_bounded_type_graph_{}.xsd",
        std::process::id()
    ));
    let mut xsd = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="T0"><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
  </xs:sequence></xs:complexType>
"#,
    );
    for level in 1..=32 {
        writeln!(
            xsd,
            r#"  <xs:complexType name="T{level}"><xs:sequence>
    <xs:element name="Left" type="T{}"/>
    <xs:element name="Right" type="T{}"/>
  </xs:sequence></xs:complexType>"#,
            level - 1,
            level - 1
        )
        .unwrap();
    }
    xsd.push_str(
        r#"  <xs:element name="Root" type="T32"/>
</xs:schema>"#,
    );
    std::fs::write(&path, xsd).unwrap();

    let result = import_root(&path, Some("Root"));
    std::fs::remove_file(path).unwrap();

    assert!(matches!(
        result,
        Err(XmlFormatError::SchemaMaterializationLimit {
            limit: MAX_MATERIALIZED_SCHEMA_ELEMENTS
        })
    ));
}

#[test]
fn resolves_named_types_extensions_and_choices() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_xsd_named_types_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Order">
<xs:complexType>
  <xs:sequence>
    <xs:element name="Item" type="LineType" minOccurs="0" maxOccurs="unbounded"/>
    <xs:choice>
      <xs:element name="Pickup" type="xs:string"/>
      <xs:element name="Delivery" type="AddressType"/>
    </xs:choice>
    <xs:element name="Priority" type="PriorityType"/>
  </xs:sequence>
</xs:complexType>
  </xs:element>
  <xs:complexType name="LineType">
<xs:complexContent>
  <xs:extension base="BaseLineType">
    <xs:sequence>
      <xs:element name="Qty" type="xs:int"/>
    </xs:sequence>
    <xs:attribute name="unit" type="xs:string"/>
  </xs:extension>
</xs:complexContent>
  </xs:complexType>
  <xs:complexType name="BaseLineType">
<xs:sequence>
  <xs:element name="Sku" type="xs:string"/>
</xs:sequence>
  </xs:complexType>
  <xs:complexType name="AddressType">
<xs:sequence>
  <xs:element name="City" type="xs:string"/>
</xs:sequence>
  </xs:complexType>
  <xs:simpleType name="PriorityType">
<xs:restriction base="xs:integer">
  <xs:maxInclusive value="5"/>
</xs:restriction>
  </xs:simpleType>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    // Named type with a complexContent extension: base children first,
    // then the extension's own element and attribute.
    let item = schema.child("Item").unwrap();
    assert!(item.repeating);
    assert_eq!(
        item.child("Sku").map(|c| c.attribute),
        Some(false),
        "base type child"
    );
    assert!(matches!(
        item.child("Qty").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(item.child("unit").unwrap().attribute);

    // Both choice branches import as children.
    assert!(schema.child("Pickup").is_some());
    assert!(schema.child("Delivery").unwrap().child("City").is_some());

    // Named simpleType resolves to its restriction base.
    assert!(matches!(
        schema.child("Priority").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
}

#[test]
fn resolves_declarations_across_includes_with_cycles() {
    let dir = std::env::temp_dir().join(format!("ferrule_xsd_include_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("main.xsd");
    let shared = dir.join("shared.xsd");

    std::fs::write(
        &shared,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:include schemaLocation="main.xsd"/>
  <xs:complexType name="BaseLineType">
<xs:sequence>
  <xs:element name="Sku" type="xs:string"/>
</xs:sequence>
  </xs:complexType>
  <xs:simpleType name="PriorityType">
<xs:restriction base="xs:integer"/>
  </xs:simpleType>
  <xs:element name="SharedNote" type="xs:string"/>
</xs:schema>
"#,
    )
    .unwrap();
    std::fs::write(
        &main,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:include schemaLocation="shared.xsd"/>
  <xs:element name="Order">
<xs:complexType>
  <xs:sequence>
    <xs:element name="Item" type="LineType" maxOccurs="unbounded"/>
    <xs:element ref="SharedNote"/>
    <xs:element name="Priority" type="PriorityType"/>
    <xs:element name="Unknown" type="MissingType"/>
  </xs:sequence>
</xs:complexType>
  </xs:element>
  <xs:complexType name="LineType">
<xs:complexContent>
  <xs:extension base="BaseLineType">
    <xs:sequence>
      <xs:element name="Qty" type="xs:int"/>
    </xs:sequence>
  </xs:extension>
</xs:complexContent>
  </xs:complexType>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import(&main).unwrap();
    let included_root = import_root(&main, Some("SharedNote")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let item = schema.child("Item").unwrap();
    assert!(item.repeating);
    assert!(item.child("Sku").is_some());
    assert!(matches!(
        item.child("Qty").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(matches!(
        schema.child("SharedNote").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
    assert_eq!(included_root.name, "SharedNote");
    assert!(matches!(
        schema.child("Priority").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    // A declaration missing from an include cycle still degrades instead
    // of recursing forever.
    assert!(matches!(
        schema.child("Unknown").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
}

#[test]
fn resolves_namespace_qualified_imports() {
    let dir = std::env::temp_dir().join(format!("ferrule_xsd_import_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("orders.xsd");
    let shared = dir.join("customers.xsd");

    std::fs::write(
        &shared,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
       xmlns:customer="urn:ferrule:test:customers"
       targetNamespace="urn:ferrule:test:customers">
  <xs:complexType name="CustomerType">
<xs:sequence>
  <xs:element name="Name" type="xs:string"/>
  <xs:element name="Number" type="xs:int"/>
</xs:sequence>
  </xs:complexType>
  <xs:element name="BillingAddress">
<xs:complexType>
  <xs:sequence>
    <xs:element name="City" type="xs:string"/>
  </xs:sequence>
</xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();
    std::fs::write(
        &main,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
       xmlns:customer="urn:ferrule:test:customers"
       targetNamespace="urn:ferrule:test:orders">
  <xs:import namespace="urn:ferrule:test:customers" schemaLocation="customers.xsd"/>
  <xs:element name="Order">
<xs:complexType>
  <xs:sequence>
    <xs:element name="Customer" type="customer:CustomerType"/>
    <xs:element ref="customer:BillingAddress"/>
  </xs:sequence>
</xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import_root(&main, Some("Order")).unwrap();
    let imported_root =
        import_root(&main, Some("{urn:ferrule:test:customers}BillingAddress")).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    assert_eq!(imported_root.name, "BillingAddress");
    assert!(imported_root.child("City").is_some());

    let customer = schema.child("Customer").unwrap();
    assert!(customer.child("Name").is_some());
    assert!(matches!(
        customer.child("Number").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(
        schema
            .child("BillingAddress")
            .unwrap()
            .child("City")
            .is_some()
    );
}

#[test]
fn imports_attributes_as_flagged_scalars() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("ferrule_xsd_attr_test_{}.xsd", std::process::id()));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Books">
<xs:complexType>
  <xs:sequence maxOccurs="unbounded">
    <xs:element name="Book">
      <xs:complexType>
        <xs:sequence>
          <xs:element name="Title" type="xs:string"/>
        </xs:sequence>
        <xs:attribute name="isbn" type="xs:string"/>
        <xs:attribute name="pages" type="xs:int"/>
        <xs:attribute name="draft" type="xs:string" use="prohibited"/>
      </xs:complexType>
    </xs:element>
  </xs:sequence>
  <xs:attribute name="count" type="xs:int"/>
</xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let count = schema.child("count").unwrap();
    assert!(count.attribute);
    assert!(matches!(
        count.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));

    let book = schema.child("Book").unwrap();
    assert!(book.repeating);
    let isbn = book.child("isbn").unwrap();
    assert!(isbn.attribute);
    assert!(book.child("pages").unwrap().attribute);
    assert!(!book.child("Title").unwrap().attribute);
    assert!(book.child("draft").is_none());
}

#[test]
fn imports_simple_content_as_text_plus_attributes() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_simple_content_test_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Catalog">
<xs:complexType>
  <xs:sequence>
    <xs:element name="Price">
      <xs:complexType>
        <xs:simpleContent>
          <xs:extension base="xs:decimal">
            <xs:attribute name="currency" type="xs:string"/>
          </xs:extension>
        </xs:simpleContent>
      </xs:complexType>
    </xs:element>
  </xs:sequence>
</xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();

    let schema = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let price = schema.child("Price").unwrap();
    let text = price.child(XML_TEXT_FIELD).unwrap();
    assert!(text.text);
    assert!(matches!(
        text.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    let currency = price.child("currency").unwrap();
    assert!(currency.attribute);
    assert!(matches!(
        currency.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
}

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
fn export_rejects_group_alternatives_instead_of_flattening_them() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .with_alternatives(vec![
        ir::GroupAlternative {
            name: "first".into(),
            members: vec!["Value".into()],
            required: vec!["Value".into()],
        },
        ir::GroupAlternative {
            name: "second".into(),
            members: Vec::new(),
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
fn export_rejects_multiple_or_mixed_text_fields() {
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
    assert!(matches!(
        export(&mixed),
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
