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
    // The shared fixture maps its scalar constant to Out. Keep that binding
    // valid when adding an otherwise unused tuple to a boundary schema.
    SchemaNode::group(
        name,
        vec![SchemaNode::scalar("Out", ScalarType::Float), tuple_frame()],
    )
}

#[test]
fn csharp_embeds_tuple_frames_in_every_boundary_schema_as_v5_descriptors() {
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
        let descriptor =
            codegen::serialize_embedded_schema(&with_frame(schema), MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("the framed schema encodes");
        assert!(
            descriptor.starts_with("FERRULE-EMBEDDED-SCHEMA/5\n"),
            "{schema}"
        );
        for (route, json_boundary, artifacts) in [
            ("complete", true, crate::emit(&program)),
            ("legacy typed", false, crate::emit_typed(&program)),
            (
                "basic owned sources",
                true,
                crate::emit_with_basic_owned_sources(&program),
            ),
            (
                "basic owned filters",
                false,
                crate::emit_with_basic_owned_filters(&program),
            ),
        ] {
            let artifacts = artifacts.unwrap_or_else(|error| panic!("{schema}: {error}"));
            let literal = serde_json::to_string(&descriptor).unwrap();
            let emitted = artifacts
                .files()
                .iter()
                .any(|file| String::from_utf8_lossy(&file.contents).contains(&literal));
            assert_eq!(
                emitted, json_boundary,
                "{schema}: {route} must embed a descriptor only when it has a JSON boundary"
            );
        }
        // TypedOnly intentionally has no JSON/XML boundary descriptor. It
        // must still accept the tuple-shaped typed schema without losing that
        // distinction by accidentally adding JSON companions.
        let typed = crate::emit_with_companions(&program, codegen::CompanionSelection::TypedOnly)
            .unwrap_or_else(|error| panic!("{schema}: {error}"));
        let literal = serde_json::to_string(&descriptor).unwrap();
        assert!(
            typed
                .files()
                .iter()
                .all(|file| { !String::from_utf8_lossy(&file.contents).contains(&literal) }),
            "{schema}: typed-only emission must not add a JSON descriptor"
        );
    }
}
