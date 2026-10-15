use std::path::Path;

use anyhow::bail;
use codegen::{CsvInputPolicy, CsvX12BoundaryPolicy, X12BoundaryOptions};
use mapping::{EdiBoundaryKind, Project, TabularBoundaryKind};

/// A retained family is authoritative; suffixes only supply absent identity.
pub(super) fn policy(project: &Project) -> anyhow::Result<CsvX12BoundaryPolicy> {
    let source_kind = project.source_options.tabular_kind;
    let source_extension = extension(project.source_path.as_deref());
    if source_kind != Some(TabularBoundaryKind::Csv)
        && (source_kind.is_some() || source_extension.as_deref() != Some("csv"))
    {
        bail!("generated CSV-to-X12 source requires an explicit CSV format identity");
    }
    let target_kind = project.target_options.edi_kind;
    let target_extension = extension(project.target_path.as_deref());
    if target_kind != Some(EdiBoundaryKind::X12)
        && (target_kind.is_some() || !matches!(target_extension.as_deref(), Some("edi" | "x12")))
    {
        bail!("generated CSV-to-X12 target requires an explicit X12 format identity");
    }
    Ok(CsvX12BoundaryPolicy {
        source: CsvInputPolicy::from_format_options(&project.source_options)?,
        target: X12BoundaryOptions::from_format_options(&project.target_options)?,
    })
}

fn extension(path: Option<&str>) -> Option<String> {
    path.and_then(|path| Path::new(path).extension())
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

/// Only an explicit caller choice changes the target grammar. All retained
/// source and target options are captured by the ordinary policy first.
pub(super) fn policy_with_envelope_profile(
    project: &Project,
    target_envelope: Option<codegen::X12EnvelopeProfile>,
) -> anyhow::Result<CsvX12BoundaryPolicy> {
    let mut policy = policy(project)?;
    if let Some(profile) = target_envelope {
        policy.target.envelope_profile = profile;
    }
    Ok(policy)
}

#[cfg(test)]
mod tests;
