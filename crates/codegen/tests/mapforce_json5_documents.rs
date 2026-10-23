use codegen::{
    CsvInputPolicy, MapForceJson5DocumentError, prepare_csv_to_mapforce_json5_documents,
};
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{DocumentProject, PrimarySourcePlan};

#[path = "../../../conformance/mapforce-json5-documents/project.rs"]
mod fixture;

fn lower(project: mapping::Project) -> codegen::DocumentProgram {
    codegen::lower_document(&DocumentProject::new(
        project,
        PrimarySourcePlan { filename: 90 },
    ))
    .unwrap()
}

#[test]
fn preserves_the_selected_complete_schema() {
    for named in [false, true] {
        let mut project = fixture::project(named);
        let schema = if named {
            &mut project.extra_targets[0].schema
        } else {
            &mut project.target
        };
        let SchemaKind::Group { required, .. } = &mut schema.kind else {
            unreachable!()
        };
        required.push("Time".into());
        let expected = schema.clone();
        let descriptor = prepare_csv_to_mapforce_json5_documents(
            &lower(project),
            &CsvInputPolicy::default(),
            named.then_some("selected"),
        )
        .unwrap();
        assert_eq!(
            codegen_schema::decode(&descriptor, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap(),
            expected
        );
    }
}

#[test]
fn refuses_unknown_target_and_unsupported_unused_members() {
    let document = lower(fixture::project(false));
    assert!(matches!(
        prepare_csv_to_mapforce_json5_documents(
            &document,
            &CsvInputPolicy::default(),
            Some("absent")
        ),
        Err(MapForceJson5DocumentError::Visitor(_))
    ));
    let mut project = fixture::project(false);
    let SchemaKind::Group { children, .. } = &mut project.target.kind else {
        unreachable!()
    };
    let mut child = SchemaNode::scalar("Unused", ScalarType::String);
    child.kind = SchemaKind::ScalarUnion {
        types: ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
    };
    children.push(child);
    assert!(matches!(
        prepare_csv_to_mapforce_json5_documents(&lower(project), &CsvInputPolicy::default(), None),
        Err(MapForceJson5DocumentError::Shape(_))
    ));
}

#[test]
fn refuses_foreign_source_shapes_and_bad_dialects() {
    let document = lower(fixture::project(false));
    let bad = CsvInputPolicy {
        delimiter: Some('\n'),
        ..Default::default()
    };
    assert!(matches!(
        prepare_csv_to_mapforce_json5_documents(&document, &bad, None),
        Err(MapForceJson5DocumentError::Source(_))
    ));
    let mut project = fixture::project(false);
    let SchemaKind::Group { children, .. } = &mut project.source.kind else {
        unreachable!()
    };
    children.push(SchemaNode::group("Unused", vec![]));
    assert!(matches!(
        prepare_csv_to_mapforce_json5_documents(&lower(project), &CsvInputPolicy::default(), None),
        Err(MapForceJson5DocumentError::Source(_))
    ));
}
