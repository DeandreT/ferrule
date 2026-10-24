//! Saved primary filename document projects: the explicit local filesystem
//! host, one interpreter run over every selected document, and staged
//! publication of the one complete CSV output document.
//!
//! The project's saved source path is never read. Filenames resolve only
//! beneath the host-selected input root, and no network source is reachable.

use std::cell::RefCell;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, bail};
use format_csv::{
    CsvByteDecodingPolicy, CsvSourceData, CsvWriteOptions, LocalFileError, LocalPrimaryFiles,
    PrimarySourceHost,
};
use mapping::{DocumentProject, PrimaryFileSelection};

use crate::{RunOutcome, WrittenOutput, absolute_mapping_path, resolve_run_path};

/// Default number of documents one run may select.
pub const DEFAULT_MAX_PRIMARY_DOCUMENTS: usize = 1024;
/// Upper bound of one complete output document.
pub const MAX_PRIMARY_DOCUMENT_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// Host policy of one primary document run: where documents load from and
/// how many and how large they may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryDocumentPolicy {
    pub input_root: PathBuf,
    pub max_document_bytes: u64,
    pub max_documents: usize,
}

impl PrimaryDocumentPolicy {
    pub fn new(input_root: impl Into<PathBuf>) -> Self {
        Self {
            input_root: input_root.into(),
            max_document_bytes: format_csv::DEFAULT_LOCAL_DOCUMENT_BYTES,
            max_documents: DEFAULT_MAX_PRIMARY_DOCUMENTS,
        }
    }
}

/// Typed failure of one primary document run before publication.
#[derive(Debug)]
pub enum PrimaryDocumentError {
    /// The source or target lacks an explicit CSV format identity.
    Format {
        side: &'static str,
    },
    /// The input root is unavailable.
    Root(LocalFileError),
    Run(engine::PrimaryDocumentRunError<LocalFileError>),
    Output(format_csv::CsvBoundedError),
    /// The mapping's primary target is not a repeated row set.
    OutputShape,
}

impl fmt::Display for PrimaryDocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format { side } => write!(
                f,
                "a primary document {side} requires an explicit CSV format identity"
            ),
            Self::Root(error) => error.fmt(f),
            Self::Run(error) => error.fmt(f),
            Self::Output(error) => error.fmt(f),
            Self::OutputShape => f.write_str("the primary CSV target did not produce rows"),
        }
    }
}

impl std::error::Error for PrimaryDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Root(error) => Some(error),
            Self::Run(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Format { .. } | Self::OutputShape => None,
        }
    }
}

/// One complete output document and the documents that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryDocumentOutput {
    pub bytes: Vec<u8>,
    pub records: usize,
    /// Logical requests loaded, in load order.
    pub documents: Vec<String>,
    /// Resolved locations of the loaded documents, in load order.
    pub locations: Vec<PathBuf>,
}

/// Reads a saved primary filename document project.
pub fn load_document_project(path: &Path) -> anyhow::Result<DocumentProject> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading document project {}", path.display()))?;
    mapping::document_project_file::decode_bytes(&bytes)
        .with_context(|| format!("parsing document project {}", path.display()))
}

/// The CSV side whose identity is neither an explicit CSV kind nor, without
/// any kind, a saved `.csv` path. Shared with CSV document generation.
pub(crate) fn csv_document_identity(project: &mapping::Project) -> Result<(), &'static str> {
    for (side, path, options) in [
        (
            "source",
            project.source_path.as_deref(),
            &project.source_options,
        ),
        (
            "target",
            project.target_path.as_deref(),
            &project.target_options,
        ),
    ] {
        let kind = options.tabular_kind;
        let extension = path
            .and_then(|p| Path::new(p).extension())
            .and_then(|s| s.to_str());
        if kind != Some(mapping::TabularBoundaryKind::Csv)
            && (kind.is_some() || !extension.is_some_and(|s| s.eq_ignore_ascii_case("csv")))
        {
            return Err(side);
        }
    }
    Ok(())
}

/// The refusals of a document run that need no input: both CSV identities,
/// the filename plan and the embedded mapping.
pub fn check_primary_csv_document(document: &DocumentProject) -> anyhow::Result<()> {
    csv_document_identity(document.project())
        .map_err(|side| PrimaryDocumentError::Format { side })?;
    document.global_filename()?;
    let issues = engine::validate(document.project());
    if !issues.is_empty() {
        bail!(
            "primary document mapping has {} validation issue(s)",
            issues.len()
        );
    }
    Ok(())
}

struct RecordingHost {
    files: LocalPrimaryFiles,
    documents: RefCell<Vec<(String, PathBuf)>>,
}

impl PrimarySourceHost for RecordingHost {
    type Error = LocalFileError;

    fn select(
        &self,
        filename: &str,
        selection: PrimaryFileSelection,
    ) -> Result<Vec<String>, LocalFileError> {
        self.files.select(filename, selection)
    }

