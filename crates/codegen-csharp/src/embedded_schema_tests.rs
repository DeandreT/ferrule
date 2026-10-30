include!("../../codegen/src/tests/embedded_schema_emission.rs");

fn emit_schema_fixture(program: &Program) -> Result<ArtifactSet, EmbeddedSchemaError> {
    match crate::emit(program) {
        Ok(artifacts) => Ok(artifacts),
        Err(crate::EmitError::EmbeddedSchema(error)) => Err(error),
        Err(error) => panic!("unexpected emitter error: {error}"),
    }
}

fn tuple_frame() -> SchemaNode {
    SchemaNode::group(
        "Point",
        vec![
            SchemaNode::scalar("item1", ScalarType::String),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    )
    .with_json_tuple(ir::JsonTupleLayout {
        length: ir::ItemCountRange::new(0, Some(2)),
        tail: ir::JsonTupleTail::Closed,
    })
    .expect("the test frame is canonical")
}

fn with_frame(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![tuple_frame()])
}

#[test]
fn csharp_refuses_tuple_frames_in_every_boundary_schema_before_artifacts() {
    let mut source = stable_program();
    source.source = with_frame("FramedSource");
    let mut target = stable_program();
    target.target = with_frame("FramedTarget");
    let mut named_source = stable_program();
    named_source.extra_sources.push(NamedSourceProgram {
        name: "catalog".into(),
        source: with_frame("FramedCatalog"),
        dynamic: None,
    });
    let mut named_target = stable_program();
    named_target.extra_targets.push(NamedTargetProgram {
        name: "report".into(),
        target: with_frame("FramedReport"),
        root: named_target.root.clone(),
    });
    for (program, schema) in [
        (source, "FramedSource"),
        (target, "FramedTarget"),
        (named_source, "FramedCatalog"),
        (named_target, "FramedReport"),
    ] {
        let expected = Err(crate::EmitError::JsonTupleFrame {
            schema: schema.into(),
        });
        assert_eq!(crate::emit(&program), expected, "{schema}");
        assert_eq!(
            crate::emit_with_companions(&program, codegen::CompanionSelection::TypedOnly),
            expected,
            "{schema}"
        );
        assert_eq!(crate::emit_typed(&program), expected, "{schema}");
        assert_eq!(
            crate::emit_with_basic_owned_sources(&program),
            expected,
            "{schema}"
        );
        assert_eq!(
            crate::emit_with_basic_owned_filters(&program),
            expected,
            "{schema}"
        );
    }
}
