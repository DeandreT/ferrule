use codegen::{
    PrimaryMappedCsvDocumentBoundaryError as Error, PrimaryMappedCsvDocumentPolicy as Policy,
    validate_primary_mapped_csv_documents as validate,
};
use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    CsvTextRepairCause, CsvTextRepairDependency, DocumentProject, EmptyFileSelection, Node,
    PrimaryFileSelection, PrimarySourcePlan,
};

#[allow(dead_code)]
#[path = "../../cli/tests/code_generation/primary_mapped_csv_documents/fixture.rs"]
mod fixture;

fn lowered(document: &DocumentProject) -> codegen::DocumentProgram {
    codegen::lower_document(document).expect("authored supported graph")
}
fn policy(document: &DocumentProject) -> Policy {
    Policy::from_project(document.project(), fixture::ROWS).expect("authored endpoints")
}

#[test]
fn direct_literal_and_parameter_compositions_are_explicit() {
    for literal in [false, true] {
        let document = fixture::document(literal);
        let program = lowered(&document);
        validate(&program, &policy(&document)).unwrap();
        assert!(program.output_filename().is_none());
        assert_eq!(program.primary_selection(), PrimaryFileSelection::Direct);
        if literal {
            assert!(
                matches!(program.primary_filename(), codegen::PrimaryFilenamePlan::Literal { node: fixture::INPUT_NODE, text } if text == fixture::LITERAL)
            );
        } else {
            assert!(
                matches!(program.primary_filename(), codegen::PrimaryFilenamePlan::Parameter { node: fixture::INPUT_NODE, name } if name == "InputName")
            );
        }
    }
}

#[test]
fn source_row_schema_metadata_and_physical_options_are_not_dropped() {
    let base = fixture::document(false);
    for mutation in 0..6 {
        let mut project = base.project().clone();
        match mutation {
            0 => project.source.repeating = true,
            1 => project.source.fixed = Some("fixed".into()),
            2 => {
                let SchemaKind::Group { children, .. } = &mut project.source.kind else {
                    unreachable!()
                };
                children.push(children[0].clone());
            }
            3 => {
                let SchemaKind::Group { children, .. } = &mut project.source.kind else {
                    unreachable!()
                };
                children.push(SchemaNode::group("Nested", vec![]));
            }
            4 => project.source_options.json_document = true,
            _ => {
                project.source_options.csv_text_repair_dependency =
                    Some(CsvTextRepairDependency::new(CsvTextRepairCause::Encoding))
            }
        }
        assert!(
            matches!(
                Policy::from_project(&project, fixture::ROWS),
                Err(Error::Input(_))
            ),
            "mutation {mutation}"
        );
    }
    let program = lowered(&base);
    let mut invalid = policy(&base);
    invalid.input.delimiter = Some('\n');
    invalid.output.rows_field.clear();
    assert!(matches!(
        validate(&program, &invalid),
        Err(Error::Input(codegen::CsvInputBoundaryError::BadDelimiter(
            '\n'
        )))
    ));
}

#[test]
fn metadata_route_priority_is_output_filename_then_direct_then_input_then_output() {
    let base = fixture::document(false);
    let mut project = base.project().clone();
    project.graph.nodes.insert(
        98,
        Node::Const {
            value: Value::String("global.csv".into()),
        },
    );
    let wildcard = PrimarySourcePlan::new(fixture::INPUT_NODE).with_selection(
        PrimaryFileSelection::FileNameWildcard {
            empty: EmptyFileSelection::Refuse,
        },
    );
    let document = DocumentProject::new(project, wildcard.clone()).with_output_filename(98);
    let mut invalid = policy(&base);
    invalid.input.delimiter = Some('\n');
    assert!(matches!(
        validate(&lowered(&document), &invalid),
        Err(Error::OutputFilename)
    ));
    let document = DocumentProject::new(base.project().clone(), wildcard);
    assert!(matches!(
        validate(&lowered(&document), &invalid),
        Err(Error::Selection)
    ));
    invalid.input = policy(&base).input;
    invalid.output.target.quote_disabled = true;
    invalid.output.target.raw_delimited_crlf = true;
    assert!(matches!(
        validate(&lowered(&base), &invalid),
        Err(Error::Output(_))
    ));
}

