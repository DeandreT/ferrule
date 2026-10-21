//! Emission coverage for the explicit owned XML mixed-content companion.

use super::*;
use ::mapping::{
    Binding, NamedSource, Node, OwnedSourceField, OwnedSourceRef, OwnedXmlMixedContent,
    OwnedXmlMixedContentContext, OwnedXmlMixedContentReplacement, Project, Scope, SourceOwner,
};

fn input() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            SchemaNode::group(
                "Para",
                vec![
                    SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                    SchemaNode::scalar("b", ScalarType::String).repeating(),
                ],
            ),
        ],
    )
}

fn source(owner: SourceOwner, path: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner,
        path: path.iter().map(|part| (*part).into()).collect(),
    }
}

fn project() -> Project {
    let notes = SourceOwner::Named {
        name: "notes".into(),
    };
    Project {
        source: input(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "notes".into(),
            path: "notes.json".into(),
            schema: input(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: ::mapping::Graph {
            nodes: [
                (
                    1,
                    Node::OwnedXmlMixedContent(OwnedXmlMixedContent {
                        source: source(SourceOwner::Primary, &["Para"]),
                        frame: Some(vec![]),
                        replacements: vec![
                            OwnedXmlMixedContentReplacement {
                                element: "b".into(),
                                context: OwnedXmlMixedContentContext::Occurrence,
                                expression: 2,
                            },
                            OwnedXmlMixedContentReplacement {
                                element: "i".into(),
                                context: OwnedXmlMixedContentContext::Parent,
                                expression: 3,
                            },
                        ],
                    }),
                ),
                (
                    2,
                    Node::OwnedSourceField(OwnedSourceField {
                        source: source(SourceOwner::Primary, &["Para", "b"]),
                        frame: None,
                    }),
                ),
                (
                    3,
                    Node::OwnedSourceField(OwnedSourceField {
                        source: source(notes, &["Title"]),
                        frame: None,
                    }),
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Text".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn emits_owned_group_reads_with_occurrence_and_parent_replacements() {
    let program = codegen::lower_with_basic_owned_xml_mixed_content(&project()).unwrap();
    let artifacts = emit_with_basic_owned_xml_mixed_content(&program).unwrap();
    let mapping = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    assert!(mapping.contains(
        "context.OwnedXmlMixedContent(\n            1U,\n            global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary,"
    ));
    assert!(mapping.contains(
        "OwnedRootIsDeclaredNonrepeating(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Primary),"
    ));
    assert!(mapping.contains("                new(\"b\", true, Node_2),\n"));
    assert!(mapping.contains("                new(\"i\", false, Node_3),\n"));
    assert!(
        artifacts
            .files()
            .iter()
            .any(|file| file.path.as_str() == "Runtime/FerruleXmlMixedContent.Owned.cs")
    );
    // Older entry points, including the serialization companion, keep refusing it.
    let expected = codegen::validate_program(&program).unwrap_err();
    for result in [
        emit(&program),
        emit_with_basic_owned_sources(&program),
        emit_with_basic_owned_dynamic_field(&program),
        emit_with_basic_owned_xml_serialize(&program),
    ] {
        assert!(matches!(result, Err(EmitError::ProgramValidation(error)) if error == expected));
    }
}