    fn load(&self, request: &str) -> Result<CsvSourceData, LocalFileError> {
        let data = self.files.load(request)?;
        if let Some(location) = &data.resolved_path {
            self.documents
                .borrow_mut()
                .push((request.to_owned(), PathBuf::from(location)));
        }
        Ok(data)
    }
}

/// Runs `document` once over every document its filename selects beneath the
/// policy's input root and serializes the one complete CSV output. Nothing is
/// published. `project_path` is the active mapping path seen by the mapping.
pub fn execute_primary_csv_document(
    document: &DocumentProject,
    project_path: &Path,
    policy: &PrimaryDocumentPolicy,
    runtime_parameters: Option<&engine::RuntimeParameters>,
) -> Result<PrimaryDocumentOutput, PrimaryDocumentError> {
    let project = document.project();
    csv_document_identity(project).map_err(|side| PrimaryDocumentError::Format { side })?;
    let host = RecordingHost {
        files: LocalPrimaryFiles::new(&policy.input_root)
            .map_err(PrimaryDocumentError::Root)?
            .with_max_document_bytes(policy.max_document_bytes),
        documents: RefCell::new(Vec::new()),
    };
    let current_datetime = jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string();
    let mut execution =
        engine::ExecutionContext::new(project_path).with_current_datetime(&current_datetime);
    if let Some(parameters) = runtime_parameters {
        execution = execution.with_parameters(parameters);
    }
    let output = engine::run_primary_csv_document(
        document,
        &execution,
        &host,
        CsvByteDecodingPolicy::StrictUtf8,
        policy.max_documents,
    )
    .map_err(PrimaryDocumentError::Run)?;
    let ir::Instance::Repeated(rows) = output else {
        return Err(PrimaryDocumentError::OutputShape);
    };
    let bytes = format_csv::to_bytes_with_options_bounded(
        &project.target,
        &rows,
        &CsvWriteOptions::from(&project.target_options),
        MAX_PRIMARY_DOCUMENT_OUTPUT_BYTES,
    )
    .map_err(PrimaryDocumentError::Output)?;
    let (documents, locations) = host.documents.into_inner().into_iter().unzip();
    Ok(PrimaryDocumentOutput {
        bytes,
        records: rows.len(),
        documents,
        locations,
    })
}

/// Loads a saved document project, runs it and publishes the complete output
/// to `output_path`, or to the project's saved target path when omitted. The
/// output never replaces the project or a document the run read.
pub fn run_primary_csv_document_file(
    project_path: &Path,
    output_path: Option<&Path>,
    policy: &PrimaryDocumentPolicy,
    runtime_parameters: Option<&engine::RuntimeParameters>,
) -> anyhow::Result<RunOutcome> {
    let document = load_document_project(project_path)?;
    let output_path = resolve_run_path(
        project_path,
        output_path,
        document.project().target_path.as_deref(),
        "output",
        "target_path",
        false,
    )?;
    let active = absolute_mapping_path(project_path)?;
    let output = execute_primary_csv_document(&document, &active, policy, runtime_parameters)?;
    let destination = comparable(&output_path);
    if destination == comparable(project_path)
        || output
            .locations
            .iter()
            .any(|location| comparable(location) == destination)
    {
        bail!(
            "output {} would replace the project or one of its input documents",
            output_path.display()
        );
    }
    publish(&output_path, &output.bytes)?;
    let written = WrittenOutput {
        name: document.project().target.name.clone(),
        records_written: output.records,
        path: output_path.clone(),
    };
    Ok(RunOutcome {
        records_written: output.records,
        input_path: policy.input_root.clone(),
        output_path,
        primary_outputs: Vec::new(),
        extra_outputs: Vec::new(),
        artifacts: vec![written],
    })
}

/// The canonical form of an existing path, or of its existing parent joined
/// with its file name.
fn comparable(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            std::fs::canonicalize(parent)
                .map(|parent| parent.join(name))
                .unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

static STAGE: AtomicU64 = AtomicU64::new(0);

/// Writes a complete sibling file, then renames it over the destination.
fn publish(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("output destination {} cannot be a symlink", path.display())
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!(
                "output destination {} is not a regular file",
                path.display()
            )
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("checking output {}", path.display()));
        }
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating output directory {}", parent.display()))?;
    let name = path
        .file_name()
        .with_context(|| format!("output {} has no file name", path.display()))?
        .to_string_lossy();
    for _ in 0..64 {
        let staged = parent.join(format!(
            ".{name}.ferrule-stage-{}-{}",
            std::process::id(),
            STAGE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("staging output {}", staged.display()));
            }
        };
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| {
                drop(file);
                std::fs::rename(&staged, path)
            });
        if let Err(error) = result {
            let _ = std::fs::remove_file(&staged);
            return Err(error).with_context(|| format!("publishing output {}", path.display()));
        }
        return Ok(());
    }
    bail!("could not reserve a staging file for {}", path.display())
}
