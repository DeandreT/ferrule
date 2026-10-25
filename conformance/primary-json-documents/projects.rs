//! Strict JSON plans share the authored grouped mapping with the XML cohort.
use super::xml_projects as grouped;
use super::*;

pub use grouped::MODELS;

pub fn document(model: &str) -> mapping::DocumentProject {
    let xml = grouped::document(model);
    let mut project = xml.project().clone();
    let json = mapping::FormatOptions {
        json_document: model != "saved_path_identity",
        ..Default::default()
    };
    project.source_options = json.clone();
    project.target_options = json;
    project.source_path = Some("never-preview.json".into());
    project.target_path = Some("never-preview-output.json".into());
    if let Node::Const {
        value: Value::String(path),
    } = project.graph.nodes.get_mut(&grouped::FILENAME).unwrap()
    {
        *path = "orders/input.json".into();
    }
    mapping::DocumentProject::new(project, xml.primary_source().clone())
}
