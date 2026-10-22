//! Explicit input selection for in-memory project previews.
//!
//! A preview either evaluates caller-owned bytes or acquires the project's
//! configured filesystem inputs. Both return serialized artifacts without
//! staging or publishing a file; only the filesystem form can reach a
//! caller-owned PDF reader.

use std::path::Path;

use crate::{FilesystemPdfReader, PayloadRunOptions, PayloadRunOutcome, RunOptions};

/// Where a project preview's inputs come from.
#[derive(Clone, Copy)]
pub enum ProjectPreviewInputs<'a> {
    /// Bounded host-owned bytes with logical identities. No input file is
    /// opened and no PDF worker is launched; PDFs are parsed in-process.
    Payloads(&'a PayloadRunOptions<'a>),
    /// The configured filesystem inputs, acquired exactly as a file run
    /// acquires them: the primary once, then only the selected static named
    /// sources, then dynamic sources as the mapping requests them.
    Files {
        options: &'a RunOptions<'a>,
        /// Reads configured PDF inputs instead of the in-process parser. The
        /// reader returns only after its extraction is closed.
        pdf_reader: Option<&'a dyn FilesystemPdfReader>,
    },
}

/// Previews an in-memory project from explicitly selected inputs.
pub fn preview_project_value(
    project: &mapping::Project,
    project_path: &Path,
    inputs: ProjectPreviewInputs<'_>,
) -> anyhow::Result<PayloadRunOutcome> {
    match inputs {
        ProjectPreviewInputs::Payloads(options) => {
            crate::run_project_value_payloads(project, project_path, options)
        }
        ProjectPreviewInputs::Files {
            options,
            pdf_reader: None,
        } => crate::preview_project_value_from_files(project, project_path, options),
        ProjectPreviewInputs::Files {
            options,
            pdf_reader: Some(reader),
        } => crate::preview_project_value_from_files_with_pdf_reader(
            project,
            project_path,
            options,
            reader,
        ),
    }
}
