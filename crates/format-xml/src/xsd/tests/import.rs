use super::super::*;
use crate::{from_str, to_string};
use ir::{Instance, SchemaKind, Value, XmlNamespace};

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
fn repeating_compositor_projects_each_named_member_as_a_repetition() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_repeating_compositor_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Report"><xs:complexType>
            <xs:sequence maxOccurs="unbounded">
              <xs:element name="Date" type="xs:string"/>
              <xs:element name="Note" type="xs:string" minOccurs="0"/>
            </xs:sequence>
          </xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();

    let schema = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(schema.child("Date").is_some_and(|child| child.repeating));
    assert!(schema.child("Note").is_some_and(|child| child.repeating));
    assert_eq!(schema.xml_repeating_sequences.len(), 1);

    let instance = from_str(
        "<Report><Date>first</Date><Note>memo</Note><Date>second</Date></Report>",
        &schema,
    )
    .unwrap();
    assert_eq!(
        instance.field("Date"),
        Some(&Instance::Repeated(vec![
            Instance::Scalar(Value::String("first".into())),
            Instance::Scalar(Value::String("second".into())),
        ]))
    );
    assert_eq!(
        instance.field("Note"),
        Some(&Instance::Repeated(vec![Instance::Scalar(Value::String(
            "memo".into()
        ))]))
    );
    let rendered = to_string(&schema, &instance).unwrap();
    assert!(
        rendered.find("<Date>first</Date>") < rendered.find("<Note>memo</Note>")
            && rendered.find("<Note>memo</Note>") < rendered.find("<Date>second</Date>")
    );
    assert_eq!(from_str(&rendered, &schema).unwrap(), instance);

    let ambiguous = Instance::Group(vec![
        (
            "Date".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("first".into())),
                Instance::Scalar(Value::String("second".into())),
            ]),
        ),
        (
            "Note".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::String("memo".into()))]),
        ),
    ]);
    assert!(matches!(
        to_string(&schema, &ambiguous),
        Err(XmlFormatError::AmbiguousRepeatingSequence { .. })
    ));

    let paired = Instance::Group(vec![
        (
            "Date".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("first".into())),
                Instance::Scalar(Value::String("second".into())),
            ]),
        ),
        (
            "Note".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("one".into())),
                Instance::Scalar(Value::String("two".into())),
            ]),
        ),
    ]);
    let paired = to_string(&schema, &paired).unwrap();
    let (Some(first_date), Some(first_note), Some(second_date), Some(second_note)) = (
        paired.find("<Date>first</Date>"),
        paired.find("<Note>one</Note>"),
        paired.find("<Date>second</Date>"),
        paired.find("<Note>two</Note>"),
    ) else {
        panic!("paired sequence output omitted a member: {paired}");
    };
    assert!(first_date < first_note && first_note < second_date && second_date < second_note);

    let ambiguous_single_cycle = Instance::Group(vec![
        (
            "Date".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::String("first".into()))]),
        ),
        (
            "Note".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("one".into())),
                Instance::Scalar(Value::String("two".into())),
            ]),
        ),
    ]);
    assert!(matches!(
        to_string(&schema, &ambiguous_single_cycle),
        Err(XmlFormatError::AmbiguousRepeatingSequence { .. })
    ));
}

