use std::path::Path;

use anyhow::bail;
use codegen::{CsvInputPolicy, CsvJsonBoundaryError, CsvJsonBoundaryOwner, CsvJsonBoundaryPolicy};
use mapping::{FormatOptions, Project, TabularBoundaryKind};

/// Retained physical identities are authoritative; suffixes fill absent identity.
pub(super) fn policy(project: &Project) -> anyhow::Result<CsvJsonBoundaryPolicy> {
    let source_kind = project.source_options.tabular_kind;
    if source_kind != Some(TabularBoundaryKind::Csv)
        && (source_kind.is_some()
            || extension(project.source_path.as_deref()).as_deref() != Some("csv"))
    {
        bail!("generated CSV-to-JSON source requires an explicit CSV format identity");
    }
    let source = CsvInputPolicy::from_input_format_options(&project.source_options)
        .map_err(CsvJsonBoundaryError::Source)?;
    target(
        project.target_path.as_deref(),
        &project.target_options,
        CsvJsonBoundaryOwner::Target,
        "primary target",
    )?;
    for (index, side) in project.extra_targets.iter().enumerate() {
        target(
            side.path.as_deref(),
            &side.options,
            CsvJsonBoundaryOwner::NamedTarget {
                index,
                name: side.name.clone(),
            },
            &format!("named target {:?}", side.name),
        )?;
    }
    Ok(CsvJsonBoundaryPolicy {
        source,
        extra_target_names: project
            .extra_targets
            .iter()
            .map(|target| target.name.clone())
            .collect(),
    })
}

fn target(
    path: Option<&str>,
    options: &FormatOptions,
    owner: CsvJsonBoundaryOwner,
    label: &str,
) -> anyhow::Result<()> {
    if options.edi_kind.is_some()
        || options.tabular_kind.is_some()
        || (!options.json_document && extension(path).as_deref() != Some("json"))
    {
        bail!("generated CSV-to-JSON {label} requires an explicit strict JSON format identity");
    }
    codegen::validate_x12_json_format_options(options).map_err(|error| {
        CsvJsonBoundaryError::JsonFormatOptions {
            owner,
            error: Box::new(error),
        }
    })?;
    Ok(())
}

fn extension(path: Option<&str>) -> Option<String> {
    path.and_then(|path| Path::new(path).extension())
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests;
