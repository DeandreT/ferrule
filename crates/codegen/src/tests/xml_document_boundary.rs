use super::*;

fn document_project() -> Project {
    let mut project = supported_project();
    project.source = serde_json::from_str(r#"{"name":"Root","xml_namespace":{"kind":"unqualified"},"xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code"]},{"name":"Derived","members":["Code","Extra"]}]}}"#).unwrap();
    project.source_options = mapping::FormatOptions {
        xml_document: true,
        xml_allow_inactive_root_type_members: true,
        xml_root_view_read_policy: true,
        ..Default::default()
    };
    project.target = SchemaNode::group("Output", vec![scalar("Value").attribute()]);
    project.graph.nodes = BTreeMap::from([(
        0,
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: false,
        },
    )]);
    project.root = Scope {
        bindings: vec![MappingBinding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Default::default()
    };
    project
}

#[test]
fn public_observed_xml_document_lowering_carries_literal_output_policy() {
    let mut project = document_project();
    project.target_options = mapping::FormatOptions {
        xml_document: true,
        xml_schema_hints: Some(ir::XmlSchemaHints {
            no_namespace_location: Some("../literal.xsd".into()),
            locations: vec![ir::XmlSchemaLocation {
                namespace: "urn:literal".into(),
                location: "https://literal.invalid/a.xsd".into(),
            }],
        }),
        ..Default::default()
    };
    let program = lower(&project).unwrap();
    let boundary = program.xml_boundary.unwrap();
    assert_eq!(
        boundary.input,
        crate::XmlInputPolicy {
            allow_inactive_root_type_members: true,
            root_view_policy: true,
        }
    );
    assert_eq!(
        boundary.output.schema_hints,
        project.target_options.xml_schema_hints
    );
    assert!(boundary.output.declaration && boundary.output.indent);
    assert!(boundary.output.default_namespace.is_none());
}

#[test]
fn public_xml_document_lowering_keeps_ordinary_hint_guard_and_named_boundaries_closed() {
    let mut ordinary = document_project();
    ordinary.source_options = Default::default();
    assert!(lower(&ordinary).unwrap().xml_boundary.is_none());
    ordinary.target_options.xml_document = true;
    ordinary.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("literal.xsd".into()),
        locations: vec![],
    });
    assert!(
        lower(&ordinary)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("XML schema hints"))
    );
    let mut named = document_project();
    named.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("constant".into()),
        },
    );
    named.extra_targets.push(mapping::NamedTarget {
        name: "another".into(),
        schema: named.target.clone(),
        root: named.root.clone(),
        path: None,
        options: Default::default(),
    });
    assert!(
        lower(&named)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("one primary input and output"))
    );
}

#[test]
fn public_xml_document_lowering_refuses_root_sequences_before_emission() {
    let mut project = document_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("constant".into()),
        },
    );
    project.root.set_source(Some(vec![]));
    let error = lower(&project).unwrap_err();
    assert!(
        error
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("non-iterating primary root")),
        "{error:?}"
    );
}

#[test]
fn public_xml_document_profile_refuses_names_unsupported_by_the_actual_reader_or_hint_parser() {
    let valid = document_project();
    let mut supplementary_input = valid.clone();
    supplementary_input.source.name = "𐀀".into();
    assert!(
        lower(&supplementary_input)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("supplementary XML names"))
    );
    let mut supplementary_output = valid.clone();
    supplementary_output.target.name = "𐀀".into();
    assert!(lower(&supplementary_output).is_ok());
    supplementary_output.target_options.xml_document = true;
    supplementary_output.target_options.xml_schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("literal.xsd".into()),
        locations: vec![],
    });
    assert!(
        lower(&supplementary_output)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("supplementary XML names"))
    );
    let mut colon = valid;
    colon.target.name = "p:Output".into();
    assert!(
        lower(&colon)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("local NCNames"))
    );
}

#[test]
fn public_xml_document_profile_refuses_namespace_declaration_roles_before_artifacts() {
    let mut project = document_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("ordinary".into()),
        },
    );
    project.root.bindings[0].target_field = "xmlns".into();
    if let SchemaKind::Group { children, .. } = &mut project.target.kind {
        children[0].name = "xmlns".into();
    }
    assert!(
        lower(&project)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("namespace declaration metadata"))
    );
    let mut source_uri = document_project();
    source_uri.source.xml_namespace = Some(ir::XmlNamespace::Qualified(
        ir::XmlNamespaceUri::new("urn:has space").unwrap(),
    ));
    assert!(
        lower(&source_uri)
            .unwrap_err()
            .diagnostics()
            .iter()
            .any(|d| d.to_string().contains("namespace is invalid or too large"))
    );
}
