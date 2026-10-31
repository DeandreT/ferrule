use ir::{
    ScalarType, SchemaNode, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD, XML_NAMESPACE_URI_FIELD,
    XmlExpandedName, XmlNamespace, XmlWildcardExcludedNames, XmlWildcardNamespaceConstraint,
    XmlWildcardNamespaceList,
};

use crate::{
    ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions, FileCodecError, FunctionId,
    Graph, HttpTimeoutSeconds, NamedSource, NamedTarget, Node, OwnedSourceRef, OwnedXmlSerialize,
    Project, Scope, SourceOwner, UserFunction, XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY, project_file,
};

const OWNER: &str = "urn:ferrule:wx:t";

fn qualified(uri: &str) -> XmlNamespace {
    XmlNamespace::qualified(uri).unwrap()
}

fn wildcard(constraint: XmlWildcardNamespaceConstraint) -> SchemaNode {
    SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap()
}

/// `notNamespace` alone, without excluded names.
fn not_only() -> SchemaNode {
    wildcard(XmlWildcardNamespaceConstraint::Not {
        namespaces: XmlWildcardNamespaceList::new([qualified(OWNER), XmlNamespace::Unqualified])
            .unwrap(),
    })
}

/// `notQName` alone, under an ordinary `##any` constraint.
fn names_only() -> SchemaNode {
    wildcard(XmlWildcardNamespaceConstraint::Any)
        .with_xml_wildcard_excluded_names(
            XmlWildcardExcludedNames::new([XmlExpandedName::new(
                qualified("urn:ferrule:wx:a"),
                "Secret",
            )
            .unwrap()])
            .unwrap(),
        )
        .unwrap()
}

fn root(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Root", children)
}

fn project() -> Project {
    Project {
        source: root(vec![]),
        target: root(vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn serialize_node(schema: SchemaNode) -> Node {
    Node::XmlSerialize {
        path: vec!["Root".into()],
        frame: None,
        schema: Box::new(schema),
        declaration: false,
        indent: false,
        namespace: None,
    }
}

/// Every schema the project file stores, each holding one exclusion.
fn placements(wildcard: &SchemaNode) -> Vec<Project> {
    let tree = root(vec![wildcard.clone()]);
    let mut placements = Vec::new();

    let mut source = project();
    source.source = tree.clone();
    placements.push(source);

    let mut target = project();
    target.target = tree.clone();
    placements.push(target);

    let mut named_source = project();
    named_source.extra_sources.push(NamedSource {
        name: "named".into(),
        path: "named.xml".into(),
        schema: tree.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    placements.push(named_source);

    let mut named_target = project();
    named_target.extra_targets.push(NamedTarget {
        name: "named".into(),
        path: Some("named.xml".into()),
        schema: tree.clone(),
        options: Default::default(),
        root: Scope::default(),
    });
    placements.push(named_target);

    let mut nested = project();
    let mut dynamic = root(vec![]);
    if let ir::SchemaKind::Group { dynamic: slot, .. } = &mut dynamic.kind {
        *slot = Some(Box::new(tree.clone()));
    }
    nested.target = root(vec![dynamic]);
    placements.push(nested);

    let mut serializer = project();
    serializer
        .graph
        .nodes
        .insert(1, serialize_node(tree.clone()));
    placements.push(serializer);

    let mut owned = project();
    owned.graph.nodes.insert(
        1,
        Node::OwnedXmlSerialize(OwnedXmlSerialize {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Root".into()],
            },
            frame: None,
            schema: Box::new(tree.clone()),
            declaration: false,
            indent: false,
            namespace: None,
        }),
    );
    placements.push(owned);

    let mut function = project();
    function.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "local".into(),
            name: "Serialize".into(),
            description: None,
            parameters: vec![],
            output_name: "Value".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: [(1, serialize_node(tree.clone()))].into(),
            },
            output: 1,
        },
    );
    placements.push(function);

    let mut request = project();
    request.source_options.external_source = Some(
        ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::new(40).unwrap(),
            Some(ExternalPayloadFormat::Xml),
            Some(tree),
            ExternalPayloadFormat::Xml,
            vec![],
        )
        .unwrap(),
    );
    placements.push(request);

    placements
}

#[test]
fn every_stored_schema_placement_requires_the_capability() {
    for wildcard in [not_only(), names_only()] {
        for (index, project) in placements(&wildcard).into_iter().enumerate() {
            assert!(project.uses_xml_wildcard_exclusions(), "placement {index}");
            let encoded = project_file::encode_pretty(&project).unwrap();
            let envelope: serde_json::Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(
                envelope["__ferrule_file"]["capabilities"],
                serde_json::json!([XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY]),
                "placement {index}"
            );
            assert_eq!(
                serde_json::to_value(project_file::decode_str(&encoded).unwrap()).unwrap(),
                serde_json::to_value(&project).unwrap(),
                "placement {index}"
            );
        }
    }
}

#[test]
fn readers_refuse_exclusions_outside_a_matching_envelope() {
    let mut project = project();
    project.target = root(vec![names_only()]);
    let encoded = project_file::encode_pretty(&project).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(saved["__ferrule_file"]["version"], 3);

    // Readers that predate this capability know only these names, so they
    // report it as unknown instead of dropping the names and widening the
    // wildcard.
    assert!(
        !["sqlite_read_v1", "xml-target-scalar-output-v1"]
            .contains(&XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY)
    );
    let mut unknown = saved.clone();
    unknown["__ferrule_file"]["capabilities"] = serde_json::json!(["xml-wildcard-exclusions-v2"]);
    assert!(matches!(
        project_file::decode_str(&unknown.to_string()),
        Err(FileCodecError::UnknownCapability { .. })
    ));

    let unmarked = serde_json::to_string(&project).unwrap();
    assert!(matches!(
        project_file::decode_str(&unmarked),
        Err(FileCodecError::CapabilityRequiresEnvelope {
            capability: XML_WILDCARD_EXCLUSIONS_V1_CAPABILITY
        })
    ));
    let mut plain = saved.clone();
    plain["__ferrule_file"]["version"] = 2.into();
    plain["__ferrule_file"]
        .as_object_mut()
        .unwrap()
        .remove("capabilities");
    assert!(matches!(
        project_file::decode_str(&plain.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
    let mut stale = saved;
    stale["document"]["target"]["kind"]["children"][0]
        .as_object_mut()
        .unwrap()
        .remove("xml_wildcard_excluded_names");
    assert!(matches!(
        project_file::decode_str(&stale.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
}

#[test]
fn existing_wildcard_constraints_keep_the_plain_envelope() {
    for constraint in [
        XmlWildcardNamespaceConstraint::Any,
        XmlWildcardNamespaceConstraint::Other {
            target_namespace: ir::XmlNamespaceUri::new(OWNER),
        },
        XmlWildcardNamespaceConstraint::list([XmlNamespace::Unqualified]).unwrap(),
    ] {
        let mut project = project();
        project.target = root(vec![wildcard(constraint)]);
        assert!(!project.uses_xml_wildcard_exclusions());
        let encoded = project_file::encode_pretty(&project).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert!(saved.pointer("/__ferrule_file/capabilities").is_none());
        assert_ne!(saved["__ferrule_file"]["version"], 3);
        assert_eq!(
            serde_json::to_value(project_file::decode_str(&encoded).unwrap()).unwrap(),
            serde_json::to_value(&project).unwrap()
        );
    }
}
