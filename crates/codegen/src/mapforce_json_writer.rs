use crate::{EmbeddedSchemaError, Program};

#[derive(Debug)]
pub enum MapForceJsonWriterError {
    Shape(&'static str),
    Descriptor(EmbeddedSchemaError),
}
impl std::fmt::Display for MapForceJsonWriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(reason) => write!(f, "MapForce JSON writer: {reason}"),
            Self::Descriptor(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for MapForceJsonWriterError {}

/// The native strict JSON writer shares the shape/Decimal domain of the explicit
/// JSON5 writer, but never emits unquoted property identifiers.
pub fn prepare_mapforce_json_writer(program: &Program) -> Result<String, MapForceJsonWriterError> {
    codegen_schema::mapforce_json5_profile::validate(&program.target)
        .map_err(MapForceJsonWriterError::Shape)?;
    crate::serialize_embedded_schema(&program.target, crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES)
        .map_err(MapForceJsonWriterError::Descriptor)
}

/// Explicit body-before-filename preparation; not the deferred XML contract.
pub struct EagerX12JsonMessage {
    pub driver: Vec<String>,
    pub filename_node: mapping::NodeId,
}

pub fn prepare_eager_x12_json_message(
    program: &Program,
    filename_target: &str,
    driver: &[String],
) -> Result<EagerX12JsonMessage, MapForceJsonWriterError> {
    use MapForceJsonWriterError::Shape;
    if program.extra_targets.len() != 1
        || !program.extra_sources.is_empty()
        || !program.failure_rules.is_empty()
        || program.root.iteration.is_some()
        || program.root.repeating
        || !matches!(program.root.construction, crate::TargetConstruction::Group)
    {
        return Err(Shape(
            "eager JSON messages require one static body and one filename output",
        ));
    }
    let target = &program.extra_targets[0];
    if target.name != filename_target
        || target.root.iteration.is_some()
        || target.root.repeating
        || !matches!(target.root.construction, crate::TargetConstruction::Group)
        || !target.root.children.is_empty()
        || target.root.bindings.len() != 1
    {
        return Err(Shape("invalid named filename output"));
    }
    let binding = &target.root.bindings[0];
    if binding.target_field != "Path"
        || binding.repeating
        || binding.target_domain != ir::ScalarType::String.into()
    {
        return Err(Shape(
            "filename output must contain one String Path binding",
        ));
    }
    let mut current = &program.source;
    if driver.is_empty() || driver.len() > 64 {
        return Err(Shape("invalid document driver"));
    }
    for part in driver {
        current = current
            .child(part)
            .ok_or(Shape("document driver is absent from the source"))?;
        if current.repeating {
            return Err(Shape("document driver crosses unselected repetition"));
        }
    }
    if !matches!(
        current.kind,
        ir::SchemaKind::Scalar {
            ty: ir::ScalarType::String
        }
    ) {
        return Err(Shape("document driver must be lexical text"));
    }
    Ok(EagerX12JsonMessage {
        driver: driver.to_vec(),
        filename_node: binding.expression,
    })
}
