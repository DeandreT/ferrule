use format_csv::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition, CsvSourceData,
};
use mapping::DocumentProject;

use crate::{ExecutionContext, PrimaryFilenameEvaluationError, evaluate_primary_filename};

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
