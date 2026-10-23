//! Complete primary document acquisition: one filename, one host selection,
//! every selected document parsed before any mapping runs.

use ir::{Instance, SchemaNode};
use mapping::PrimaryFileSelection;

use crate::{
    CsvAcquisitionError, CsvByteDecodingPolicy, CsvReadOptions, CsvSourceAcquisition, CsvSourceData,
};

/// The host side of primary acquisition. `select` interprets the evaluated
/// filename under the plan's selection and returns logical requests in load
/// order; `load` returns the bytes of one request. Neither parses or maps.
/// [`crate::LocalPrimaryFiles`] is the explicit local filesystem host.
pub trait PrimarySourceHost {
    type Error;

    fn select(
        &self,
        filename: &str,
        selection: PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error>;

    fn load(&self, request: &str) -> Result<CsvSourceData, Self::Error>;
}

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
