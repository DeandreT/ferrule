use mapping::{FormatOptions, JsonProjectionPlanError, JsonSourceShape};

use crate::DocumentProgram;

#[derive(Debug)]
pub enum JsonProjectionBoundaryError {
    Shape(JsonProjectionPlanError),
    SourceView,
    Descriptor(serde_json::Error),
}

impl std::fmt::Display for JsonProjectionBoundaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(error) => error.fmt(f),
            Self::SourceView => f.write_str("JSON projection requires its exact document-set source view and default view options"),
            Self::Descriptor(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for JsonProjectionBoundaryError {}

pub fn prepare_json_source_projection(
    document: &DocumentProgram,
    shape: &JsonSourceShape,
) -> Result<String, JsonProjectionBoundaryError> {
    let source = document
        .json_grouping_preparation()
        .map_or_else(
            || shape.document_set_schema(),
            |plan| plan.document_set_schema(shape),
        )
        .map_err(JsonProjectionBoundaryError::Shape)?;
    if document.json_source_projection() != Some(shape)
        || document.program().source != source
        || *document.source_options() != FormatOptions::default()
    {
        return Err(JsonProjectionBoundaryError::SourceView);
    }
    serde_json::to_string(shape).map_err(JsonProjectionBoundaryError::Descriptor)
}
