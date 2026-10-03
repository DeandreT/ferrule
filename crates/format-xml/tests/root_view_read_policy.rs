use format_xml::{XmlFormatError, XmlReadOptions};
use ir::{
    GroupAlternative, Instance, ScalarType, SchemaKind, SchemaNode, Value, XmlNamespace,
    XmlTypeOrigin,
};
fn schema() -> SchemaNode {
    let attr = |name: &str| {
        let mut f = SchemaNode::scalar(name, ScalarType::String).attribute();
        f.xml_namespace = Some(XmlNamespace::Unqualified);
        f
    };
    let mut s = SchemaNode::group("Root", vec![attr("Code"), attr("Extra")])
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
    s.xml_namespace = Some(XmlNamespace::Unqualified);
    s.xml_type_alternatives = true;
    s.xml_default_type = Some("Base".into());
    s
}
fn options() -> XmlReadOptions {
    XmlReadOptions {
        allow_inactive_root_type_members: true,
        root_view_policy: true,
    }
}
fn xml(annotation: &str) -> String {
    format!(
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:p="urn:owned" xsi:type="{annotation}" Code="c" Extra="e"/>"#
    )
}
#[test]
fn padded_local_and_bound_annotations_keep_literal_resolution_and_inactive_fields() {
    for (literal, identity) in [
        ("  Derived  ", "Derived"),
        ("  p:Derived  ", "{urn:owned}Derived"),
    ] {
        let value =
            format_xml::from_str_with_options(&xml(literal), &schema(), &options()).unwrap();
        assert_eq!(
            value.xml_type_origin(),
            Ok(XmlTypeOrigin::ExplicitPadded {
                literal,
                resolved_identity: identity
            })
        );
        assert!(!ir::primary_root_xml_type_equals(Some(&value), "Derived").unwrap());
        assert_eq!(
            ir::primary_root_scalar(Some(&value), &["Extra"]).unwrap(),
            Value::String("e".into())
        );
        assert!(value.field(ir::XML_TYPE_FIELD).is_none());
        assert!(format_xml::from_str(&xml(literal), &schema()).is_err());
    }
}
#[test]
fn xml_normalized_ascii_padding_is_retained_and_never_selected() {
    let text = xml("\tDerived\n");
    let value = format_xml::from_str_with_options(&text, &schema(), &options()).unwrap();
    assert_eq!(
        value.xml_type_origin(),
        Ok(XmlTypeOrigin::ExplicitPadded {
            literal: " Derived ",
            resolved_identity: "Derived"
        })
    );
    assert!(!ir::primary_root_xml_type_equals(Some(&value), "Derived").unwrap());
}
#[test]
fn bound_unknown_names_remain_explicit_and_inactive_without_writer_marker() {
    for (literal, identity) in [("Unknown", "Unknown"), ("p:Unknown", "{urn:owned}Unknown")] {
        let value =
            format_xml::from_str_with_options(&xml(literal), &schema(), &options()).unwrap();
        assert_eq!(
            value.xml_type_origin(),
            Ok(XmlTypeOrigin::Explicit(identity))
        );
        assert!(!ir::primary_root_xml_type_equals(Some(&value), "Derived").unwrap());
        assert!(value.field(ir::XML_TYPE_FIELD).is_none());
        assert!(matches!(
            format_xml::from_str(&xml(literal), &schema()),
            Err(XmlFormatError::UnknownXmlType { .. })
        ));
    }
}
#[test]
fn known_unpadded_type_and_absence_remain_exact() {
    let value = format_xml::from_str_with_options(&xml("Derived"), &schema(), &options()).unwrap();
    assert_eq!(
        value.xml_type_origin(),
        Ok(XmlTypeOrigin::Explicit("Derived"))
    );
    assert!(ir::primary_root_xml_type_equals(Some(&value), "Derived").unwrap());
    let absent =
        format_xml::from_str_with_options("<Root Code=\"c\"/>", &schema(), &options()).unwrap();
    assert_eq!(absent.xml_type_origin(), Ok(XmlTypeOrigin::Absent));
}
#[test]
fn malformed_or_unbound_qnames_are_conservative_typed_refusals() {
    for literal in [
        "missing:Derived",
        " missing:Derived ",
        "De rived",
        "p::Derived",
        ":Derived",
        "p:",
        "\u{2000}Derived\u{2000}",
        "A/B",
    ] {
        assert!(
            matches!(
                format_xml::from_str_with_options(&xml(literal), &schema(), &options()),
                Err(XmlFormatError::InvalidXmlType { .. })
            ),
            "{literal:?}"
        );
    }
    let literal = "A".repeat(4097);
    assert!(matches!(
        format_xml::from_str_with_options(&xml(&literal), &schema(), &options()),
        Err(XmlFormatError::InvalidXmlType { .. })
    ));
}
#[test]
fn true_nil_flat_group_retains_attributes_only_under_explicit_profile() {
    for nil in ["true", "false"] {
        let text = xml("Derived").replace(" Code=", &format!(" xsi:nil=\"{nil}\" Code="));
        let value = format_xml::from_str_with_options(&text, &schema(), &options()).unwrap();
        assert_eq!(
            ir::primary_root_scalar(Some(&value), &["Code"]).unwrap(),
            Value::String("c".into())
        );
        assert!(ir::primary_root_xml_type_equals(Some(&value), "Derived").unwrap());
        if nil == "true" {
            assert!(matches!(
                format_xml::from_str(&text, &schema()),
                Err(XmlFormatError::UnexpectedXmlNil { .. })
            ));
        }
    }
}
#[test]
fn nil_content_and_unmeasured_lexicals_are_conservative_refusals() {
    for nil in ["1", "0", "yes", " true "] {
        let text = xml("Derived").replace(" Code=", &format!(" xsi:nil=\"{nil}\" Code="));
        assert!(matches!(
            format_xml::from_str_with_options(&text, &schema(), &options()),
            Err(XmlFormatError::InvalidXmlNil { .. })
        ));
    }
    for content in ["<Child/>", "text", "\u{2000}"] {
        let text = xml("Derived").replace("/>", &format!(">{content}</Root>"));
        assert!(matches!(
            format_xml::from_str_with_options(&text, &schema(), &options()),
            Err(XmlFormatError::RootViewContent { .. })
        ));
    }
}
#[test]
fn policy_refuses_nonstring_fixed_nested_missingnamespace_or_missing_old_flag() {
    let mut shapes = vec![];
    for change in 0..4 {
        let mut root = schema();
        let SchemaKind::Group { children, .. } = &mut root.kind else {
            panic!()
        };
        match change {
            0 => {
                children[0].kind = SchemaKind::Scalar {
                    ty: ScalarType::Int,
                }
            }
            1 => children[0].fixed = Some("c".into()),
            2 => children[0] = SchemaNode::group("Code", vec![]),
            _ => root.xml_namespace = None,
        };
        shapes.push(root);
    }
    for root in shapes {
        assert!(matches!(
            format_xml::from_str_with_options("<Root/>", &root, &options()),
            Err(XmlFormatError::UnsupportedRootViewReadPolicy { .. })
        ));
    }
    let opts = XmlReadOptions {
        root_view_policy: true,
        ..Default::default()
    };
    assert!(matches!(
        format_xml::from_str_with_options("<Root/>", &schema(), &opts),
        Err(XmlFormatError::UnsupportedRootViewReadPolicy { .. })
    ));
}
#[test]
fn new_profile_does_not_make_nested_group_reads_permissive() {
    let mut root = schema();
    let SchemaKind::Group { children, .. } = &mut root.kind else {
        panic!()
    };
    children.push(SchemaNode::group("Inner", vec![]));
    assert!(matches!(
        format_xml::from_str_with_options("<Root><Inner/></Root>", &root, &options()),
        Err(XmlFormatError::UnsupportedRootViewReadPolicy { .. })
    ));
}
#[test]
fn file_and_text_entry_points_preserve_same_observed_fact() {
    let text = xml(" Derived ");
    let path = std::env::temp_dir().join(format!("ferrule-root-view-{}.xml", std::process::id()));
    std::fs::write(&path, &text).unwrap();
    let read = format_xml::read_with_options(&path, &schema(), &options()).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        read,
        format_xml::from_str_with_options(&text, &schema(), &options()).unwrap()
    );
    assert!(matches!(read, Instance::Group(_)));
}

#[test]
fn root_view_profile_refuses_contradictory_nonutf8_labels_only_when_enabled() {
    for encoding in ["UTF-16", "windows-1252"] {
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"{encoding}\"?>{}",
            xml("Derived").replace("Code=\"c\"", "Code=\"値\"")
        );
        assert!(matches!(
            format_xml::from_str_with_options(&text, &schema(), &options()),
            Err(XmlFormatError::RootViewEncoding)
        ));
        assert!(format_xml::from_str(&text, &schema()).is_ok());
    }
    for text in [
        xml("Derived"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>{}",
            xml("Derived")
        ),
        format!(
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>{}",
            xml("Derived")
        ),
    ] {
        assert!(format_xml::from_str_with_options(&text, &schema(), &options()).is_ok());
    }
}
#[test]
fn expanded_identity_is_bounded_before_namespace_materialization() {
    let text = xml("p:Unknown").replace("urn:owned", &format!("urn:{}", "x".repeat(4096)));
    assert!(matches!(
        format_xml::from_str_with_options(&text, &schema(), &options()),
        Err(XmlFormatError::InvalidXmlType { .. })
    ));
}
