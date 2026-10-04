use super::*;

fn project() -> Project {
    serde_json::from_str(include_str!("fixtures/dynamic_named_xml_input.json")).unwrap()
}

#[test]
fn dynamic_xml_boundary_retains_full_declaration_indices_and_per_source_schemas() {
    let project = project();
    let program = lower(&project).unwrap();
    let policy = program.xml_boundary.as_ref().unwrap();
    assert_eq!(
        policy.input.profile(),
        Some(crate::XmlInputProfile::Structured)
    );
    assert_eq!(
        policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog"]
    );
    assert!(program.extra_sources[0].dynamic.is_none());
    assert!(program.extra_sources[1].dynamic.is_some());
    for ((source, original), input) in program
        .extra_sources
        .iter()
        .zip(&project.extra_sources)
        .zip(&policy.extra_inputs)
    {
        assert_eq!(source.source, original.schema);
        assert_eq!(source.name, input.name);
        assert_eq!(
            input.input.profile(),
            Some(crate::XmlInputProfile::Structured)
        );
    }
    assert_eq!(policy.extra_outputs.len(), 1);
    assert_eq!(policy.extra_outputs[0].name, "audit");
}

#[test]
fn unproved_dynamic_boundary_falls_back_as_a_whole_without_dropping_typed_core() {
    for mutation in 0..3 {
        let mut project = project();
        match mutation {
            0 => project.extra_sources[1].options.xml_document = false,
            1 => {
                let mut second = project.extra_sources[1].clone();
                second.name = "second-catalog".into();
                project.extra_sources.push(second);
            }
            _ => {
                let SchemaKind::Group { children, .. } = &mut project.extra_sources[1].schema.kind
                else {
                    unreachable!()
                };
                let SchemaKind::Group {
                    children: fields, ..
                } = &mut children[0].kind
                else {
                    unreachable!()
                };
                fields[1].default = Some("1.25".into());
            }
        }
        let program = lower(&project).unwrap();
        assert!(program.xml_boundary.is_none());
        assert_eq!(program.extra_sources.len(), project.extra_sources.len());
        assert!(program.extra_sources[1].dynamic.is_some());
        assert_eq!(program.extra_targets.len(), 1);
    }
}

#[test]
fn unused_dynamic_declaration_is_a_policy_and_not_a_required_static_input() {
    let project: Project =
        serde_json::from_str(include_str!("fixtures/unused_dynamic_named_xml_input.json")).unwrap();
    let program = lower(&project).unwrap();
    assert!(program.xml_boundary.is_some());
    assert!(program.extra_sources[1].dynamic.is_some());
    assert!(program.root.children.is_empty());
}

#[test]
fn interleaved_dynamic_policies_keep_both_static_neighbors_in_original_order() {
    let project: Project =
        serde_json::from_str(include_str!("fixtures/dynamic_named_xml_input_mixed.json")).unwrap();
    let program = lower(&project).unwrap();
    assert_eq!(
        program
            .xml_boundary
            .as_ref()
            .unwrap()
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        ["rates", "catalog", "labels"]
    );
    assert!(program.extra_sources[0].dynamic.is_none());
    assert!(program.extra_sources[1].dynamic.is_some());
    assert!(program.extra_sources[2].dynamic.is_none());
}
