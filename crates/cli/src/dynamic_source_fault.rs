//! Typed faults for filesystem-host dynamic-source loads.
//!
//! A read keeps its whole `anyhow` cause. Its phase comes from the typed
//! errors in that chain, never from message text: a policy refusal or an I/O
//! failure means the document was not acquired, invalid UTF-8 is an encoding
//! fault, and a format reader's own error is a document fault. A failure with
//! no typed evidence (a host configuration or transport message) stays a host
//! acquisition failure, as it does in the generated runtimes.

use std::error::Error;
use std::fmt;
use std::io;

use engine::{DynamicSourceCodecKind, DynamicSourceFault};

#[cfg(test)]
#[path = "dynamic_source_fault_tests.rs"]
mod tests;

/// One failed filesystem or HTTP read of a dynamic document.
///
/// Its display text is the reader's outermost message, as the legacy string
/// loader reported it. `source()` continues with the reader's typed causes;
/// [`Self::error`] exposes the complete `anyhow` chain.
#[derive(Debug)]
pub struct DynamicSourceReadError {
    error: anyhow::Error,
}

impl DynamicSourceReadError {
    pub fn error(&self) -> &anyhow::Error {
        &self.error
    }
}

impl fmt::Display for DynamicSourceReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.error)
    }
}

impl Error for DynamicSourceReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        let error: &(dyn Error + 'static) = self.error.as_ref();
        error.source()
    }
}

/// A request for a dynamic source the project does not declare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredDynamicSource {
    pub source_name: String,
}

impl fmt::Display for UndeclaredDynamicSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "dynamic source `{}` is not declared",
            self.source_name
        )
    }
}

impl Error for UndeclaredDynamicSource {}

/// A payload run's request for a logical identity the host did not supply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsuppliedPayloadSource {
    pub source_name: String,
    pub path: String,
}

impl fmt::Display for UnsuppliedPayloadSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "host did not supply payload source `{}` at logical path `{}`",
            self.source_name, self.path
        )
    }
}

impl Error for UnsuppliedPayloadSource {}

pub(crate) fn undeclared(source_name: &str) -> DynamicSourceFault {
    DynamicSourceFault::codec(
        DynamicSourceCodecKind::UndeclaredSource,
        UndeclaredDynamicSource {
            source_name: source_name.to_owned(),
        },
    )
}

pub(crate) fn read_fault(error: anyhow::Error) -> DynamicSourceFault {
    let kind = codec_kind(&error);
    let cause = DynamicSourceReadError { error };
    match kind {
        Some(kind) => DynamicSourceFault::codec(kind, cause),
        None => DynamicSourceFault::acquisition(cause),
    }
}

/// Acquisition evidence wins over a format error that wraps it, so a reader
/// error carrying `NotFound` remains an acquisition failure.
fn codec_kind(error: &anyhow::Error) -> Option<DynamicSourceCodecKind> {
    let mut document = None;
    for link in error.chain() {
        if link.is::<engine::PolicyRefusal>() {
            return None;
        }
        if let Some(error) = link.downcast_ref::<io::Error>() {
            return io_codec_kind(error);
        }
        if is_utf8(link) {
            return Some(DynamicSourceCodecKind::Encoding);
        }
        if document.is_none() && is_format_error(link) {
            document = Some(DynamicSourceCodecKind::Document);
        }
    }
    document
}

/// `InvalidData` is the reader rejecting acquired bytes. Std reports invalid
/// UTF-8 from `read_to_string` as a payload-free `InvalidData`; readers that
/// decode themselves attach the `Utf8Error`.
fn io_codec_kind(error: &io::Error) -> Option<DynamicSourceCodecKind> {
    if error.kind() != io::ErrorKind::InvalidData {
        return None;
    }
    match error.get_ref() {
        Some(inner) if !is_utf8(inner) => Some(DynamicSourceCodecKind::Document),
        _ => Some(DynamicSourceCodecKind::Encoding),
    }
}

fn is_utf8(error: &(dyn Error + 'static)) -> bool {
    error.is::<std::str::Utf8Error>() || error.is::<std::string::FromUtf8Error>()
}

fn is_format_error(error: &(dyn Error + 'static)) -> bool {
    error.is::<format_json::JsonFormatError>()
        || error.is::<serde_json::Error>()
        || error.is::<format_xml::XmlFormatError>()
        || error.is::<format_csv::CsvFormatError>()
        || error.is::<format_xlsx::XlsxFormatError>()
        || error.is::<format_edi::EdiFormatError>()
        || error.is::<format_flextext::FlexTextError>()
        || error.is::<format_protobuf::ProtobufError>()
        || error.is::<format_pdf::PdfError>()
        || error.is::<format_xbrl::XbrlFormatError>()
}
