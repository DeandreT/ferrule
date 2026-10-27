use crate::{
    IterationOutput, IterationPlan, ProgramValidationError, SourceIteration, TargetConstruction,
    XmlOutputMode,
};

fn project() -> mapping::Project {
    serde_json::from_str(include_str!("fixtures/nested_document_collection.json")).unwrap()
}

#[test]
fn document_collections_lower_with_context_relative_xml_drivers() {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let program = crate::lower(&project).unwrap();
    assert!(matches!(
        program.root.construction,
        TargetConstruction::CollectDocuments
    ));
    assert!(program.root.produces_documents());
    assert!(program.root.children[0].produces_documents());
    assert_eq!(
        program.xml_output_mode(),
        Ok(Some(XmlOutputMode::DynamicPrimaryDocuments))
    );
}

#[test]
fn direct_program_rejects_collector_cardinality_and_target_field_ambiguity() {
    let program = crate::lower(&project()).unwrap();
    let mut first = program.clone();
    first.root.iteration = Some(IterationPlan::new(
        SourceIteration::new(vec!["Dept".into()]),
        None,
        None,
        Vec::new(),
        IterationOutput::First,
    ));
    assert!(matches!(
        crate::validate_program(&first),
        Err(ProgramValidationError::InvalidDocumentCollection { .. })
    ));
    let mut field = program.clone();
    field.root.children[0].target_field = "Person".into();
    assert!(matches!(
        crate::validate_program(&field),
        Err(ProgramValidationError::InvalidDocumentCollection { .. })
    ));
    let mut absent = program;
    absent.root.children.clear();
    assert_eq!(
        crate::validate_program(&absent),
        Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "XML document collectors require document-producing children".into(),
        })
    );
}

#[test]
fn context_relative_document_driver_keeps_repetition_and_group_requirements() {
    let mut program = crate::lower(&project()).unwrap();
    program.root.children[0].iteration =
        Some(IterationPlan::dynamic_documents(vec!["Name".into()], 8));
    assert!(crate::validate_program(&program).is_err());
}

#[test]
fn a_collector_cannot_create_an_inactive_descendant_source_frame() {
    let mut program = crate::lower(&project()).unwrap();
    program.root.iteration = None;
    assert!(
        matches!(program.xml_output_mode(), Err(ProgramValidationError::InvalidXmlBoundary { reason }) if reason == "dynamic XML output driver must end in a repeating Group")
    );
}

#[test]
fn named_collector_uses_list_cardinality_without_changing_the_primary() {
    let mut project = project();
    project.extra_targets.push(mapping::NamedTarget {
        name: "documents".into(),
        path: None,
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    project.root = mapping::Scope::default();
    let program = crate::lower(&project).unwrap();
    assert_eq!(
        program.xml_output_mode().unwrap(),
        Some(XmlOutputMode::StaticPrimaryDynamicNamedDocuments)
    );
}

#[test]
fn grouped_member_fields_do_not_shadow_real_outer_frame_fields() {
    let mut program = crate::lower(&project()).unwrap();
    let ir::SchemaKind::Group { children, .. } = &mut program.source.kind else {
        unreachable!()
    };
    children.push(ir::SchemaNode::scalar("Person", ir::ScalarType::String).repeating());
    let error = program.xml_output_mode().unwrap_err();
    assert!(
        format!("{error:?}").contains("dynamic XML output driver must end in a repeating Group")
    );
}

#[test]
fn absent_optional_groups_cannot_fall_back_to_an_incompatible_driver() {
    let mut program = crate::lower(&project()).unwrap();
    program.root.iteration = Some(IterationPlan::source(vec!["Dept".into()]));
    program.root.children[0].iteration = Some(IterationPlan::dynamic_documents(
        vec!["Selection".into(), "Person".into()],
        13,
    ));
    let ir::SchemaKind::Group { children, .. } = &mut program.source.kind else {
        unreachable!()
    };
    let department = &mut children[0];
    let person = department.child("Person").unwrap().clone();
    let ir::SchemaKind::Group {
        children: fields, ..
    } = &mut department.kind
    else {
        unreachable!()
    };
    let mut optional = ir::SchemaNode::group("Selection", vec![person]);
    optional.xml_optional = true;
    fields.push(optional);
    children.push(ir::SchemaNode::group(
        "Selection",
        vec![ir::SchemaNode::scalar("Person", ir::ScalarType::String).repeating()],
    ));
    assert!(matches!(
        program.xml_output_mode(),
        Err(ProgramValidationError::InvalidXmlBoundary { .. })
    ));
}
