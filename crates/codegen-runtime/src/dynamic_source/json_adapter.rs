//! Generated JSON dynamic-source admission with tagged, cause-retaining failures.

use std::cell::Cell;

use ir::Instance;

use super::{
    DynamicJsonSourceLoader, DynamicSourceCodecKind, DynamicSourceFault, DynamicSourceLoader,
    DynamicSourceResource, MAX_DYNAMIC_SOURCE_BYTES, MAX_DYNAMIC_SOURCE_TOTAL_BYTES,
};

/// Admits one execution's host JSON documents for their declared endpoints.
///
/// Each request calls the host first, then checks the per-document and combined
/// byte limits, strict UTF-8 and the endpoint's embedded schema, in that order.
/// Host faults pass through unchanged; adapter faults retain their causes.
pub struct JsonDynamicSourceAdapter<'a> {
    loader: &'a dyn DynamicJsonSourceLoader,
    schema: fn(&str) -> Option<&'static str>,
    document_limit: u64,
    total_limit: u64,
    total_bytes: Cell<u64>,
}

impl<'a> JsonDynamicSourceAdapter<'a> {
    /// `schema` resolves an exact declared endpoint name to its embedded schema.
    pub fn new(
        loader: &'a dyn DynamicJsonSourceLoader,
        schema: fn(&str) -> Option<&'static str>,
    ) -> Self {
        Self::with_limits(
            loader,
            schema,
            MAX_DYNAMIC_SOURCE_BYTES as u64,
            MAX_DYNAMIC_SOURCE_TOTAL_BYTES as u64,
        )
    }

    pub(super) fn with_limits(
        loader: &'a dyn DynamicJsonSourceLoader,
        schema: fn(&str) -> Option<&'static str>,
        document_limit: u64,
        total_limit: u64,
    ) -> Self {
        Self {
            loader,
            schema,
            document_limit,
            total_limit,
            total_bytes: Cell::new(0),
        }
    }
}

impl DynamicSourceLoader for JsonDynamicSourceAdapter<'_> {
    fn load(&self, source: &str, logical_path: &str) -> Result<Instance, String> {
        self.load_tagged(source, logical_path)
            .map_err(DynamicSourceFault::into_message)
    }

    fn load_tagged(
        &self,
        source: &str,
        logical_path: &str,
    ) -> Result<Instance, DynamicSourceFault> {
        let document = self.loader.load_tagged(source, logical_path)?;
        let bytes = u64::try_from(document.len()).unwrap_or(u64::MAX);
        if bytes > self.document_limit {
            return Err(DynamicSourceFault::resource(
                DynamicSourceResource::DocumentBytes,
                bytes,
                self.document_limit,
            ));
        }
        let total = self.total_bytes.get().saturating_add(bytes);
        if total > self.total_limit {
            return Err(DynamicSourceFault::resource(
                DynamicSourceResource::TotalBytes,
                total,
                self.total_limit,
            ));
        }
        self.total_bytes.set(total);
        let document = std::str::from_utf8(&document)
            .map_err(|error| DynamicSourceFault::codec(DynamicSourceCodecKind::Encoding, error))?;
        let schema =
            (self.schema)(source).ok_or_else(|| DynamicSourceFault::undeclared_source(source))?;
        crate::parse_json_with_format_cause(schema, document)
            .map_err(|error| DynamicSourceFault::codec(DynamicSourceCodecKind::Document, error))
    }
}
