use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use mapping::{FlexTextLayout, FormatOptions};

/// A validated layout and its exact schema are loaded once during staging.
pub(crate) struct FlexTextBoundaryDraft {
    pub(crate) configuration_path: PathBuf,
    pub(crate) instance_path: String,
    layout: FlexTextLayout,
    schema: ir::SchemaNode,
}

impl FlexTextBoundaryDraft {
    pub(crate) fn from_configuration(path: PathBuf) -> anyhow::Result<Self> {
        if !is_flextext_configuration(&path) {
            bail!("FlexText configuration must have a .mft extension");
        }
        let layout =
            mfd::import_flextext_configuration(&path).context("loading FlexText configuration")?;
        let schema = layout.schema();
        Ok(Self {
            configuration_path: path,
            instance_path: String::new(),
            layout,
            schema,
        })
    }

    pub(crate) fn layout(&self) -> &FlexTextLayout {
        &self.layout
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if self.instance_path.as_bytes().contains(&0) {
            bail!("FlexText file path cannot contain a null character");
        }
        Ok(())
    }

    pub(crate) fn schema(&self) -> anyhow::Result<ir::SchemaNode> {
        self.validate()?;
        Ok(self.schema.clone())
    }

    pub(crate) fn options(&self) -> anyhow::Result<FormatOptions> {
        self.validate()?;
        Ok(FormatOptions {
            flextext: Some(self.layout.clone()),
            ..FormatOptions::default()
        })
    }

    pub(crate) fn instance_path(&self) -> Option<String> {
        let path = self.instance_path.trim();
        (!path.is_empty()).then(|| path.to_owned())
    }
}

pub(crate) fn is_flextext_configuration(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("mft"))
}