#[test]
fn mapped_wrapper_selector_and_canonical_constraints_remain_closed() {
    let base = fixture::document(false);
    let program = lowered(&base);
    for field in ["", "Wrong", "\0"] {
        let mut invalid = policy(&base);
        invalid.output.rows_field = field.into();
        assert!(matches!(
            validate(&program, &invalid),
            Err(Error::Output(_))
        ));
    }
    for mutation in 0..3 {
        let mut project = base.project().clone();
        let SchemaKind::Group { children, .. } = &mut project.target.kind else {
            unreachable!()
        };
        match mutation {
            0 => children.push(SchemaNode::scalar("Extra", ScalarType::String)),
            1 => children[0].repeating = false,
            _ => {
                let SchemaKind::Group { children, .. } = &mut children[0].kind else {
                    unreachable!()
                };
                children[0].default = Some("invented".into());
            }
        }
        assert!(matches!(
            Policy::from_project(&project, fixture::ROWS),
            Err(Error::Output(_))
        ));
    }
}

#[test]
fn unsupported_filename_ownership_and_expressions_refuse_during_lowering() {
    for node in [
        Node::Const {
            value: Value::Int(9),
        },
        Node::RuntimeParameter {
            name: "InputName".into(),
            ty: ScalarType::Int,
            preview: None,
        },
        Node::SourceField {
            path: vec!["Code".into()],
            frame: None,
        },
    ] {
        let base = fixture::document(false);
        let mut project = base.project().clone();
        project.graph.nodes.insert(fixture::INPUT_NODE, node);
        assert!(matches!(
            codegen::lower_document(&DocumentProject::new(
                project,
                PrimarySourcePlan::new(fixture::INPUT_NODE)
            )),
            Err(codegen::DocumentLowerError::Filename(_))
        ));
    }
    let base = fixture::document(false);
    assert!(matches!(
        codegen::lower_document(&DocumentProject::new(
            base.project().clone(),
            PrimarySourcePlan::new(999)
        )),
        Err(codegen::DocumentLowerError::Filename(_))
    ));
}

#[test]
fn named_endpoints_and_invalid_graphs_are_not_hidden_by_the_composition() {
    let base = fixture::document(false);
    let mut conflicting_path = base.project().clone();
    conflicting_path.target_path = Some("stored-output.csv".into());
    let error = codegen::lower_document(&DocumentProject::new(
        conflicting_path,
        PrimarySourcePlan::new(fixture::INPUT_NODE),
    ))
    .expect_err("computed document paths cannot silently replace a stored target path");
    let codegen::DocumentLowerError::Mapping(cause) = error else {
        panic!("stored target path must produce a mapping diagnostic");
    };
    assert_eq!(
        cause.diagnostics(),
        &[codegen::Diagnostic::Validation {
            location: "target path".into(),
            message: "a dynamic target path cannot be combined with a stored target path".into(),
        }]
    );
    let mut project = base.project().clone();
    project.extra_sources.push(mapping::NamedSource {
        name: "Secondary".into(),
        path: "not-opened.csv".into(),
        schema: SchemaNode::group("Other", vec![]),
        options: Default::default(),
        dynamic_path: None,
    });
    assert!(matches!(
        Policy::from_project(&project, fixture::ROWS),
        Err(Error::Output(_))
    ));
    let mut project = base.project().clone();
    project.extra_targets.push(mapping::NamedTarget {
        name: "Secondary".into(),
        path: None,
        schema: SchemaNode::group("Other", vec![]),
        options: Default::default(),
        root: Default::default(),
    });
    assert!(matches!(
        Policy::from_project(&project, fixture::ROWS),
        Err(Error::Output(_))
    ));
    let mut project = base.project().clone();
    project.graph.nodes.remove(&fixture::ENDING_NODE);
    assert!(matches!(
        codegen::lower_document(&DocumentProject::new(
            project,
            PrimarySourcePlan::new(fixture::INPUT_NODE)
        )),
        Err(codegen::DocumentLowerError::Mapping(_))
    ));
}
