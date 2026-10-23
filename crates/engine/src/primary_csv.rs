use std::{error::Error, fmt};

use format_csv::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition,
    CsvSourceData, PrimarySourceHost,
};
use ir::Instance;
use mapping::DocumentProject;

use crate::{
    EngineError, ExecutionContext, PrimaryFilenameEvaluationError, ValidationIssue,
    evaluate_primary_filename,
};

/// Acquire primary CSV row occurrences, without running a mapping. The host
/// chooses path/wildcard/URI policy and performs every physical load. Filename
/// evaluation and callbacks are deferred until iteration, with no preview fallback.
pub fn acquire_primary_csv<'a, E: 'a, I, Select, Load>(
    document: &'a DocumentProject,
    execution: &ExecutionContext<'a>,
    select: Select,
    load: Load,
    decoding: CsvByteDecodingPolicy,
    maximum: usize,
) -> Result<
    CsvSourceAcquisition<'a, PrimaryFilenameEvaluationError, E>,
    CsvAcquisitionError<PrimaryFilenameEvaluationError, E>,
>
where
    I: Iterator<Item = Result<String, E>> + 'a,
    Select: FnOnce(&str) -> Result<I, E> + 'a,
    Load: FnMut(&str) -> Result<CsvSourceData, E> + 'a,
{
    let execution = *execution;
    CsvSourceAcquisition::new(
        move || evaluate_primary_filename(document, &execution),
        select,
        load,
        &document.project().source,
        CsvReadOptions::from(&document.project().source_options),
        decoding,
        maximum,
    )
}

/// Stage-preserving failure of one complete primary CSV document run.
#[derive(Debug)]
pub enum PrimaryDocumentRunError<E> {
    /// The embedded mapping is invalid; nothing was evaluated, selected or loaded.
    Invalid(Vec<ValidationIssue>),
    /// Filename, selection, load or parse failure; the mapping did not run.
    Acquisition(CsvAcquisitionError<PrimaryFilenameEvaluationError, E>),
    /// Every document was acquired; the single mapping execution failed.
    Mapping(EngineError),
}

impl<E: fmt::Display> fmt::Display for PrimaryDocumentRunError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(issues) => write!(
                f,
                "primary document mapping has {} validation issue(s)",
                issues.len()
            ),
            Self::Acquisition(error) => error.fmt(f),
            Self::Mapping(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static> Error for PrimaryDocumentRunError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invalid(_) => None,
            Self::Acquisition(error) => Some(error),
            Self::Mapping(error) => Some(error),
        }
    }
}

/// Run a primary CSV document plan once over every document its filename
/// selects. The plan's selection mode is passed to `host`, which alone decides
/// where and how requests load. All documents are parsed before the mapping
/// runs, so rows from different documents share one execution context (and
/// one grouping, sort or aggregate). The first acquisition failure stops before
/// later loads. The target instance is returned; publication is the caller's.
pub fn run_primary_csv_document<H: PrimarySourceHost>(
    document: &DocumentProject,
    execution: &ExecutionContext<'_>,
    host: &H,
    decoding: CsvByteDecodingPolicy,
    maximum: usize,
) -> Result<Instance, PrimaryDocumentRunError<H::Error>> {
    // Like document lowering, an unsupported filename is refused before the
    // mapping body is validated.
    if let Err(error) = document.global_filename() {
        return Err(PrimaryDocumentRunError::Acquisition(
            CsvAcquisitionError::Filename(error.into()),
        ));
    }
    let project = document.project();
    let issues = crate::validate(project);
    if !issues.is_empty() {
        return Err(PrimaryDocumentRunError::Invalid(issues));
    }
    let source = format_csv::acquire_primary_csv_rows(
        || evaluate_primary_filename(document, execution),
        document.primary_source().selection,
        host,
        &project.source,
        CsvReadOptions::from(&project.source_options),
        decoding,
        maximum,
    )
    .map_err(PrimaryDocumentRunError::Acquisition)?;
    crate::run_with_context(project, &source, execution).map_err(PrimaryDocumentRunError::Mapping)
}
