use super::super::*;
use crate::from_str;
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
    std::fs::remove_file(&path).unwrap();
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
fn imported_derived_types_select_xsi_type_across_an_include() {
    let dir = std::env::temp_dir().join(format!(
        "ferrule_xsd_included_alternatives_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("orders.xsd");
    std::fs::write(
        &root,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:o="urn:ferrule:orders" targetNamespace="urn:ferrule:orders">
          <xs:include schemaLocation="addresses.xsd"/>
          <xs:element name="Order"><xs:complexType><xs:sequence>
            <xs:element name="shipTo" type="o:Address"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("addresses.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:o="urn:ferrule:orders" targetNamespace="urn:ferrule:orders">
          <xs:complexType name="Address"><xs:sequence>
            <xs:element name="name" type="xs:string"/>
          </xs:sequence></xs:complexType>
          <xs:complexType name="Domestic"><xs:complexContent>
            <xs:extension base="o:Address"><xs:sequence>
              <xs:element name="state" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:complexType name="International"><xs:complexContent>
            <xs:extension base="o:Address"><xs:sequence>
              <xs:element name="postcode" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
        </xs:schema>"#,
    )
    .unwrap();

    let schema = import_root(&root, Some("{urn:ferrule:orders}Order")).unwrap();
    let ship_to = schema.child("shipTo").unwrap();
    assert_eq!(
        ship_to
            .alternatives()
            .iter()
            .map(|alternative| alternative.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "{urn:ferrule:orders}Address",
            "{urn:ferrule:orders}Domestic",
            "{urn:ferrule:orders}International",
        ]
    );
    let international = from_str(
        r#"<Order xmlns="urn:ferrule:orders"
                xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                xmlns:o="urn:ferrule:orders">
          <shipTo xsi:type="o:International"><name>Ada</name><postcode>AB12</postcode></shipTo>
        </Order>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(
        international
            .field("shipTo")
            .and_then(|address| address.field(ir::XML_TYPE_FIELD))
            .and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String(
            "{urn:ferrule:orders}International".into()
        ))
    );
    let base = from_str(
        r#"<Order xmlns="urn:ferrule:orders"><shipTo><name>Ada</name></shipTo></Order>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(
        base.field("shipTo")
            .and_then(|address| address.field(ir::XML_TYPE_FIELD))
            .and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String("{urn:ferrule:orders}Address".into()))
    );
    assert!(matches!(
        from_str(
            r#"<Order xmlns="urn:ferrule:orders" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><shipTo xsi:type="Missing"><name>Ada</name></shipTo></Order>"#,
            &schema,
        ),
        Err(crate::XmlFormatError::UnknownXmlType { .. })
    ));
    assert!(matches!(
        from_str(
            r#"<Order xmlns="urn:ferrule:orders" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:o="urn:ferrule:orders"><shipTo xsi:type="o:Domestic"><name>Ada</name><postcode>AB12</postcode></shipTo></Order>"#,
            &schema,
        ),
        Err(crate::XmlFormatError::NoMatchingAlternative { .. })
    ));
    std::fs::remove_dir_all(dir).unwrap();
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
fn resolves_top_level_element_refs_and_retains_recursive_cycles() {
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
    let recursive = office.child("Office").unwrap();
    assert_eq!(recursive.recursive_ref.as_deref(), Some("Office"));
    assert!(matches!(recursive.kind, SchemaKind::Group { .. }));
}

#[test]
fn recursive_named_types_anchor_to_their_concrete_element() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_named_recursion_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Page"><xs:complexType><xs:sequence>
    <xs:element name="MainSection" type="SectionType"/>
  </xs:sequence></xs:complexType></xs:element>
  <xs:complexType name="SectionType" mixed="true"><xs:choice minOccurs="0" maxOccurs="unbounded">
    <xs:element name="Trademark" type="xs:string"/>
    <xs:element name="SubSection" type="SectionType"/>
  </xs:choice></xs:complexType>
</xs:schema>"#,
    )
    .unwrap();

    let schema = import_root(&path, Some("Page")).unwrap();
    std::fs::remove_file(&path).unwrap();
    let main = schema.child("MainSection").unwrap();
    let subsection = main.child("SubSection").unwrap();
    assert_eq!(subsection.recursive_ref.as_deref(), Some("MainSection"));

    let instance = from_str(
        "<Page><MainSection>intro<SubSection><Trademark>Ferrule</Trademark></SubSection></MainSection></Page>",
        &schema,
    )
    .unwrap();
    let nested = instance
        .field("MainSection")
        .and_then(|section| section.field("SubSection"))
        .and_then(ir::Instance::as_repeated)
        .unwrap();
    assert_eq!(
        nested[0]
            .field("Trademark")
            .and_then(ir::Instance::as_repeated)
            .and_then(|values| values.first())
            .and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String("Ferrule".into()))
    );

    let exported = export(&schema).unwrap();
    assert!(
        exported.contains(r#"<xs:complexType name="MainSectionType" mixed="true">"#),
        "{exported}"
    );
    assert!(
        exported.contains(r#"<xs:element name="SubSection" type="MainSectionType" minOccurs="0" maxOccurs="unbounded"/>"#),
        "{exported}"
    );
    std::fs::write(&path, exported).unwrap();
    let roundtripped = import_root(&path, Some("Page")).unwrap();
    std::fs::remove_file(path).unwrap();
    let roundtripped_main = roundtripped.child("MainSection").unwrap();
    assert_eq!(
        roundtripped_main
            .child("SubSection")
            .and_then(|node| node.recursive_ref.as_deref()),
        Some("MainSection")
    );
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
