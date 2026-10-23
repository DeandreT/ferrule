use crate::{Program, X12BoundaryOptions, X12BoundarySide, X12EnvelopeProfile};
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

#[derive(Debug)]
pub enum MapForceX12SourceError {
    Boundary(crate::X12BoundaryPolicyError),
    Binding(&'static str),
}
impl std::fmt::Display for MapForceX12SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Boundary(error) => error.fmt(f),
            Self::Binding(reason) => write!(f, "MapForce X12 source: {reason}"),
        }
    }
}
impl std::error::Error for MapForceX12SourceError {}

/// Bind a physical X12 source to one lexical message body. The returned boundary
/// parses the complete source; it does not execute the body or select filenames.
pub fn prepare_mapforce_x12_source(
    program: &Program,
    schema: &SchemaNode,
    options: &FormatOptions,
    message: &[String],
) -> Result<String, MapForceX12SourceError> {
    use MapForceX12SourceError::{Binding, Boundary};
    if message.is_empty() || message.len() > 64 || message.iter().any(String::is_empty) {
        return Err(Binding("invalid message owner path"));
    }
    let node = message
        .iter()
        .try_fold(schema, |node, name| node.child(name))
        .ok_or(Binding("message owner is absent from the physical schema"))?;
    if !node.repeating || !matches!(node.kind, SchemaKind::Group { .. }) {
        return Err(Binding("message owner must be a repeated group"));
    }
    fn lexical(node: &mut SchemaNode) {
        match &mut node.kind {
            SchemaKind::Scalar { ty } => *ty = ScalarType::String,
            SchemaKind::Group { children, .. } => children.iter_mut().for_each(lexical),
            SchemaKind::ScalarUnion { .. } => {}
        }
    }
    let mut selected = node.clone();
    selected.repeating = false;
    lexical(&mut selected);
    if program.source != selected {
        return Err(Binding(
            "body source differs from the selected lexical message",
        ));
    }
    let mut boundary = X12BoundaryOptions::from_format_options(options).map_err(Boundary)?;
    boundary.envelope_profile = X12EnvelopeProfile::MapForceInterchanges;
    if boundary
        .implied_decimals
        .iter()
        .any(|value| value.places() != 0)
    {
        return Err(Binding(
            "fractional implied-decimal source projection is not admitted",
        ));
    }
    crate::x12_boundary::descriptor(schema, Some(&boundary), X12BoundarySide::Source)
        .map_err(Boundary)
}
