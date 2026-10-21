//! Emission coverage for the explicit owned XML serialization companion.

use super::*;
use ::mapping::{
    Binding, NamedSource, NamedTarget, Node, OwnedSourceRef, OwnedXmlSerialize, Project, Scope,
    SourceOwner,
};

fn header() -> SchemaNode {
    SchemaNode::group(
        "Header",
        vec![SchemaNode::scalar("id", ScalarType::String).attribute()],
    )
}

fn input() -> SchemaNode {
    SchemaNode::group("Input", vec![header()])
}

fn output() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Xml", ScalarType::String)],
    )
}

fn serializer(owner: SourceOwner, frame: Option<Vec<String>>) -> Node {
    Node::OwnedXmlSerialize(OwnedXmlSerialize {
        source: OwnedSourceRef {
            owner,
            path: vec!["Header".into()],
        },
        frame,
        schema: Box::new(header()),
        declaration: true,
        indent: false,
        namespace: Some("urn:owned".into()),
    })
}

fn binding(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Xml".into(),
            node,
        }],
        ..Scope::default()
    }
}

/// The primary target pins the endpoint root; the named target reads `audit`.
fn project() -> Project {
    Project {
        source: input(),
        target: output(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "audit".into(),
            path: "audit.json".into(),
            schema: input(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: None,
            schema: output(),
            options: Default::default(),
            root: binding(2),
        }],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: mapping_graph([
            (1, serializer(SourceOwner::Primary, Some(vec![]))),
            (
                2,
                serializer(
                    SourceOwner::Named {
                        name: "audit".into(),
                    },
                    None,
                ),
            ),
        ]),
        root: binding(1),
    }
}

fn mapping_graph<const N: usize>(nodes: [(u32, Node); N]) -> ::mapping::Graph {
    ::mapping::Graph {
        nodes: nodes.into_iter().collect(),
    }
}

#[test]
fn emits_exact_owner_reads_before_the_shared_writer() {
    let program = codegen::lower_with_basic_owned_xml_serialize(&project()).unwrap();
    let artifacts = emit_with_basic_owned_xml_serialize(&program).unwrap();
    let mapping = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    assert!(mapping.contains(
        "context.ResolveOwnedXmlInstance(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary, "
    ));
    assert!(mapping.contains(
        ", OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary)),"
    ));
    assert!(mapping.contains(
        "context.ResolveOwnedXmlInstance(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"audit\"), "
    ));
    assert!(
        mapping.contains("global::Ferrule.Runtime.FerruleXml.SerializeEmbedded(\n            1U,")
    );
    assert!(mapping.contains("            true,\n            false,\n            \"urn:owned\");"));
    assert!(mapping.contains("error.WithOwnedSourceNode(1U);"));
    assert!(
        artifacts
            .files()
            .iter()
            .any(|file| file.path.as_str() == "Runtime/FerruleXml.Owned.cs")
    );
}

#[test]
fn older_csharp_entry_points_keep_refusing_owned_serializers() {
    let program = codegen::lower_with_basic_owned_xml_serialize(&project()).unwrap();
    let expected = codegen::validate_program(&program).unwrap_err();
    for result in [
        emit(&program),
        emit_with_basic_owned_sources(&program),
        emit_with_basic_owned_path_hierarchy(&program),
        emit_with_basic_owned_adjacency_tree(&program),
        emit_with_basic_owned_group_into_blocks(&program),
        emit_with_basic_owned_recursive_collect(&program),
        emit_with_basic_owned_sequence_exists(&program),
        emit_with_basic_owned_sequence_aggregate(&program),
        emit_with_basic_owned_recursive_filter(&program),
        emit_with_basic_owned_lookup_find(&program),
        emit_with_basic_owned_dynamic_field(&program),
    ] {
        assert!(matches!(result, Err(EmitError::ProgramValidation(error)) if error == expected));
    }
}