#[test]
fn required_repeating_member_reconstructs_one_item_per_outer_cycle() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_required_repeating_member_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Rows"><xs:complexType>
            <xs:sequence maxOccurs="unbounded">
              <xs:element name="Date" type="xs:string"/>
              <xs:element name="Tag" type="xs:string" maxOccurs="unbounded"/>
            </xs:sequence>
          </xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    let schema = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();

    let instance = Instance::Group(vec![
        (
            "Date".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("first".into())),
                Instance::Scalar(Value::String("second".into())),
            ]),
        ),
        (
            "Tag".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("a".into())),
                Instance::Scalar(Value::String("b".into())),
            ]),
        ),
    ]);
    let rendered = to_string(&schema, &instance).unwrap();
    assert!(
        rendered.find("<Date>first</Date>") < rendered.find("<Tag>a</Tag>")
            && rendered.find("<Tag>a</Tag>") < rendered.find("<Date>second</Date>")
            && rendered.find("<Date>second</Date>") < rendered.find("<Tag>b</Tag>")
    );
    assert!(
        export(&schema)
            .unwrap()
            .contains("<xs:element name=\"Tag\" type=\"xs:string\" maxOccurs=\"unbounded\"/>")
    );

    let missing_required = Instance::Group(vec![
        (
            "Date".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::String("first".into()))]),
        ),
        ("Tag".into(), Instance::Repeated(Vec::new())),
    ]);
    assert!(matches!(
        to_string(&schema, &missing_required),
        Err(XmlFormatError::AmbiguousRepeatingSequence { .. })
    ));
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
                xmlns:o="urn:ferrule:orders" targetNamespace="urn:ferrule:orders" elementFormDefault="qualified">
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
                xmlns:o="urn:ferrule:orders" targetNamespace="urn:ferrule:orders" elementFormDefault="qualified">
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
fn imports_one_concrete_type_derived_from_an_abstract_base() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_single_concrete_type_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:single-type" targetNamespace="urn:ferrule:single-type"
                elementFormDefault="qualified">
          <xs:complexType name="AbstractParty" abstract="true"><xs:sequence>
            <xs:element name="id" type="xs:string"/>
          </xs:sequence></xs:complexType>
          <xs:complexType name="Person"><xs:complexContent>
            <xs:extension base="t:AbstractParty"><xs:sequence>
              <xs:element name="displayName" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:element name="Directory"><xs:complexType><xs:sequence>
            <xs:element name="party" type="t:AbstractParty"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();

    let schema = import_root(&path, Some("{urn:ferrule:single-type}Directory")).unwrap();
    std::fs::remove_file(path).unwrap();
    let party = schema.child("party").unwrap();
    assert_eq!(party.alternatives().len(), 1);
    assert_eq!(
        party.alternatives()[0].name,
        "{urn:ferrule:single-type}Person"
    );
    assert_eq!(
        party.alternatives()[0].members,
        vec!["id".to_string(), "displayName".to_string()]
    );

    let input = from_str(
        r#"<Directory xmlns="urn:ferrule:single-type"
                xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                xmlns:t="urn:ferrule:single-type">
          <party xsi:type="t:Person"><id>p-1</id><displayName>Ada</displayName></party>
        </Directory>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(
        input
            .field("party")
            .and_then(|party| party.field(ir::XML_TYPE_FIELD))
            .and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String("{urn:ferrule:single-type}Person".into()))
    );
    let output = crate::to_string(&schema, &input).unwrap();
    assert!(output.contains("xsi:type=\"ft:Person\""), "{output}");
}

