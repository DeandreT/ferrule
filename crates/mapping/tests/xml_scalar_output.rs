use ir::{
    ScalarType, SchemaNode, XmlDoubleFields, XmlScalarOutputPolicy, XmlScalarOutputUnresolved,
    XmlScalarOutputUnresolvedReason,
};
use mapping::{FormatOptions, Graph, Project, Scope};

fn project() -> Project {
    let mut project = Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group(
            "Envelope",
            vec![
                SchemaNode::scalar("Amount", ScalarType::Float).repeating(),
                SchemaNode::scalar("Label", ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    project.target_options = FormatOptions {
        xml_document: true,
        xml_scalar_output: Some(policy()),
        ..Default::default()
    };
    project
}
fn policy() -> XmlScalarOutputPolicy {
    XmlScalarOutputPolicy::SuppressCastInSubtreeV1 {
        declarations: XmlDoubleFields::new(vec!["Amount".into()]).unwrap(),
    }
}

#[test]
fn complete_declarations_and_placement_are_required() {
    let mut project = project();
    assert!(
        project
            .validate_xml_scalar_output()
            .unwrap()
            .unwrap()
            .preserves("Amount")
    );
    project.target_options.xml_scalar_output =
        Some(XmlScalarOutputPolicy::SuppressCastInSubtreeV1 {
            declarations: XmlDoubleFields::new(vec!["Label".into()]).unwrap(),
        });
    assert!(project.validate_xml_scalar_output().is_err());
    project.target_options.xml_scalar_output = Some(policy());
    project.source_options.xml_scalar_output = Some(policy());
    assert_eq!(
        project.validate_xml_scalar_output().unwrap_err().owner,
        "source"
    );
    project.source_options.xml_scalar_output = None;
    project.target_options.json_document = true;
    assert!(project.validate_xml_scalar_output().is_err());
}

#[test]
fn both_supported_and_unresolved_require_capability_envelopes() {
    for policy in [
        policy(),
        XmlScalarOutputPolicy::SuppressCastInDocumentV1 {},
        XmlScalarOutputPolicy::Unresolved {
            request: XmlScalarOutputUnresolved::new(
                XmlScalarOutputUnresolvedReason::UnsupportedShape,
                "nested target".into(),
            )
            .unwrap(),
        },
    ] {
        let mut project = project();
        project.target_options.xml_scalar_output = Some(policy);
        let encoded = mapping::project_file::encode_pretty(&project).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            envelope["__ferrule_file"]["capabilities"],
            serde_json::json!([mapping::XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY])
        );
        assert_eq!(
            serde_json::to_value(mapping::project_file::decode_str(&encoded).unwrap()).unwrap(),
            serde_json::to_value(&project).unwrap()
        );
        let raw = serde_json::to_string(&project).unwrap();
        assert!(matches!(
            mapping::project_file::decode_str(&raw),
            Err(mapping::FileCodecError::CapabilityRequiresEnvelope { .. })
        ));
    }
}

#[test]
fn invalid_wire_policies_and_declaration_lists_reject() {
    for text in [
        r#"{"mode":"future"}"#,
        r#"{"mode":"suppress_cast_in_subtree_v1","declarations":[]}"#,
        r#"{"mode":"suppress_cast_in_subtree_v1","declarations":["A","A"]}"#,
        r#"{"mode":"suppress_cast_in_subtree_v1","declarations":["B","A"]}"#,
        r#"{"mode":"suppress_cast_in_subtree_v1","declarations":["Amount"],"extra":true}"#,
        r#"{"mode":"unresolved","request":{"reason":"unsupported_shape","detail":""}}"#,
        r#"{"mode":"suppress_cast_in_document_v1","declarations":["Amount"]}"#,
        r#"{"mode":"suppress_cast_in_document_v1","extra":true}"#,
    ] {
        assert!(
            serde_json::from_str::<XmlScalarOutputPolicy>(text).is_err(),
            "{text}"
        );
    }
    assert!(
        XmlDoubleFields::new(vec!["x".repeat(ir::MAX_XML_SCALAR_OUTPUT_NAME_BYTES + 1)]).is_err()
    );
}

#[test]
fn invalid_source_and_named_placements_still_require_the_file_capability() {
    for location in 0..3 {
        let mut project = project();
        let options = project.target_options.clone();
        project.target_options.xml_scalar_output = None;
        match location {
            0 => project.source_options = options,
            1 => project.extra_sources.push(mapping::NamedSource {
                name: "named".into(),
                path: "input.xml".into(),
                schema: project.target.clone(),
                options,
                dynamic_path: None,
            }),
            _ => project.extra_targets.push(mapping::NamedTarget {
                name: "named".into(),
                path: Some("output.xml".into()),
                schema: project.target.clone(),
                options,
                root: Scope::default(),
            }),
        }
        assert!(project.validate_xml_scalar_output().is_err());
        let encoded = mapping::project_file::encode_pretty(&project).unwrap();
        assert!(encoded.contains(mapping::XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY));
        assert!(
            mapping::project_file::decode_str(&serde_json::to_string(&project).unwrap()).is_err()
        );
        let pipeline = mapping::Pipeline {
            main_mapping_path: None,
            stages: vec![mapping::PipelineStage {
                id: "one".into(),
                mapping_path: None,
                project,
                source: mapping::PipelineInput::Host {
                    name: "source".into(),
                },
                extra_sources: vec![],
            }],
        };
        let encoded = mapping::pipeline_file::encode_pretty(&pipeline).unwrap();
        assert!(encoded.contains(mapping::XML_TARGET_SCALAR_OUTPUT_V1_CAPABILITY));
        assert!(mapping::pipeline_file::decode_str(&encoded).is_ok());
        assert!(
            mapping::pipeline_file::decode_str(&serde_json::to_string(&pipeline).unwrap()).is_err()
        );
    }
}

fn document_project() -> Project {
    let mut project = project();
    project.target = SchemaNode::group(
        "Articles",
        vec![
            SchemaNode::group(
                "Article",
                vec![
                    SchemaNode::scalar("Number", ScalarType::Int),
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Price", ScalarType::Float),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    project.root = serde_json::from_value(serde_json::json!({
        "target_field": "",
        "children": [{"target_field": "Article", "source": ["Article"], "bindings": []}]
    }))
    .unwrap();
    project.target_options.xml_scalar_output =
        Some(XmlScalarOutputPolicy::SuppressCastInDocumentV1 {});
    project
}

#[test]
fn document_profile_wire_is_closed_and_round_trips() {
    let policy: XmlScalarOutputPolicy =
        serde_json::from_str(r#"{"mode":"suppress_cast_in_document_v1"}"#).unwrap();
    assert_eq!(policy, XmlScalarOutputPolicy::SuppressCastInDocumentV1 {});
    assert_eq!(
        serde_json::to_string(&policy).unwrap(),
        r#"{"mode":"suppress_cast_in_document_v1"}"#
    );
}

#[test]
fn document_profile_admits_ordinary_nested_scopes_and_plain_elements_only() {
    let project = document_project();
    let preservation = project.validate_xml_scalar_output().unwrap().unwrap();
    assert_eq!(preservation, ir::XmlScalarPreservation::Document);
    assert!(preservation.preserves("Price") && preservation.for_child().is_some());

    let schema_edits: [fn(&mut SchemaNode); 4] = [
        |price| price.nillable = true,
        |price| *price = price.clone().attribute(),
        |price| price.xml_namespace = ir::XmlNamespace::qualified("urn:x"),
        |price| {
            price.kind = ir::SchemaKind::ScalarUnion {
                types: ir::ScalarTypeSet::new([ScalarType::Float, ScalarType::String]).unwrap(),
            }
        },
    ];
    for edit in schema_edits {
        let mut project = document_project();
        let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
            unreachable!()
        };
        let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
            unreachable!()
        };
        edit(&mut children[2]);
        assert!(project.validate_xml_scalar_output().is_err());
    }

    let mut repeated_root = document_project();
    repeated_root.target.repeating = true;
    assert!(repeated_root.validate_xml_scalar_output().is_err());
    let mut optional_root = document_project();
    optional_root.target.xml_optional = true;
    assert!(optional_root.validate_xml_scalar_output().is_err());

    // Per-item output documents use a separate writer route.
    let mut documents = document_project();
    assert!(documents.root.children[0].set_output_path(Some(0)));
    assert!(documents.validate_xml_scalar_output().is_err());

    let mut scalar_scope = document_project();
    scalar_scope.root.children[0].construction = mapping::ScopeConstruction::Scalar { value: 0 };
    assert!(scalar_scope.validate_xml_scalar_output().is_err());

    let mut named = document_project();
    named.extra_targets.push(mapping::NamedTarget {
        name: "named".into(),
        path: Some("output.xml".into()),
        schema: named.target.clone(),
        options: FormatOptions::default(),
        root: Scope::default(),
    });
    assert!(named.validate_xml_scalar_output().is_err());
}
