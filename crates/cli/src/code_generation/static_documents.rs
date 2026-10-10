use std::path::Path;

use anyhow::{Context, bail};
use codegen::{
    DocumentBoundaryOptions, NamedDocumentBoundaryOptions, StaticDocumentBoundaryPolicy,
    X12BoundaryOptions,
};
use mapping::{EdiBoundaryKind, FormatOptions, Project};

pub(super) fn policy(project: &Project) -> anyhow::Result<StaticDocumentBoundaryPolicy> {
    Ok(StaticDocumentBoundaryPolicy {
        source: boundary(
            project.source_path.as_deref(),
            &project.source_options,
            "primary source",
        )?,
        target: boundary(
            project.target_path.as_deref(),
            &project.target_options,
            "primary target",
        )?,
        extra_sources: project
            .extra_sources
            .iter()
            .map(|source| {
                Ok(NamedDocumentBoundaryOptions {
                    name: source.name.clone(),
                    options: boundary(
                        Some(&source.path),
                        &source.options,
                        &format!("named source {:?}", source.name),
                    )?,
                })
            })
            .collect::<anyhow::Result<_>>()?,
        extra_targets: project
            .extra_targets
            .iter()
            .map(|target| {
                Ok(NamedDocumentBoundaryOptions {
                    name: target.name.clone(),
                    options: boundary(
                        target.path.as_deref(),
                        &target.options,
                        &format!("named target {:?}", target.name),
                    )?,
                })
            })
            .collect::<anyhow::Result<_>>()?,
    })
}

fn boundary(
    path: Option<&str>,
    options: &FormatOptions,
    owner: &str,
) -> anyhow::Result<DocumentBoundaryOptions> {
    // This profile never infers a physical codec from the schema or contents.
    // Retained explicit family/JSON identity takes precedence over suffixes.
    if let Some(kind) = options.edi_kind {
        if kind != EdiBoundaryKind::X12 {
            bail!("static document {owner} does not support the declared EDI family");
        }
        return X12BoundaryOptions::from_format_options(options)
            .map(DocumentBoundaryOptions::X12)
            .with_context(|| format!("static document {owner} format options"));
    }
    let x12 = if options.json_document {
        false
    } else {
        let extension = path
            .and_then(|path| Path::new(path).extension())
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("edi" | "x12") => true,
            Some("json") => false,
            _ => bail!(
                "static document {owner} requires an explicit X12 or strict JSON format identity"
            ),
        }
    };
    if x12 {
        X12BoundaryOptions::from_format_options(options)
            .map(DocumentBoundaryOptions::X12)
            .with_context(|| format!("static document {owner} format options"))
    } else {
        codegen::validate_x12_json_format_options(options)
            .with_context(|| format!("static document {owner} format options"))?;
        Ok(DocumentBoundaryOptions::Json)
    }
}

#[cfg(test)]
mod tests;
