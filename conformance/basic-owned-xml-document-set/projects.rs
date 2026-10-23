//! Genuine public Project definitions for owned XML document-set entry
//! points. Authored source only: no mapping, document or path is evaluated
//! here. The models are the owned document-path models with ordinary XML
//! format identity on the primary, the named `Ref` endpoint and the target.
use super::*;

use super::super::basic_owned_document_path::projects as document_path;

pub use document_path::NAMED;

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 3] = ["nested", "cross", "sorted"];

fn xml() -> mapping::FormatOptions {
    mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    }
}

pub fn project(model: &str) -> Project {
    let mut project = document_path::project(model);
    project.source_options = xml();
    project.target_options = xml();
    for source in &mut project.extra_sources {
        source.options = xml();
    }
    project
}
