use ir::{
    GroupAlternative, ScalarType, SchemaNode, XmlNamespace, primary_root_schema_has_scalar,
    primary_root_schema_is_supported,
};
#[test]
fn exact_scalar_gate_rejects_metadata_valid_terminal_name_aliases() {
    let code = SchemaNode::scalar("Code", ScalarType::String).xml_unqualified();
    let mut root = SchemaNode::group(
        "Root",
        vec![
            code.clone(),
            SchemaNode::scalar("Extra", ScalarType::String),
        ],
    )
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
    root.xml_type_alternatives = true;
    root.xml_default_type = Some("Base".into());
    assert!(root.metadata_is_valid());
    assert!(primary_root_schema_is_supported(&root));
    assert!(primary_root_schema_has_scalar(&root, &["Code"]));
    let ir::SchemaKind::Group { children, .. } = &mut root.kind else {
        panic!("group")
    };
    children[0] = code
        .with_xml_name_alternatives(vec![XmlNamespace::qualified("urn:alias").unwrap()])
        .unwrap();
    assert!(children[0].metadata_is_valid());
    assert!(root.metadata_is_valid());
    assert!(primary_root_schema_is_supported(&root));
    assert!(
        !primary_root_schema_has_scalar(&root, &["Code"]),
        "unproved aliased terminal admitted by exact ordinary scalar gate"
    );
}
