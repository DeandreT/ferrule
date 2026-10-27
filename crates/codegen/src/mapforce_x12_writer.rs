use crate::{Program, X12BoundaryOptions, X12BoundaryPolicy, X12EnvelopeProfile};
use mapping::{FormatOptions, MapForceEdiMinimum};

#[derive(Debug)]
pub enum MapForceX12WriterError {
    Boundary(crate::X12BoundaryPolicyError),
    Minimum,
    Serialization(serde_json::Error),
}
impl std::fmt::Display for MapForceX12WriterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(e) => e.fmt(f),
            Self::Minimum => f.write_str("invalid MapForce X12 occurrence minimums"),
            Self::Serialization(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for MapForceX12WriterError {}

pub fn prepare_mapforce_x12_writer(
    program: &Program,
    options: &FormatOptions,
    minimums: &[MapForceEdiMinimum],
) -> Result<String, MapForceX12WriterError> {
    prepare_writer(program, options, minimums, None)
}

/// A writer for each member of an explicitly lowered dynamic document set.
/// Validates the complete program without changing or evaluating its root.
pub fn prepare_mapforce_x12_document_writer(
    document: &crate::DocumentProgram,
    options: &FormatOptions,
    minimums: &[MapForceEdiMinimum],
) -> Result<String, MapForceX12WriterError> {
    prepare_writer(document.program(), options, minimums, Some(document))
}

fn prepare_writer(
    program: &Program,
    options: &FormatOptions,
    minimums: &[MapForceEdiMinimum],
    document: Option<&crate::DocumentProgram>,
) -> Result<String, MapForceX12WriterError> {
    if minimums.len() > 10000 {
        return Err(MapForceX12WriterError::Minimum);
    }
    let mut seen = std::collections::BTreeSet::new();
    for minimum in minimums {
        let mut node = &program.target;
        for part in &minimum.path {
            node = node.child(part).ok_or(MapForceX12WriterError::Minimum)?;
        }
        if minimum.minimum > 1024
            || minimum.path.len() > 64
            || !seen.insert(&minimum.path)
            || !matches!(node.kind, ir::SchemaKind::Group { .. })
        {
            return Err(MapForceX12WriterError::Minimum);
        }
    }
    let mut target = X12BoundaryOptions::from_format_options(options)
        .map_err(MapForceX12WriterError::Boundary)?;
    target.envelope_profile = X12EnvelopeProfile::MapForceInterchanges;
    let profile = match document {
        Some(document) => crate::x12_boundary::prepare_x12_document_target(document, target),
        None => crate::prepare_x12_boundary(
            program,
            &X12BoundaryPolicy {
                source: None,
                target: Some(target),
            },
        ),
    }
    .map_err(MapForceX12WriterError::Boundary)?;
    serde_json::to_string(
        &serde_json::json!({"boundary":profile.target_descriptor,"minimums":minimums}),
    )
    .map_err(MapForceX12WriterError::Serialization)
}
