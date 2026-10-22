use super::{GenerateOutcome, GenerateTarget};
use anyhow::{Context, bail};
use std::{fs, io::Read, path::Path};

/// Generate the explicit retained-document CSV profile. The format is decoded
/// by its own lossless codec; ordinary project decoding remains unchanged.
pub fn generate_document_with_csv_document_set_adapters(
    project_path: &Path,
    output_directory: &Path,
    target: GenerateTarget,
    raw_delimited_crlf: bool,
) -> anyhow::Result<GenerateOutcome> {
    if !raw_delimited_crlf {
        bail!("grouped CSV document generation requires explicit raw-delimited CRLF output");
    }
    let mut bytes = Vec::new();
    fs::File::open(project_path)
        .with_context(|| format!("reading {}", project_path.display()))?
        .take(mapping::document_project_file::MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > mapping::document_project_file::MAX_DOCUMENT_BYTES {
        bail!("document project exceeds the bounded file size");
    }
    let document = mapping::document_project_file::decode_bytes(&bytes)
        .with_context(|| format!("decoding document project {}", project_path.display()))?;
    let project = document.project();
    if project.source_options.tabular_kind != Some(mapping::TabularBoundaryKind::Csv)
        || project.target_options.tabular_kind != Some(mapping::TabularBoundaryKind::Csv)
    {
        bail!("grouped document adapters require explicit CSV endpoint identities");
    }
    let mut policy = codegen::CsvDocumentBoundaryPolicy::from_project(project)?;
    policy.target.raw_delimited_crlf = true;
    let lowered = codegen::lower_document(&document)?;
    let artifacts = match target {
        GenerateTarget::Rust { runtime_path } => {
            let runtime =
                fs::canonicalize(runtime_path).context("resolving Rust codegen runtime path")?;
            codegen_rust::emit_document_with_csv_document_set_boundary(
                &lowered,
                &codegen_rust::Options {
                    package_name: "ferrule-generated-mapping".into(),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        runtime
                            .to_str()
                            .context("Rust runtime path is not UTF-8")?
                            .to_owned(),
                    ),
                },
                &policy,
            )?
        }
        GenerateTarget::CSharp => {
            codegen_csharp::emit_document_with_csv_document_set_boundary(&lowered, &policy)?
        }
    };
    super::write_artifacts(output_directory, &artifacts)?;
    Ok(GenerateOutcome {
        output_directory: output_directory.to_path_buf(),
        files_written: artifacts.len(),
    })
}
