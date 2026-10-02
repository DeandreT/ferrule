use mapping::Node;
#[test]
fn new_nodes_keep_closed_wire_payloads_and_have_no_dependencies() {
    for wire in [
        r#"{"kind":"source_root_xml_type_equals","canonical_expanded_type":"{urn:t}Derived"}"#,
        r#"{"kind":"source_root_field","path":["Nested","Code"]}"#,
    ] {
        let node: Node = serde_json::from_str(wire).unwrap();
        assert!(node.dependencies().is_empty());
        assert_eq!(serde_json::to_string(&node).unwrap(), wire);
    }
    let legacy: Node = serde_json::from_str(r#"{"kind":"source_field","path":["Code"]}"#).unwrap();
    assert_eq!(
        serde_json::to_string(&legacy).unwrap(),
        r#"{"kind":"source_field","path":["Code"]}"#
    );
}
#[test]
fn malformed_and_oversized_primitive_wire_payloads_reject() {
    for wire in [
        r#"{"kind":"source_root_xml_type_equals","canonical_expanded_type":"{}Derived"}"#,
        r#"{"kind":"source_root_xml_type_equals","canonical_expanded_type":"p:Derived"}"#,
        r#"{"kind":"source_root_field","path":[]}"#,
        r#"{"kind":"source_root_field","path":["element()"]}"#,
    ] {
        assert!(serde_json::from_str::<Node>(wire).is_err());
    }
    let bad = Node::SourceRootXmlTypeEquals {
        canonical_expanded_type: "x".repeat(4097),
    };
    assert!(serde_json::to_string(&bad).is_err());
    let bad = Node::SourceRootField {
        path: vec!["Code".into(); 17],
        required: false,
    };
    assert!(serde_json::to_string(&bad).is_err());
}

#[test]
fn legacy_unknown_wire_members_never_configure_a_root_owner_or_frame() {
    let node:Node=serde_json::from_str(r#"{"kind":"source_root_field","path":["Code"],"frame":["Rows"],"source":"secondary","document":"first.xml"}"#).unwrap();
    assert!(matches!(&node,Node::SourceRootField { path, .. } if path==&["Code"]));
    assert_eq!(
        serde_json::to_string(&node).unwrap(),
        r#"{"kind":"source_root_field","path":["Code"]}"#
    );
}

#[test]
fn required_root_reads_have_an_explicit_boolean_wire_policy() {
    let wire = r#"{"kind":"source_root_field","path":["Code"],"required":true}"#;
    let node: Node = serde_json::from_str(wire).unwrap();
    assert!(matches!(
        &node,
        Node::SourceRootField { required: true, .. }
    ));
    assert_eq!(serde_json::to_string(&node).unwrap(), wire);
    for invalid in ["null", "1", "\"true\""] {
        let wire =
            format!(r#"{{"kind":"source_root_field","path":["Code"],"required":{invalid}}}"#);
        assert!(serde_json::from_str::<Node>(&wire).is_err());
    }
}
