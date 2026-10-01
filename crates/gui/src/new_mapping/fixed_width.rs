use std::path::PathBuf;

use anyhow::{Context as _, bail};
use ir::SchemaNode;
use mapping::FormatOptions;

use super::ImportedSchema;
use crate::extra_targets::FixedWidthTargetDraft;

/// A staged primary boundary. The schema is cached; the data path is optional
/// and never opened while configuring the layout.
pub(crate) struct FixedWidthBoundaryDraft {
    pub(crate) schema_path: PathBuf,
    pub(crate) schema: SchemaNode,
    pub(crate) data_path: String,
    pub(crate) layout: FixedWidthTargetDraft,
}

impl FixedWidthBoundaryDraft {
    pub(crate) fn from_imported(imported: &ImportedSchema) -> anyhow::Result<Self> {
        let layout = FixedWidthTargetDraft::from_schema(&imported.schema, None)
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            schema_path: imported.path.clone(),
            schema: imported.schema.clone(),
            data_path: String::new(),
            layout,
        })
    }

    pub(crate) fn into_imported(self) -> ImportedSchema {
        ImportedSchema {
            path: self.schema_path,
            schema: self.schema,
        }
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        self.layout
            .layout_for_schema(&self.schema)
            .map_err(anyhow::Error::msg)
            .context("fixed-width layout is incomplete")?;
        if self.data_path.as_bytes().contains(&0) {
            bail!("fixed-width data path cannot contain a null character");
        }
        Ok(())
    }

    pub(crate) fn options(&self) -> anyhow::Result<FormatOptions> {
        self.validate()?;
        Ok(FormatOptions {
            fixed_width: Some(
                self.layout
                    .layout_for_schema(&self.schema)
                    .map_err(anyhow::Error::msg)?,
            ),
            ..FormatOptions::default()
        })
    }

    pub(crate) fn instance_path(&self) -> Option<String> {
        let path = self.data_path.trim();
        (!path.is_empty()).then(|| path.to_owned())
    }
}
