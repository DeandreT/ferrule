use crate::{EmbeddedSchemaError, MapForceJsonWriterError, Program};

#[derive(Debug)]
pub enum DeferredXmlPlanError {
    Shape(&'static str),
    Filename(MapForceJsonWriterError),
    Descriptor(EmbeddedSchemaError),
}
impl std::fmt::Display for DeferredXmlPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(s) => write!(f, "deferred XML: {s}"),
            Self::Filename(e) => e.fmt(f),
            Self::Descriptor(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for DeferredXmlPlanError {}

pub struct DeferredX12XmlMessage {
    pub driver: Vec<String>,
    pub filename_node: mapping::NodeId,
    pub descriptor: String,
}

pub fn prepare_deferred_x12_xml(
    program: &Program,
    filename_target: &str,
    driver: &[String],
) -> Result<DeferredX12XmlMessage, DeferredXmlPlanError> {
    if program.extra_targets.len() != 3
        || program.extra_targets[1].name != "PreparedXmlValues"
        || program.extra_targets[2].name != "DeferredXmlValues"
        || program.extra_targets[1..]
            .iter()
            .any(|target| target.target != program.target)
    {
        return Err(DeferredXmlPlanError::Shape(
            "requires the bound early and deferred XML targets",
        ));
    }
    let mut shape = program.clone();
    shape.extra_targets.truncate(1);
    let name = crate::prepare_eager_x12_json_message(&shape, filename_target, driver)
        .map_err(DeferredXmlPlanError::Filename)?;
    let descriptor =
        crate::serialize_embedded_schema(&program.target, crate::MAX_EMBEDDED_XML_SCHEMA_BYTES)
            .map_err(DeferredXmlPlanError::Descriptor)?;
    Ok(DeferredX12XmlMessage {
        driver: name.driver,
        filename_node: name.filename_node,
        descriptor,
    })
}
