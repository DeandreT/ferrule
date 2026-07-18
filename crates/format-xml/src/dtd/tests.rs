use ir::{Instance, SchemaKind, Value};

use super::*;
use crate::{from_str, to_string};

const SUPPORTED_DTD: &str = r#"
        <!-- self-authored DTD exercising the supported subset -->
        <!ELEMENT Header (#PCDATA)>
        <!ELEMENT Item (#PCDATA)>
        <!ELEMENT Note (#PCDATA)>
        <!ELEMENT Point EMPTY>
        <!ATTLIST Point Lat CDATA #REQUIRED Direction (E|W) #REQUIRED>
        <!ELEMENT Footer (#PCDATA)>
        <!ELEMENT Report (Header,(Item|Note),Point,Point+,Footer?)>
    "#;

#[test]
fn imports_forward_references_choices_attributes_and_repetitions() {
    let schema = import_root_str(SUPPORTED_DTD, Some("Report")).unwrap();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("Report should be a group");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["Header", "Item", "Note", "Point", "Footer"]
    );
    let point = schema.child("Point").unwrap();
    assert!(point.repeating);
    assert!(
        point
            .child("Lat")
            .is_some_and(|attribute| attribute.attribute)
    );
    assert!(
        point
            .child("Direction")
            .is_some_and(|attribute| attribute.attribute)
    );
    assert!(!schema.child("Footer").unwrap().repeating);
}

#[test]
fn named_root_and_first_declared_root_are_distinct() {
    let first = import_root_str(&format!("\u{feff}{SUPPORTED_DTD}"), None).unwrap();
    let report = import_root_str(SUPPORTED_DTD, Some("Report")).unwrap();
    assert_eq!(first.name, "Header");
    assert_eq!(report.name, "Report");
    assert!(matches!(
        import_root_str(SUPPORTED_DTD, Some("Missing")),
        Err(DtdError::MissingRoot(name)) if name == "Missing"
    ));
}

#[test]
fn absent_groups_remain_absent_while_present_empty_groups_round_trip() {
    let schema = import_root_str(
        r#"
                <!ELEMENT Root ((Left|Right),Flag?)>
                <!ELEMENT Left (Value)>
                <!ELEMENT Right (Value)>
                <!ELEMENT Value (#PCDATA)>
                <!ELEMENT Flag EMPTY>
            "#,
        Some("Root"),
    )
    .unwrap();
    let instance = from_str(
        "<Root><Left><Value>selected</Value></Left><Flag/></Root>",
        &schema,
    )
    .unwrap();
    assert!(instance.field("Left").is_some());
    assert!(instance.field("Right").is_none());
    assert_eq!(instance.field("Flag"), Some(&Instance::Group(Vec::new())));

    let xml = to_string(&schema, &instance).unwrap();
    assert!(xml.contains("<Left>"), "{xml}");
    assert!(!xml.contains("<Right>"), "{xml}");
    assert!(xml.contains("<Flag>"), "{xml}");
    assert!(from_str(&xml, &schema).unwrap().field("Flag").is_some());
}

#[test]
fn pcdata_with_attributes_becomes_simple_content_group() {
    let schema = import_root_str(
        "<!ELEMENT Label (#PCDATA)><!ATTLIST Label lang CDATA #REQUIRED>",
        Some("Label"),
    )
    .unwrap();
    let instance = from_str("<Label lang=\"en\">hello</Label>", &schema).unwrap();
    assert_eq!(
        instance.field(XML_TEXT_FIELD).and_then(Instance::as_scalar),
        Some(&Value::String("hello".to_string()))
    );
    assert_eq!(
        instance.field("lang").and_then(Instance::as_scalar),
        Some(&Value::String("en".to_string()))
    );
}

#[test]
fn file_api_imports_self_authored_dtd() {
    let path = std::env::temp_dir().join(format!(
        "ferrule_dtd_import_test_{}.dtd",
        std::process::id()
    ));
    std::fs::write(&path, SUPPORTED_DTD).unwrap();
    let schema = import_root(&path, Some("Report")).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(schema.name, "Report");
}

#[test]
fn rejects_unsupported_or_unrepresentable_content_precisely() {
    let cases = [
        ("<!ELEMENT Root ANY>", "ANY element content"),
        (
            "<!ELEMENT Root (#PCDATA|Child)*><!ELEMENT Child EMPTY>",
            "mixed PCDATA and child-element content",
        ),
        ("<!ENTITY item \"value\"><!ELEMENT Root EMPTY>", "entity"),
        ("<!NOTATION image SYSTEM \"image/png\">", "notation"),
        ("<![INCLUDE[<!ELEMENT Root EMPTY>]]>", "conditional"),
        (
            "<!ELEMENT Root EMPTY><!ATTLIST Root optional CDATA #IMPLIED>",
            "attribute defaults other than #REQUIRED",
        ),
    ];
    for (text, expected) in cases {
        let error = import_root_str(text, Some("Root")).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }

    let tuple = import_root_str(
        "<!ELEMENT Root (A,B)*><!ELEMENT A EMPTY><!ELEMENT B EMPTY>",
        Some("Root"),
    )
    .unwrap_err();
    assert!(matches!(
        tuple,
        DtdError::UnsupportedRepeatingParticle {
            member_count: 2,
            ..
        }
    ));
}

#[test]
fn rejects_unresolved_cycles_duplicates_and_orphan_attributes() {
    assert!(matches!(
        import_root_str("<!ELEMENT Root (Missing)>", Some("Root")),
        Err(DtdError::UnresolvedElement { child, .. }) if child == "Missing"
    ));
    assert!(matches!(
        import_root_str(
            "<!ELEMENT Root (Child)><!ELEMENT Child (Root)>",
            Some("Root")
        ),
        Err(DtdError::RecursiveElement(name)) if name == "Root"
    ));
    assert!(matches!(
        import_root_str("<!ELEMENT Root EMPTY><!ELEMENT Root EMPTY>", Some("Root")),
        Err(DtdError::DuplicateElement(name)) if name == "Root"
    ));
    assert!(matches!(
        import_root_str(
            "<!ATTLIST Missing id CDATA #REQUIRED><!ELEMENT Root EMPTY>",
            Some("Root")
        ),
        Err(DtdError::UndeclaredAttributeOwner(name)) if name == "Missing"
    ));
    assert!(matches!(
        import_root_str(
            "<!ELEMENT Root (Code)><!ELEMENT Code (#PCDATA)><!ATTLIST Root Code CDATA #REQUIRED>",
            Some("Root")
        ),
        Err(DtdError::AttributeElementNameCollision { name, .. }) if name == "Code"
    ));
}

#[test]
fn enforces_input_and_nesting_limits() {
    let oversized = " ".repeat(MAX_INPUT_BYTES + 1);
    assert!(matches!(
        import_root_str(&oversized, None),
        Err(DtdError::InputTooLarge { .. })
    ));

    let nested = format!(
        "<!ELEMENT Root ({}Leaf{})><!ELEMENT Leaf EMPTY>",
        "(".repeat(MAX_NESTING_DEPTH + 1),
        ")".repeat(MAX_NESTING_DEPTH + 1)
    );
    assert!(matches!(
        import_root_str(&nested, Some("Root")),
        Err(DtdError::LimitExceeded {
            kind: "particle nesting depth",
            ..
        })
    ));

    let mut chain = String::new();
    for index in 0..=MAX_NESTING_DEPTH {
        let next = index + 1;
        chain.push_str(&format!("<!ELEMENT N{index} (N{next})>"));
    }
    chain.push_str(&format!("<!ELEMENT N{} EMPTY>", MAX_NESTING_DEPTH + 1));
    assert!(matches!(
        import_root_str(&chain, Some("N0")),
        Err(DtdError::LimitExceeded {
            kind: "schema expansion depth",
            ..
        })
    ));
}