#[test]
fn imports_transitive_derived_types_through_an_abstract_intermediate() {
    let dir = std::env::temp_dir().join(format!(
        "ferrule_xsd_transitive_alternatives_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("orders.xsd");
    std::fs::write(
        &root,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:o="urn:ferrule:transitive" targetNamespace="urn:ferrule:transitive" elementFormDefault="qualified">
          <xs:include schemaLocation="address-types.xsd"/>
          <xs:element name="Order"><xs:complexType><xs:sequence>
            <xs:element name="shipTo" type="o:Address"/>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("address-types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:o="urn:ferrule:transitive" targetNamespace="urn:ferrule:transitive" elementFormDefault="qualified">
          <xs:complexType name="Address" abstract="true"><xs:sequence>
            <xs:element name="name" type="xs:string"/>
          </xs:sequence></xs:complexType>
          <xs:complexType name="RegionalAddress" abstract="true"><xs:complexContent>
            <xs:extension base="o:Address"><xs:sequence>
              <xs:element name="region" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:complexType name="DomesticAddress"><xs:complexContent>
            <xs:extension base="o:RegionalAddress"><xs:sequence>
              <xs:element name="state" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:complexType name="InternationalAddress"><xs:complexContent>
            <xs:extension base="o:RegionalAddress"><xs:sequence>
              <xs:element name="postcode" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
        </xs:schema>"#,
    )
    .unwrap();

    let schema = import_root(&root, Some("{urn:ferrule:transitive}Order")).unwrap();
    let ship_to = schema.child("shipTo").unwrap();
    assert_eq!(
        ship_to
            .alternatives()
            .iter()
            .map(|alternative| alternative.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "{urn:ferrule:transitive}DomesticAddress",
            "{urn:ferrule:transitive}InternationalAddress",
        ]
    );
    assert!(ship_to.child("region").is_some());
    assert!(ship_to.child("state").is_some());
    assert!(ship_to.child("postcode").is_some());

    let domestic = from_str(
        r#"<Order xmlns="urn:ferrule:transitive"
                xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                xmlns:o="urn:ferrule:transitive">
          <shipTo xsi:type="o:DomesticAddress"><name>Ada</name><region>west</region><state>WA</state></shipTo>
        </Order>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(
        domestic
            .field("shipTo")
            .and_then(|address| address.field(ir::XML_TYPE_FIELD))
            .and_then(ir::Instance::as_scalar),
        Some(&ir::Value::String(
            "{urn:ferrule:transitive}DomesticAddress".into()
        ))
    );
    assert!(matches!(
        from_str(
            r#"<Order xmlns="urn:ferrule:transitive"
                    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                    xmlns:o="urn:ferrule:transitive">
              <shipTo xsi:type="o:DomesticAddress"><name>Ada</name><region>west</region><postcode>AB12</postcode></shipTo>
            </Order>"#,
            &schema,
        ),
        Err(crate::XmlFormatError::NoMatchingAlternative { .. })
    ));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cyclic_type_derivations_fail_closed_without_recursing_forever() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_cyclic_derivations_{}.xsd",
        std::process::id()
    ));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:cycle" targetNamespace="urn:ferrule:cycle">
          <xs:complexType name="A"><xs:complexContent>
            <xs:extension base="t:B"><xs:sequence>
              <xs:element name="AValue" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:complexType name="B"><xs:complexContent>
            <xs:extension base="t:A"><xs:sequence>
              <xs:element name="BValue" type="xs:string"/>
            </xs:sequence></xs:extension>
          </xs:complexContent></xs:complexType>
          <xs:element name="Root" type="t:A"/>
        </xs:schema>"#,
    )
    .unwrap();

    let schema = import_root(&path, Some("{urn:ferrule:cycle}Root")).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(schema.alternatives().is_empty());
}

#[test]
fn conflicting_derived_type_identities_leave_the_base_view_unchanged() {
    let dir = std::env::temp_dir().join(format!(
        "ferrule_xsd_conflicting_derivations_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("root.xsd");
    std::fs::write(
        &root,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:t="urn:ferrule:conflict" targetNamespace="urn:ferrule:conflict">
          <xs:include schemaLocation="first.xsd"/>
          <xs:include schemaLocation="second.xsd"/>
          <xs:complexType name="Base"><xs:sequence>
            <xs:element name="Value" type="xs:string"/>
          </xs:sequence></xs:complexType>
          <xs:element name="Root" type="t:Base"/>
        </xs:schema>"#,
    )
    .unwrap();
    for (file, field) in [("first.xsd", "First"), ("second.xsd", "Second")] {
        std::fs::write(
            dir.join(file),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                        xmlns:t="urn:ferrule:conflict" targetNamespace="urn:ferrule:conflict">
                  <xs:complexType name="Derived"><xs:complexContent>
                    <xs:extension base="t:Base"><xs:sequence>
                      <xs:element name="{field}" type="xs:string"/>
                    </xs:sequence></xs:extension>
                  </xs:complexContent></xs:complexType>
                </xs:schema>"#
            ),
        )
        .unwrap();
    }

    let schema = import_root(&root, Some("{urn:ferrule:conflict}Root")).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(schema.alternatives().is_empty());
    assert!(schema.child("Value").is_some());
    assert!(schema.child("First").is_none());
    assert!(schema.child("Second").is_none());
}

#[test]
fn bounds_derived_type_discovery_with_the_schema_materialization_limit() {
    use std::fmt::Write as _;

    let path = std::env::temp_dir().join(format!(
        "ferrule_xsd_bounded_derivations_{}.xsd",
        std::process::id()
    ));
    let mut xsd = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:complexType name="Base"><xs:sequence>
            <xs:element name="Value" type="xs:string"/>
          </xs:sequence></xs:complexType>
"#,
    );
    for index in 0..=MAX_MATERIALIZED_SCHEMA_ELEMENTS {
        writeln!(
            xsd,
            r#"  <xs:complexType name="Derived{index}"><xs:complexContent><xs:extension base="Base"/></xs:complexContent></xs:complexType>"#
        )
        .unwrap();
    }
    xsd.push_str("  <xs:element name=\"Root\" type=\"Base\"/>\n</xs:schema>");
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

#[test]
fn fixed_elements_simple_content_and_attributes_roundtrip_and_validate() {
    let path =
        std::env::temp_dir().join(format!("ferrule_xsd_fixed_test_{}.xsd", std::process::id()));
    std::fs::write(
        &path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Root"><xs:complexType><xs:sequence>
    <xs:element name="Mode" type="xs:string" fixed="prod"/>
    <xs:element name="Price" type="xs:decimal" fixed="2.0"/>
    <xs:element name="Label" fixed="ready"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:string">
        <xs:attribute name="language" type="xs:string" fixed="en"/>
      </xs:extension>
    </xs:simpleContent></xs:complexType></xs:element>
  </xs:sequence><xs:attribute name="version" type="xs:int" fixed="2"/>
  </xs:complexType></xs:element>
</xs:schema>"#,
    )
    .unwrap();

    let schema = import(&path).unwrap();
    assert_eq!(
        schema.child("Mode").and_then(|node| node.fixed.as_deref()),
        Some("prod")
    );
    let label = schema.child("Label").unwrap();
    assert_eq!(
        label.text_child().and_then(|node| node.fixed.as_deref()),
        Some("ready")
    );
    assert_eq!(
        label
            .child("language")
            .and_then(|node| node.fixed.as_deref()),
        Some("en")
    );
    assert_eq!(
        schema
            .child("version")
            .and_then(|node| node.fixed.as_deref()),
        Some("2")
    );

    let mut instance = from_str(
        r#"<Root version="2"><Mode>prod</Mode><Price>2.0</Price><Label language="en">ready</Label></Root>"#,
        &schema,
    )
    .unwrap();
    let output = to_string(&schema, &instance).unwrap();
    assert_eq!(from_str(&output, &schema).unwrap(), instance);
    assert!(matches!(
        from_str(
            r#"<Root version="3"><Mode>prod</Mode><Price>2.0</Price><Label language="en">ready</Label></Root>"#,
            &schema,
        ),
        Err(XmlFormatError::FixedValue { name, .. }) if name == "version"
    ));
    assert!(matches!(
        from_str(
            r#"<Root version="2"><Mode>test</Mode><Price>2.0</Price><Label language="en">ready</Label></Root>"#,
            &schema,
        ),
        Err(XmlFormatError::FixedValue { name, .. }) if name == "Mode"
    ));

    let Instance::Group(fields) = &mut instance else {
        panic!("root should be a group")
    };
    let Some((_, Instance::Scalar(mode))) = fields.iter_mut().find(|(name, _)| name == "Mode")
    else {
        panic!("mode should be a scalar")
    };
    *mode = Value::String("test".into());
    assert!(matches!(
        to_string(&schema, &instance),
        Err(XmlFormatError::FixedValue { name, .. }) if name == "Mode"
    ));

    let exported = export(&schema).unwrap();
    assert!(exported.contains(r#"name="Mode" type="xs:string" fixed="prod""#));
    assert!(exported.contains(r#"name="Price" type="xs:decimal" fixed="2.0""#));
    assert!(exported.contains(r#"name="Label" fixed="ready""#));
    assert!(exported.contains(r#"name="language" type="xs:string" fixed="en""#));
    std::fs::write(&path, exported).unwrap();
    assert_eq!(import(&path).unwrap(), schema);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn imports_cross_namespace_element_and_attribute_references_exactly() {
    let dir =
        std::env::temp_dir().join(format!("ferrule_xsd_expanded_names_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("document.xsd");
    let metadata = dir.join("metadata.xsd");
    std::fs::write(
        &metadata,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:metadata"><xs:element name="Label" type="xs:string"/><xs:attribute name="token" type="xs:string"/></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        &main,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:m="urn:ferrule:metadata" targetNamespace="urn:ferrule:document" elementFormDefault="qualified"><xs:import namespace="urn:ferrule:metadata" schemaLocation="metadata.xsd"/><xs:element name="Root"><xs:complexType><xs:sequence><xs:element ref="m:Label"/><xs:element name="Local" type="xs:string" form="unqualified"/></xs:sequence><xs:attribute ref="m:token"/></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();

    let schema = import(&main).unwrap();
    assert!(matches!(
        schema.xml_namespace,
        Some(XmlNamespace::Qualified(ref namespace))
            if namespace.as_str() == "urn:ferrule:document"
    ));
    assert!(matches!(
        schema.child("Label").and_then(|node| node.xml_namespace.as_ref()),
        Some(XmlNamespace::Qualified(namespace))
            if namespace.as_str() == "urn:ferrule:metadata"
    ));
    assert!(matches!(
        schema
            .child("Local")
            .and_then(|node| node.xml_namespace.as_ref()),
        Some(XmlNamespace::Unqualified)
    ));
    assert!(matches!(
        schema.child("token").and_then(|node| node.xml_namespace.as_ref()),
        Some(XmlNamespace::Qualified(namespace))
            if namespace.as_str() == "urn:ferrule:metadata"
    ));

    let instance = from_str(
        r#"<Root xmlns="urn:ferrule:document" xmlns:m="urn:ferrule:metadata" m:token="T"><m:Label>cross</m:Label><Local xmlns="">plain</Local></Root>"#,
        &schema,
    )
    .unwrap();
    assert_eq!(
        instance.field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("cross".into()))
    );
    let rendered = to_string(&schema, &instance).unwrap();
    assert_eq!(from_str(&rendered, &schema).unwrap(), instance);
    assert!(matches!(
        export(&schema),
        Err(XmlFormatError::UnsupportedNamespaceExport { node, .. }) if node == "Label"
    ));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_sibling_expanded_names_that_share_one_local_mapping_name() {
    let dir = std::env::temp_dir().join(format!(
        "ferrule_xsd_namespace_collision_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("first.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:first"><xs:element name="Code" type="xs:string"/></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("second.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:ferrule:second"><xs:element name="Code" type="xs:string"/></xs:schema>"#,
    )
    .unwrap();
    let root = dir.join("root.xsd");
    std::fs::write(
        &root,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:a="urn:ferrule:first" xmlns:b="urn:ferrule:second" targetNamespace="urn:ferrule:root"><xs:import namespace="urn:ferrule:first" schemaLocation="first.xsd"/><xs:import namespace="urn:ferrule:second" schemaLocation="second.xsd"/><xs:element name="Root"><xs:complexType><xs:sequence><xs:element ref="a:Code"/><xs:element ref="b:Code"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();

    assert!(matches!(
        import(&root),
        Err(XmlFormatError::AmbiguousNamespaceSiblings { group, name, .. })
            if group == "Root" && name == "Code"
    ));
    std::fs::remove_dir_all(dir).unwrap();
}
