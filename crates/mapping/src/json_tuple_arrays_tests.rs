use ir::{ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode};

use crate::{
    ExternalHttpMode, ExternalPayloadFormat, ExternalSourceOptions, FileCodecError, FunctionId,
    Graph, HttpTimeoutSeconds, JSON_TUPLE_ARRAYS_V1_CAPABILITY, NamedSource, NamedTarget, Node,
    OwnedSourceRef, OwnedXmlSerialize, Project, Scope, SourceOwner, UserFunction, project_file,
};

/// A closed `[string, integer]` tuple of length 1..2.
fn frame() -> SchemaNode {
    SchemaNode::group(
        "point",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    )
    .with_json_tuple(JsonTupleLayout {
        length: ItemCountRange::new(1, Some(2)),
        tail: JsonTupleTail::Closed,
    })
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

/// Every schema the project file stores, each holding one frame.
fn placements() -> Vec<Project> {
    let tree = root(vec![frame()]);
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
        path: "named.json".into(),
        schema: tree.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    placements.push(named_source);

    let mut named_target = project();
    named_target.extra_targets.push(NamedTarget {
        name: "named".into(),
        path: Some("named.json".into()),
        schema: tree.clone(),
        options: Default::default(),
        root: Scope::default(),
    });
    placements.push(named_target);

    let mut nested = project();
    let mut dynamic = root(vec![]);
    assert!(dynamic.set_dynamic_fields(Some(tree.clone())));
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
            Some(ExternalPayloadFormat::Json),
            Some(tree),
            ExternalPayloadFormat::Json,
            vec![],
        )
        .unwrap(),
    );
    placements.push(request);

    placements
}

#[test]
fn every_stored_schema_placement_requires_the_capability() {
    for (index, project) in placements().into_iter().enumerate() {
        assert!(project.uses_json_tuple_arrays(), "placement {index}");
        let encoded = project_file::encode_pretty(&project).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            envelope["__ferrule_file"]["version"], 3,
            "placement {index}"
        );
        assert_eq!(
            envelope["__ferrule_file"]["capabilities"],
            serde_json::json!([JSON_TUPLE_ARRAYS_V1_CAPABILITY]),
            "placement {index}"
        );
        assert_eq!(
            serde_json::to_value(project_file::decode_str(&encoded).unwrap()).unwrap(),
            serde_json::to_value(&project).unwrap(),
            "placement {index}"
        );
    }
}

#[test]
fn readers_refuse_frames_outside_a_matching_envelope() {
    let mut project = project();
    project.target = root(vec![frame()]);
    let encoded = project_file::encode_pretty(&project).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&encoded).unwrap();

    // Readers that predate this capability report it as unknown instead of
    // dropping the layout and reading each tuple as an object.
    let mut unknown = saved.clone();
    unknown["__ferrule_file"]["capabilities"] = serde_json::json!(["json_tuple_arrays_v2"]);
    assert!(matches!(
        project_file::decode_str(&unknown.to_string()),
        Err(FileCodecError::UnknownCapability { .. })
    ));

    let unmarked = serde_json::to_string(&project).unwrap();
    assert!(matches!(
        project_file::decode_str(&unmarked),
        Err(FileCodecError::CapabilityRequiresEnvelope {
            capability: JSON_TUPLE_ARRAYS_V1_CAPABILITY
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
    let mut stale = saved.clone();
    stale["document"]["target"]["kind"]["children"][0]
        .as_object_mut()
        .unwrap()
        .remove("json_tuple");
    assert!(matches!(
        project_file::decode_str(&stale.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
    let mut malformed = saved;
    malformed["document"]["target"]["kind"]["children"][0]["json_tuple"]["tail"] = "open".into();
    assert!(matches!(
        project_file::decode_str(&malformed.to_string()),
        Err(FileCodecError::Deserialization(_))
    ));
}

#[test]
fn schemas_without_frames_keep_the_plain_envelope() {
    let mut project = project();
    project.target = root(vec![
        SchemaNode::scalar("item1", ScalarType::String),
        SchemaNode::scalar("rest", ScalarType::Int).repeating(),
    ]);
    assert!(!project.uses_json_tuple_arrays());
    let encoded = project_file::encode_pretty(&project).unwrap();
    assert!(!encoded.contains("json_tuple"));
    assert!(!encoded.contains("__ferrule_file"));
    assert_eq!(
        serde_json::to_value(project_file::decode_str(&encoded).unwrap()).unwrap(),
        serde_json::to_value(&project).unwrap()
    );
}
