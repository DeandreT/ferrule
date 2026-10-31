use std::{error::Error, fmt};

use document_source::{
    PrimaryAcquisitionError, PrimaryAssemblyError, PrimarySourceHost, SourceData,
    acquire_primary_documents, assemble_primary_documents,
};
use format_csv::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition,
    CsvSourceDocument, CsvSourceError,
};
use ir::Instance;
use mapping::{DocumentProject, PrimaryDocumentFormat, PrimaryRouteError, PrimaryRouteSide};

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
    Load: FnMut(&str) -> Result<SourceData, E> + 'a,
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

/// Stage-preserving failure of one complete primary document run. `E` is the
/// host's failure and `P` the document parser's.
#[derive(Debug)]
pub enum PrimaryDocumentRunError<E, P = CsvSourceError> {
    /// The saved formats have no primary document route; nothing was
    /// evaluated, selected or loaded.
    Route(PrimaryRouteError),
    /// The embedded mapping is invalid; nothing was evaluated, selected or loaded.
    Invalid(Vec<ValidationIssue>),
    /// Filename, selection, load or parse failure; the mapping did not run.
    Acquisition(PrimaryAcquisitionError<PrimaryFilenameEvaluationError, E, P>),
    /// The parsed documents cannot form the route's one source.
    Assembly(PrimaryAssemblyError),
    /// Every document was acquired; the single mapping execution failed.
    Mapping(EngineError),
}

impl<E: fmt::Display, P: fmt::Display> fmt::Display for PrimaryDocumentRunError<E, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Route(error) => error.fmt(f),
            Self::Invalid(issues) => write!(
                f,
                "primary document mapping has {} validation issue(s)",
                issues.len()
            ),
            Self::Acquisition(error) => error.fmt(f),
            Self::Assembly(error) => error.fmt(f),
            Self::Mapping(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static, P: Error + 'static> Error for PrimaryDocumentRunError<E, P> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invalid(_) => None,
            Self::Route(error) => Some(error),
            Self::Acquisition(error) => Some(error),
            Self::Assembly(error) => Some(error),
            Self::Mapping(error) => Some(error),
        }
    }
}

/// Run a primary document plan once over every document its filename
/// selects. The saved formats decide the route, and an unsupported filename
/// or invalid mapping is refused, before the filename is evaluated or the host
/// is called. The plan's selection mode is passed to `host`, which alone
/// decides where and how requests load; `parse` reads one loaded document.
/// Every document is parsed before the mapping runs once, so documents share
/// one execution context (and one grouping, sort or aggregate). The first
/// acquisition failure stops before later loads. The target instance is
/// returned; publication is the caller's.
pub fn run_primary_document<H: PrimarySourceHost, P>(
    document: &DocumentProject,
    execution: &ExecutionContext<'_>,
    host: &H,
    maximum: usize,
    parse: impl FnMut(&str, SourceData) -> Result<Instance, P>,
) -> Result<Instance, PrimaryDocumentRunError<H::Error, P>> {
    let route = document
        .primary_route()
        .map_err(PrimaryDocumentRunError::Route)?;
    // Like document lowering, an unsupported filename is refused before the
    // mapping body is validated.
    if let Err(error) = document.global_filename() {
        return Err(PrimaryDocumentRunError::Acquisition(
            PrimaryAcquisitionError::Filename(error.into()),
        ));
    }
    let project = document.project();
    let issues = crate::validate_primary_document(document);
    if !issues.is_empty() {
        return Err(PrimaryDocumentRunError::Invalid(issues));
    }
    let documents = acquire_primary_documents(
        || {
            let output = crate::evaluate_output_filename(document, execution)?;
            let input = evaluate_primary_filename(document, execution)?;
            if let Some(output) = output {
                for (node, text) in [
                    (document.primary_source().filename, input.as_str()),
                    (
                        document.output_filename().expect("output filename"),
                        output.as_str(),
                    ),
                ] {
                    DocumentProject::validate_endpoint_text(node, text)?;
                }
            }
            Ok::<_, PrimaryFilenameEvaluationError>(input)
        },
        document.primary_source().selection,
        host,
        maximum,
        parse,
    )
    .map_err(PrimaryDocumentRunError::Acquisition)?;
    let source = assemble_primary_documents(route.shape, documents)
        .map_err(PrimaryDocumentRunError::Assembly)?;
    crate::run_with_context(project, &source, execution).map_err(PrimaryDocumentRunError::Mapping)
}

/// [`run_primary_document`] for a CSV route, parsing each document with the
/// project's CSV dialect and `decoding`. Every row keeps its own document
/// origin and local position.
pub fn run_primary_csv_document<H: PrimarySourceHost>(
    document: &DocumentProject,
    execution: &ExecutionContext<'_>,
    host: &H,
    decoding: CsvByteDecodingPolicy,
    maximum: usize,
) -> Result<Instance, PrimaryDocumentRunError<H::Error>> {
    match document.primary_route() {
        Ok(route) if route.format != PrimaryDocumentFormat::Csv => {
            return Err(PrimaryDocumentRunError::Route(
                PrimaryRouteError::NotAdmitted {
                    side: PrimaryRouteSide::Source,
                    format: route.format.name(),
                },
            ));
        }
        _ => {}
    }
    run_primary_document(
        document,
        execution,
        host,
        maximum,
        primary_csv_parser(document.project(), decoding),
    )
}

/// Parses one loaded primary CSV document with the project's source schema
/// and dialect. Every row keeps its own document origin and local position.
pub fn primary_csv_parser(
    project: &mapping::Project,
    decoding: CsvByteDecodingPolicy,
) -> impl FnMut(&str, SourceData) -> Result<Instance, CsvSourceError> + '_ {
    let options = CsvReadOptions::from(&project.source_options);
    move |request, data| {
        let parsed = CsvSourceDocument::parse_bytes(
            request,
            data.resolved_path.as_deref(),
            &data.bytes,
            &project.source,
            &options,
            decoding,
        )?;
        // The parsed document keeps its rows; the mapping input owns copies.
        Ok(Instance::Repeated(
            parsed.rows().map(|row| row.value().clone()).collect(),
        ))
    }
}
