use super::*;
use crate::{
    ExpressionNode, OwnedSourceCapabilityOrigin as Origin,
    OwnedSourceCapabilityReason as Capability, OwnedSourceDiagnostic,
    OwnedSourceDiagnosticLocation as Location, OwnedSourceDiagnosticOwner as Owner,
    OwnedSourceDiagnosticRequest as Request, OwnedSourceNamespace as Namespace,
};
use mapping::{
    Binding, OwnedSourceRef, OwnedXmlSerialize, SourceOwner, SourceScopeLocation, SourceScopeTarget,
};

fn header() -> SchemaNode {
    SchemaNode::group("Header", vec![scalar("id").attribute()])
}

fn serializer() -> OwnedXmlSerialize {
    OwnedXmlSerialize {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec!["Header".into()],
        },
        frame: None,
        schema: Box::new(header()),
        declaration: false,
        indent: true,
        namespace: None,
    }
}

fn owned_project() -> Project {
    let mut project = supported_project();
    project.source = SchemaNode::group("Input", vec![header()]);
    project.target = SchemaNode::group("Output", vec![scalar("Xml")]);
    project.graph = Graph {
        nodes: [(1, Node::OwnedXmlSerialize(serializer()))].into(),
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Xml".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    project
}

fn refusal(
    reason: Capability,
    origin: Origin,
    scope: Option<SourceScopeLocation>,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope,
            },
            request: Some(Request::XmlElement {
                owner: Owner::Primary,
                path: vec!["Header".into()],
                frame: None,
            }),
            reason,
            origin,
        }),
    }
}

#[test]
fn owned_serializer_lowers_exactly_and_stays_closed_by_default() {
    let project = owned_project();
    let program = crate::lower_with_basic_owned_xml_serialize(&project).unwrap();
    assert_eq!(
        program.expressions,
        vec![ExpressionNode {
            id: 1,
            expression: Expression::OwnedXmlSerialize(serializer()),
        }]
    );
    crate::validate_basic_owned_xml_serialize_emission(&program).unwrap();

    let expected = refusal(
        Capability::XmlOperator,
        Origin::Expression {
            namespace: Namespace::Main,
            node: 1,
        },
        Some(SourceScopeLocation::root(SourceScopeTarget::Primary)),
    );
    assert_eq!(validate_program(&program), Err(expected.clone()));
    assert_eq!(
        crate::validate_basic_owned_path_hierarchy_emission(&program),
        Err(expected.clone())
    );
    for result in [
        crate::validate_basic_owned_adjacency_tree_emission(&program),
        crate::validate_basic_owned_group_into_blocks_emission(&program),
        crate::validate_basic_owned_recursive_collect_emission(&program),
        crate::validate_basic_owned_sequence_exists_emission(&program),
        crate::validate_basic_owned_sequence_aggregate_emission(&program),
        crate::validate_basic_owned_recursive_filter_emission(&program),
        crate::validate_basic_owned_lookup_find_emission(&program),
        crate::validate_basic_owned_dynamic_field_emission(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    // A retained expression no target reaches is refused the same way.
    let mut unreached = program.clone();
    unreached.root.bindings.clear();
    assert_eq!(
        validate_program(&unreached),
        Err(refusal(
            Capability::XmlOperator,
            Origin::Expression {
                namespace: Namespace::Main,
                node: 1,
            },
            None,
        ))
    );
    crate::validate_program_with_basic_owned_xml_serialize(&unreached).unwrap();
    let Err(error) = lower(&project) else {
        panic!("default lowering keeps owned serializers closed");
    };
    let ProgramValidationError::OwnedSource { diagnostic } = expected else {
        unreachable!();
    };
    assert_eq!(
        error.diagnostics(),
        &[crate::Diagnostic::OwnedSource { diagnostic }]
    );
}
