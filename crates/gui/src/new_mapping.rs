use std::path::PathBuf;

use ir::SchemaNode;
use mapping::{Graph, Project, Scope};

#[derive(Default)]
pub(super) struct NewMappingSetup {
    pub(super) source: Option<ImportedSchema>,
    pub(super) target: Option<ImportedSchema>,
}

pub(super) struct ImportedSchema {
    pub(super) path: PathBuf,
    pub(super) schema: SchemaNode,
}

#[derive(Clone, Copy)]
pub(super) enum SchemaSide {
    Source,
    Target,
}

impl SchemaSide {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Target => "Target",
        }
    }
}

pub(super) fn import_schema(path: &std::path::Path) -> anyhow::Result<SchemaNode> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let json = match extension.as_deref() {
        Some("xsd") => cli::import_xsd(path)?,
        Some("json") => cli::import_json_schema(path)?,
        _ => anyhow::bail!("schema must be an XSD or JSON Schema file"),
    };
    Ok(serde_json::from_str(&json)?)
}

pub(super) fn blank_project() -> Project {
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}
