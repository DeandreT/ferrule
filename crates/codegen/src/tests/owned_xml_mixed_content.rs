use super::*;
use crate::{
    ExpressionNode, OwnedSourceCapabilityOrigin as Origin,
    OwnedSourceCapabilityReason as Capability, OwnedSourceDiagnostic,
    OwnedSourceDiagnosticLocation as Location, OwnedSourceDiagnosticOwner as Owner,
    OwnedSourceDiagnosticRequest as Request, OwnedSourceNamespace as Namespace,
    OwnedSourceStructureReason as Structure,
};
use mapping::{
    Binding, OwnedSourceField, OwnedSourceRef, OwnedXmlMixedContent, OwnedXmlMixedContentContext,
    OwnedXmlMixedContentReplacement, SourceOwner, SourceScopeLocation, SourceScopeTarget,
};

fn para() -> SchemaNode {
    SchemaNode::group(
        "Para",
        vec![
            scalar(ir::XML_TEXT_FIELD).text(),
            scalar("b").repeating(),
            scalar("u"),
        ],
    )
}

fn source_ref(path: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path.iter().map(|part| (*part).into()).collect(),
    }
}

fn mixed() -> OwnedXmlMixedContent {
    OwnedXmlMixedContent {
        source: source_ref(&["Para"]),
        frame: None,
        replacements: vec![OwnedXmlMixedContentReplacement {
            element: "b".into(),
            context: OwnedXmlMixedContentContext::Occurrence,
            expression: 2,
        }],
    }
}

fn owned_project() -> Project {
    let mut project = supported_project();
    project.source = SchemaNode::group("Input", vec![para(), scalar("Title")]);
    project.target = SchemaNode::group("Output", vec![scalar("Text")]);
    project.graph = Graph {
        nodes: [
            (1, Node::OwnedXmlMixedContent(mixed())),
            (
                2,
                Node::OwnedSourceField(OwnedSourceField {
                    source: source_ref(&["Para", "b"]),
                    frame: None,
                }),
            ),
        ]
        .into(),
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Text".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    project
}

fn refusal(scope: Option<SourceScopeLocation>) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedSourceDiagnostic::Capability {
            location: Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope,
            },
            request: Some(Request::XmlElement {
                owner: Owner::Primary,
                path: vec!["Para".into()],
                frame: None,
            }),
            reason: Capability::XmlOperator,
            origin: Origin::Expression {
                namespace: Namespace::Main,
                node: 1,
            },
        }),
    }
}

#[test]
fn owned_mixed_content_lowers_exactly_and_stays_closed_before_its_companion() {
    let project = owned_project();
    let program = crate::lower_with_basic_owned_xml_mixed_content(&project).unwrap();
    assert_eq!(
        program.expressions[0],
        ExpressionNode {
            id: 1,
            expression: Expression::OwnedXmlMixedContent(mixed()),
        }
    );
    crate::validate_basic_owned_xml_mixed_content_emission(&program).unwrap();
    crate::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content(&program).unwrap();

    let expected = refusal(Some(SourceScopeLocation::root(SourceScopeTarget::Primary)));
    assert_eq!(validate_program(&program), Err(expected.clone()));
    for result in [
        crate::validate_basic_owned_path_hierarchy_emission(&program),
        crate::validate_basic_owned_adjacency_tree_emission(&program),
        crate::validate_basic_owned_group_into_blocks_emission(&program),
        crate::validate_basic_owned_recursive_collect_emission(&program),
        crate::validate_basic_owned_sequence_exists_emission(&program),
        crate::validate_basic_owned_sequence_aggregate_emission(&program),
        crate::validate_basic_owned_recursive_filter_emission(&program),
        crate::validate_basic_owned_lookup_find_emission(&program),
        crate::validate_basic_owned_dynamic_field_emission(&program),
        // The serialization companion it composes still refuses it.
        crate::validate_basic_owned_xml_serialize_emission(&program),
        crate::validate_program_with_basic_owned_xml_serialize(&program),
    ] {
        assert_eq!(result, Err(expected.clone()));
    }
    // A retained expression no target reaches is refused the same way.
    let mut unreached = program.clone();
    unreached.root.bindings.clear();
    assert_eq!(validate_program(&unreached), Err(refusal(None)));
    crate::validate_program_with_basic_owned_xml_mixed_content(&unreached).unwrap();
    let Err(error) = lower(&project) else {
        panic!("default lowering keeps owned mixed content closed");
    };
    let ProgramValidationError::OwnedSource { diagnostic } = expected else {
        unreachable!();
    };
    assert_eq!(
        error.diagnostics(),
        &[crate::Diagnostic::OwnedSource { diagnostic }]
    );
}

