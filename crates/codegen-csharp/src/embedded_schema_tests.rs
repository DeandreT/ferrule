include!("../../codegen/src/tests/embedded_schema_emission.rs");

fn emit_schema_fixture(program: &Program) -> Result<ArtifactSet, EmbeddedSchemaError> {
    match crate::emit(program) {
        Ok(artifacts) => Ok(artifacts),
        Err(crate::EmitError::EmbeddedSchema(error)) => Err(error),
        Err(error) => panic!("unexpected emitter error: {error}"),
    }
}
