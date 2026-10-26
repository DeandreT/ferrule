//! CSV parsing adapter for format-neutral primary document hosts.

use ir::{Instance, SchemaNode};
use mapping::PrimaryFileSelection;

use crate::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition,
    PrimarySourceHost,
};

/// Evaluate the filename, select, then load and parse every selected document
/// in order. The first failure ends acquisition before any later request is
/// loaded. Success is one repeated row instance across all documents, each row
/// retaining its own document origin, for a single mapping execution.
pub fn acquire_primary_csv_rows<'a, F: 'a, H: PrimarySourceHost>(
    filename: impl FnOnce() -> Result<String, F> + 'a,
    selection: PrimaryFileSelection,
    host: &'a H,
    schema: &'a SchemaNode,
    options: CsvReadOptions,
    decoding: CsvByteDecodingPolicy,
    maximum: usize,
) -> Result<Instance, CsvAcquisitionError<F, H::Error>>
where
    H::Error: 'a,
{
    let rows = CsvSourceAcquisition::new(
        filename,
        move |name| {
            host.select(name, selection)
                .map(|requests| requests.into_iter().map(Ok))
        },
        move |request| host.load(request),
        schema,
        options,
        decoding,
        maximum,
    )?;
    let mut values = Vec::new();
    for row in rows {
        // The parsed document keeps its rows; the mapping input owns copies.
        values.push(row?.value().clone());
    }
    Ok(Instance::Repeated(values))
}