#[test]
fn direct_programs_refuse_invalid_owned_mixed_content() {
    let base = crate::lower_with_basic_owned_xml_mixed_content(&owned_project()).unwrap();
    let with = |change: &dyn Fn(&mut OwnedXmlMixedContent)| {
        let mut program = base.clone();
        let Expression::OwnedXmlMixedContent(mixed) = &mut program.expressions[0].expression else {
            unreachable!("authored owned mixed content");
        };
        change(mixed);
        program
    };
    let path = |parts: &[&str]| parts.iter().map(|part| (*part).into()).collect::<Vec<_>>();
    let cases = [
        (
            with(&|mixed| mixed.source.path = path(&["Title"])),
            ProgramValidationError::InvalidXmlMixedContentSource {
                node: 1,
                path: path(&["Title"]),
            },
        ),
        (
            with(&|mixed| mixed.replacements[0].element = "u".into()),
            ProgramValidationError::InvalidXmlMixedContentCollection {
                node: 1,
                replacement: 0,
                collection: path(&["Para", "u"]),
            },
        ),
        (
            with(&|mixed| mixed.replacements[0].element = String::new()),
            ProgramValidationError::EmptyXmlMixedContentElement {
                node: 1,
                replacement: 0,
            },
        ),
        (
            with(&|mixed| {
                let duplicate = mixed.replacements[0].clone();
                mixed.replacements.push(duplicate);
            }),
            ProgramValidationError::DuplicateXmlMixedContentElement {
                node: 1,
                element: "b".into(),
            },
        ),
        (
            with(&|mixed| {
                mixed.source.owner = SourceOwner::Named {
                    name: "missing".into(),
                }
            }),
            ProgramValidationError::OwnedSource {
                diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                    location: Location::Expression {
                        namespace: Namespace::Main,
                        node: 1,
                        scope: None,
                    },
                    request: Request::XmlElement {
                        owner: Owner::Named {
                            name: "missing".into(),
                        },
                        path: path(&["Para"]),
                        frame: None,
                    },
                    reason: Structure::UnknownOwner,
                }),
            },
        ),
        (
            with(&|mixed| mixed.frame = Some(path(&["Title"]))),
            ProgramValidationError::OwnedSource {
                diagnostic: Box::new(OwnedSourceDiagnostic::Structure {
                    location: Location::Expression {
                        namespace: Namespace::Main,
                        node: 1,
                        scope: None,
                    },
                    request: Request::XmlElement {
                        owner: Owner::Primary,
                        path: path(&["Para"]),
                        frame: Some(path(&["Title"])),
                    },
                    reason: Structure::IncompatibleFramePath,
                }),
            },
        ),
    ];
    for (program, expected) in &cases {
        assert_eq!(
            crate::validate_program_with_basic_owned_xml_mixed_content(program).as_ref(),
            Err(expected)
        );
        assert_eq!(validate_program(program).as_ref(), Err(expected));
    }
    // A parent-context rule may name an undeclared element, like the legacy node.
    let undeclared = with(&|mixed| {
        mixed.replacements[0].element = "em".into();
        mixed.replacements[0].context = OwnedXmlMixedContentContext::Parent;
    });
    crate::validate_program_with_basic_owned_xml_mixed_content(&undeclared).unwrap();
}
