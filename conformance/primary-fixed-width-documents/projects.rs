//! Fixed-width policies over the same authored cross-document grouped row map.
use super::csv_projects as grouped;
use super::*;

pub const MODELS: [&str; 6] = [
    "literal_direct",
    "parameter_wildcard",
    "parameter_wildcard_map_no_documents",
    "contiguous",
    "present_empty",
    "unicode_fill",
];

pub fn document(model: &str) -> mapping::DocumentProject {
    let base = grouped::document(match model {
        "literal_direct" => "literal_direct",
        "parameter_wildcard_map_no_documents" => model,
        _ => "parameter_wildcard",
    });
    let mut project = base.project().clone();
    let fill = if model == "unicode_fill" { '🟦' } else { '_' };
    let layout = |widths: &[u32]| {
        mapping::FixedWidthLayout::new(
            widths
                .iter()
                .map(|w| mapping::FixedFieldWidth::new(*w).unwrap())
                .collect(),
            fill,
            model != "contiguous",
            model != "present_empty",
        )
        .unwrap()
    };
    project.source_options = mapping::FormatOptions {
        fixed_width: Some(layout(&[2, 4, 2])),
        ..Default::default()
    };
    project.target_options = mapping::FormatOptions {
        fixed_width: Some(layout(&[2, 12, 3, 4])),
        ..Default::default()
    };
    project.source_path = Some("never-preview.dat".into());
    project.target_path = Some("never-preview-output.dat".into());
    if let Node::Const {
        value: Value::String(path),
    } = project.graph.nodes.get_mut(&grouped::FILENAME).unwrap()
    {
        *path = "orders/input.dat".into();
    }
    mapping::DocumentProject::new(project, base.primary_source().clone())
}
