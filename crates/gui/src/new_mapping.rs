use std::path::PathBuf;

use ir::SchemaNode;

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
