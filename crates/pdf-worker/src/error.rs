use std::error::Error;
use std::fmt::{self, Write as _};
use std::io::{self, Write};

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{
    CodecError, CodecErrorKind, EncodedBody, MAX_DIAGNOSTIC_CAUSES, MAX_DIAGNOSTIC_TEXT_BYTES,
    MAX_RESULT_BYTES, PROTOCOL, RequestIdentity, Sha256Digest, scan,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticText {
    text: String,
    truncated: bool,
}
impl DiagnosticText {
    pub fn new(text: String, truncated: bool) -> Result<Self, CodecError> {
        if text.len() > MAX_DIAGNOSTIC_TEXT_BYTES {
            return Err(CodecError::new(CodecErrorKind::DiagnosticLimit));
        }
        Ok(Self { text, truncated })
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostics {
    display: DiagnosticText,
    causes: Vec<DiagnosticText>,
    cause_chain_truncated: bool,
}
impl Diagnostics {
    pub fn new(
        display: DiagnosticText,
        causes: Vec<DiagnosticText>,
        cause_chain_truncated: bool,
    ) -> Result<Self, CodecError> {
        let sum = causes.iter().try_fold(display.text.len(), |sum, text| {
            sum.checked_add(text.text.len())
        });
        if causes.len() > MAX_DIAGNOSTIC_CAUSES || sum.is_none_or(|n| n > MAX_DIAGNOSTIC_TEXT_BYTES)
        {
            return Err(CodecError::new(CodecErrorKind::DiagnosticLimit));
        }
        Ok(Self {
            display,
            causes,
            cause_chain_truncated,
        })
    }
    pub fn display(&self) -> &DiagnosticText {
        &self.display
    }
    pub fn causes(&self) -> &[DiagnosticText] {
        &self.causes
    }
    pub fn cause_chain_truncated(&self) -> bool {
        self.cause_chain_truncated
    }
    pub fn empty() -> Self {
        Self {
            display: DiagnosticText {
                text: String::new(),
                truncated: false,
            },
            causes: Vec::new(),
            cause_chain_truncated: false,
        }
    }
}
struct TextSink {
    text: String,
    remaining: usize,
    truncated: bool,
}
impl fmt::Write for TextSink {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        if s.len() <= self.remaining {
            self.text.push_str(s);
            self.remaining -= s.len();
            return Ok(());
        }
        let mut end = self.remaining.min(s.len());
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&s[..end]);
        self.remaining -= end;
        self.truncated = true;
        Err(fmt::Error)
    }
}
fn capture_text(error: &dyn Error, remaining: &mut usize) -> DiagnosticText {
    let mut sink = TextSink {
        text: String::new(),
        remaining: *remaining,
        truncated: false,
    };
    if write!(&mut sink, "{error}").is_err() {
        sink.truncated = true;
    }
    *remaining = sink.remaining;
    DiagnosticText {
        text: sink.text,
        truncated: sink.truncated,
    }
}
// Test-only generic observations use this same bounded borrowed routine; it is
// intentionally not a public generic-error conversion API.
pub(crate) fn capture(error: &dyn Error) -> Diagnostics {
    let mut remaining = MAX_DIAGNOSTIC_TEXT_BYTES;
    let display = capture_text(error, &mut remaining);
    let mut causes = Vec::new();
    let mut next = error.source();
    while let Some(cause) = next {
        if causes.len() == MAX_DIAGNOSTIC_CAUSES {
            break;
        }
        causes.push(capture_text(cause, &mut remaining));
        next = cause.source();
    }
    Diagnostics {
        display,
        causes,
        cause_chain_truncated: next.is_some(),
    }
}
fn diagnostic(bytes: &[u8], remaining: &mut usize) -> Result<DiagnosticText, CodecError> {
    let fields = scan::object(bytes, &["text", "truncated"])?;
    fields.exact(&["text", "truncated"])?;
    let raw = fields.required("text")?;
    let n = scan::text_len(raw)?;
    if n > *remaining {
        return Err(CodecError::new(CodecErrorKind::DiagnosticLimit));
    }
    *remaining -= n;
    DiagnosticText::new(scan::text(raw)?, boolean(fields.required("truncated")?)?)
}
pub(crate) fn boolean(bytes: &[u8]) -> Result<bool, CodecError> {
    match bytes {
        b"true" => Ok(true),
        b"false" => Ok(false),
        _ => Err(CodecError::new(CodecErrorKind::Syntax)),
    }
}
pub(crate) fn decode_diagnostics(bytes: &[u8]) -> Result<Diagnostics, CodecError> {
    let fields = scan::object(bytes, &["display", "causes", "cause_chain_truncated"])?;
    fields.exact(&["display", "causes", "cause_chain_truncated"])?;
    let mut remaining = MAX_DIAGNOSTIC_TEXT_BYTES;
    let display = diagnostic(fields.required("display")?, &mut remaining)?;
    let mut array = scan::Array::new(fields.required("causes")?)?;
    let mut causes = Vec::new();
    while let Some(raw) = array.next()? {
        if causes.len() == MAX_DIAGNOSTIC_CAUSES {
            return Err(CodecError::new(CodecErrorKind::DiagnosticLimit));
        }
        causes.push(diagnostic(raw, &mut remaining)?);
    }
    Diagnostics::new(
        display,
        causes,
        boolean(fields.required("cause_chain_truncated")?)?,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfErrorVariant {
    RepairDependency,
    Io,
    InputTooLarge,
    TooManyPages,
    TooManyEvents,
    DecodedTextTooLarge,
    TooManyOutputNodes,
    ValueTooLarge,
    InstanceTooDeep,
    InvalidPdf,
    InvalidLayout,
    InvalidCandidateRegion,
}
impl PdfErrorVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepairDependency => "repair_dependency",
            Self::Io => "io",
            Self::InputTooLarge => "input_too_large",
            Self::TooManyPages => "too_many_pages",
            Self::TooManyEvents => "too_many_events",
            Self::DecodedTextTooLarge => "decoded_text_too_large",
            Self::TooManyOutputNodes => "too_many_output_nodes",
            Self::ValueTooLarge => "value_too_large",
            Self::InstanceTooDeep => "instance_too_deep",
            Self::InvalidPdf => "invalid_pdf",
            Self::InvalidLayout => "invalid_layout",
            Self::InvalidCandidateRegion => "invalid_candidate_region",
        }
    }
    pub(crate) fn parse(s: &str) -> Result<Self, CodecError> {
        match s {
            "repair_dependency" => Ok(Self::RepairDependency),
            "io" => Ok(Self::Io),
            "input_too_large" => Ok(Self::InputTooLarge),
            "too_many_pages" => Ok(Self::TooManyPages),
            "too_many_events" => Ok(Self::TooManyEvents),
            "decoded_text_too_large" => Ok(Self::DecodedTextTooLarge),
            "too_many_output_nodes" => Ok(Self::TooManyOutputNodes),
            "value_too_large" => Ok(Self::ValueTooLarge),
            "instance_too_deep" => Ok(Self::InstanceTooDeep),
            "invalid_pdf" => Ok(Self::InvalidPdf),
            "invalid_layout" => Ok(Self::InvalidLayout),
            "invalid_candidate_region" => Ok(Self::InvalidCandidateRegion),
            _ => Err(CodecError::new(CodecErrorKind::UnknownField)),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfErrorRecord {
    RepairDependency {
        kind: mapping::PdfRepairDependency,
    },
    Io {
        kind: String,
        raw_os_error: Option<i32>,
    },
    InputTooLarge,
    TooManyPages,
    TooManyEvents,
    DecodedTextTooLarge,
    TooManyOutputNodes,
    ValueTooLarge {
        path: String,
    },
    InstanceTooDeep,
    InvalidPdf {
        message: String,
    },
    InvalidLayout {
        message: String,
    },
    InvalidCandidateRegion,
}
impl PdfErrorRecord {
    pub fn variant(&self) -> PdfErrorVariant {
        match self {
            Self::RepairDependency { .. } => PdfErrorVariant::RepairDependency,
            Self::Io { .. } => PdfErrorVariant::Io,
            Self::InputTooLarge => PdfErrorVariant::InputTooLarge,
            Self::TooManyPages => PdfErrorVariant::TooManyPages,
            Self::TooManyEvents => PdfErrorVariant::TooManyEvents,
            Self::DecodedTextTooLarge => PdfErrorVariant::DecodedTextTooLarge,
            Self::TooManyOutputNodes => PdfErrorVariant::TooManyOutputNodes,
            Self::ValueTooLarge { .. } => PdfErrorVariant::ValueTooLarge,
            Self::InstanceTooDeep => PdfErrorVariant::InstanceTooDeep,
            Self::InvalidPdf { .. } => PdfErrorVariant::InvalidPdf,
            Self::InvalidLayout { .. } => PdfErrorVariant::InvalidLayout,
            Self::InvalidCandidateRegion => PdfErrorVariant::InvalidCandidateRegion,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), CodecError> {
        if let Self::Io { kind, .. } = self
            && (kind.is_empty()
                || kind.len() > 64
                || !kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        {
            return Err(CodecError::new(CodecErrorKind::UnknownField));
        }
        Ok(())
    }
}
impl Serialize for PdfErrorRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("variant", self.variant().as_str())?;
        match self {
            Self::RepairDependency {
                kind: mapping::PdfRepairDependency::ObjectFind,
            } => map.serialize_entry("kind", "object_find")?,
            Self::Io { kind, raw_os_error } => {
                map.serialize_entry("kind", kind)?;
                map.serialize_entry("raw_os_error", raw_os_error)?;
            }
            Self::ValueTooLarge { path } => map.serialize_entry("path", path)?,
            Self::InvalidPdf { message } | Self::InvalidLayout { message } => {
                map.serialize_entry("message", message)?
            }
            _ => {}
        }
        map.end()
    }
}
pub(crate) fn decode_pdf(bytes: &[u8]) -> Result<PdfErrorRecord, CodecError> {
    let f = scan::object(
        bytes,
        &["variant", "kind", "raw_os_error", "path", "message"],
    )?;
    let variant = PdfErrorVariant::parse(&scan::text(f.required("variant")?)?)?;
    let value = match variant {
        PdfErrorVariant::RepairDependency => {
            f.exact(&["variant", "kind"])?;
            if scan::text(f.required("kind")?)? != "object_find" {
                return Err(CodecError::new(CodecErrorKind::UnknownField));
            }
            PdfErrorRecord::RepairDependency {
                kind: mapping::PdfRepairDependency::ObjectFind,
            }
        }
        PdfErrorVariant::Io => {
            f.exact(&["variant", "kind", "raw_os_error"])?;
            let raw = f.required("raw_os_error")?;
            let raw_os_error = serde_json::from_slice(raw)
                .map_err(|e| CodecError::sourced(CodecErrorKind::InvalidLength, e))?;
            PdfErrorRecord::Io {
                kind: scan::text(f.required("kind")?)?,
                raw_os_error,
            }
        }
        PdfErrorVariant::ValueTooLarge => {
            f.exact(&["variant", "path"])?;
            PdfErrorRecord::ValueTooLarge {
                path: scan::text(f.required("path")?)?,
            }
        }
        PdfErrorVariant::InvalidPdf => {
            f.exact(&["variant", "message"])?;
            PdfErrorRecord::InvalidPdf {
                message: scan::text(f.required("message")?)?,
            }
        }
        PdfErrorVariant::InvalidLayout => {
            f.exact(&["variant", "message"])?;
            PdfErrorRecord::InvalidLayout {
                message: scan::text(f.required("message")?)?,
            }
        }
        v => {
            f.exact(&["variant"])?;
            match v {
                PdfErrorVariant::InputTooLarge => PdfErrorRecord::InputTooLarge,
                PdfErrorVariant::TooManyPages => PdfErrorRecord::TooManyPages,
                PdfErrorVariant::TooManyEvents => PdfErrorRecord::TooManyEvents,
                PdfErrorVariant::DecodedTextTooLarge => PdfErrorRecord::DecodedTextTooLarge,
                PdfErrorVariant::TooManyOutputNodes => PdfErrorRecord::TooManyOutputNodes,
                PdfErrorVariant::InstanceTooDeep => PdfErrorRecord::InstanceTooDeep,
                PdfErrorVariant::InvalidCandidateRegion => PdfErrorRecord::InvalidCandidateRegion,
                _ => return Err(CodecError::new(CodecErrorKind::UnknownField)),
            }
        }
    };
    value.validate()?;
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    UnsupportedHost,
    ResourceSetup,
    SnapshotMismatch,
    RequestTooLarge,
    InstanceTooLarge,
    ProtocolError,
    ErrorEnvelopeTooLarge {
        original_pdf_variant: PdfErrorVariant,
        attempted_byte_len: u64,
        attempted_sha256: Sha256Digest,
    },
}
impl Serialize for HostError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let kind = match self {
            Self::UnsupportedHost => "unsupported_host",
            Self::ResourceSetup => "resource_setup",
            Self::SnapshotMismatch => "snapshot_mismatch",
            Self::RequestTooLarge => "request_too_large",
            Self::InstanceTooLarge => "instance_too_large",
            Self::ProtocolError => "protocol_error",
            Self::ErrorEnvelopeTooLarge { .. } => "error_envelope_too_large",
        };
        map.serialize_entry("variant", kind)?;
        if let Self::ErrorEnvelopeTooLarge {
            original_pdf_variant,
            attempted_byte_len,
            attempted_sha256,
        } = self
        {
            map.serialize_entry("original_pdf_variant", original_pdf_variant.as_str())?;
            map.serialize_entry("attempted_byte_len", attempted_byte_len)?;
            map.serialize_entry("attempted_sha256", &attempted_sha256.to_hex())?;
        }
        map.end()
    }
}
pub(crate) fn decode_host(bytes: &[u8]) -> Result<HostError, CodecError> {
    let f = scan::object(
        bytes,
        &[
            "variant",
            "original_pdf_variant",
            "attempted_byte_len",
            "attempted_sha256",
        ],
    )?;
    let kind = scan::text(f.required("variant")?)?;
    if kind == "error_envelope_too_large" {
        f.exact(&[
            "variant",
            "original_pdf_variant",
            "attempted_byte_len",
            "attempted_sha256",
        ])?;
        let len = scan::uint(f.required("attempted_byte_len")?)?;
        if len <= MAX_RESULT_BYTES as u64 {
            return Err(CodecError::new(CodecErrorKind::InvalidLength));
        }
        return Ok(HostError::ErrorEnvelopeTooLarge {
            original_pdf_variant: PdfErrorVariant::parse(&scan::text(
                f.required("original_pdf_variant")?,
            )?)?,
            attempted_byte_len: len,
            attempted_sha256: Sha256Digest::parse_hex(&scan::text(
                f.required("attempted_sha256")?,
            )?)?,
        });
    }
    f.exact(&["variant"])?;
    match kind.as_str() {
        "unsupported_host" => Ok(HostError::UnsupportedHost),
        "resource_setup" => Ok(HostError::ResourceSetup),
        "snapshot_mismatch" => Ok(HostError::SnapshotMismatch),
        "request_too_large" => Ok(HostError::RequestTooLarge),
        "instance_too_large" => Ok(HostError::InstanceTooLarge),
        "protocol_error" => Ok(HostError::ProtocolError),
        _ => Err(CodecError::new(CodecErrorKind::UnknownField)),
    }
}
struct BorrowedPdf<'a>(&'a format_pdf::PdfError);
fn variant(error: &format_pdf::PdfError) -> PdfErrorVariant {
    match error {
        format_pdf::PdfError::RepairDependency(_) => PdfErrorVariant::RepairDependency,
        format_pdf::PdfError::Io(_) => PdfErrorVariant::Io,
        format_pdf::PdfError::InputTooLarge => PdfErrorVariant::InputTooLarge,
        format_pdf::PdfError::TooManyPages => PdfErrorVariant::TooManyPages,
        format_pdf::PdfError::TooManyEvents => PdfErrorVariant::TooManyEvents,
        format_pdf::PdfError::DecodedTextTooLarge => PdfErrorVariant::DecodedTextTooLarge,
        format_pdf::PdfError::TooManyOutputNodes => PdfErrorVariant::TooManyOutputNodes,
        format_pdf::PdfError::ValueTooLarge(_) => PdfErrorVariant::ValueTooLarge,
        format_pdf::PdfError::InstanceTooDeep => PdfErrorVariant::InstanceTooDeep,
        format_pdf::PdfError::InvalidPdf(_) => PdfErrorVariant::InvalidPdf,
        format_pdf::PdfError::InvalidLayout(_) => PdfErrorVariant::InvalidLayout,
        format_pdf::PdfError::InvalidCandidateRegion => PdfErrorVariant::InvalidCandidateRegion,
    }
}
impl Serialize for BorrowedPdf<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("variant", variant(self.0).as_str())?;
        match self.0 {
            format_pdf::PdfError::RepairDependency(mapping::PdfRepairDependency::ObjectFind) => {
                map.serialize_entry("kind", "object_find")?
            }
            format_pdf::PdfError::Io(e) => {
                map.serialize_entry("kind", &format!("{:?}", e.kind()))?;
                map.serialize_entry("raw_os_error", &e.raw_os_error())?;
            }
            format_pdf::PdfError::ValueTooLarge(path) => map.serialize_entry("path", path)?,
            format_pdf::PdfError::InvalidPdf(message)
            | format_pdf::PdfError::InvalidLayout(message) => {
                map.serialize_entry("message", message)?
            }
            _ => {}
        }
        map.end()
    }
}
#[derive(Serialize)]
struct ErrorOutcome<'a, T: ?Sized> {
    kind: &'static str,
    error: &'a T,
    diagnostics: &'a Diagnostics,
}
#[derive(Serialize)]
struct ErrorWire<'a, T: ?Sized> {
    protocol: &'static str,
    request_id: String,
    profile: &'static str,
    outcome: ErrorOutcome<'a, T>,
}
struct Attempt {
    count: u64,
    hash: Sha256,
    prefix: Vec<u8>,
}
impl Write for Attempt {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.count = self
            .count
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("error byte count overflow"))?;
        self.hash.update(bytes);
        let keep = bytes.len().min(MAX_RESULT_BYTES - self.prefix.len());
        self.prefix.extend_from_slice(&bytes[..keep]);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn bounded_pdf<T: Serialize + ?Sized, W: Write>(
    identity: &RequestIdentity,
    error: &T,
    original: PdfErrorVariant,
    diagnostics: &Diagnostics,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    let wire = ErrorWire {
        protocol: PROTOCOL,
        request_id: identity.request_id().to_hex(),
        profile: identity.profile().as_str(),
        outcome: ErrorOutcome {
            kind: "pdf_error",
            error,
            diagnostics,
        },
    };
    let mut attempt = Attempt {
        count: 0,
        hash: Sha256::new(),
        prefix: Vec::with_capacity(MAX_RESULT_BYTES),
    };
    serde_json::to_writer(&mut attempt, &wire)
        .map_err(|e| CodecError::sourced(CodecErrorKind::WriteIo, e))?;
    let sha256 = Sha256Digest(attempt.hash.finalize().into());
    if attempt.count <= MAX_RESULT_BYTES as u64 {
        output
            .write_all(&attempt.prefix)
            .map_err(|e| CodecError::sourced(CodecErrorKind::WriteIo, e))?;
        return Ok(EncodedBody {
            byte_len: attempt.count,
            sha256,
        });
    }
    let fallback = HostError::ErrorEnvelopeTooLarge {
        original_pdf_variant: original,
        attempted_byte_len: attempt.count,
        attempted_sha256: sha256,
    };
    crate::encode(
        &ErrorWire {
            protocol: PROTOCOL,
            request_id: identity.request_id().to_hex(),
            profile: identity.profile().as_str(),
            outcome: ErrorOutcome {
                kind: "host_error",
                error: &fallback,
                diagnostics,
            },
        },
        output,
        MAX_RESULT_BYTES,
        CodecErrorKind::ResultTooLarge,
    )
}
pub fn encode_pdf_error<W: Write>(
    identity: &RequestIdentity,
    error: &format_pdf::PdfError,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    let diagnostics = capture(error);
    bounded_pdf(
        identity,
        &BorrowedPdf(error),
        variant(error),
        &diagnostics,
        output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Cause {
        text: String,
        next: Option<Box<Cause>>,
    }
    impl fmt::Display for Cause {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.text)
        }
    }
    impl Error for Cause {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.next.as_deref().map(|v| v as &dyn Error)
        }
    }
    #[test]
    fn e15_whole_utf8_prefix_and_e16_a09_complete_chain() {
        let e = Cause {
            text: format!("{}雪suffix", "A".repeat(4095)),
            next: None,
        };
        let actual = capture(&e);
        assert_eq!(actual.display.text.len(), 4095);
        assert_eq!(actual.display.text, "A".repeat(4095));
        assert!(actual.display.truncated);
        assert!(actual.causes.is_empty());
        assert!(!actual.cause_chain_truncated);
        println!("E15 PASS");
        let supplement: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/wire-vectors-supplement.json"
        ))
        .expect("frozen supplemental catalog");
        let expected = &supplement["rows"][8]["independent_expected"];
        let mut next = None;
        for i in (0..=9).rev() {
            next = Some(Box::new(Cause {
                text: format!("C{i}"),
                next,
            }));
        }
        let root = next.expect("independent C0 chain");
        let actual = capture(root.as_ref());
        let observed = serde_json::to_value(&actual).expect("bounded diagnostic encoding");
        let mut frozen = expected.clone();
        frozen
            .as_object_mut()
            .expect("oracle object")
            .remove("aggregate_retained_utf8_bytes");
        assert!(observed == frozen, "E16/A09 complete Diagnostics mismatch");
        assert_eq!(
            actual.display.text.len() + actual.causes.iter().map(|v| v.text.len()).sum::<usize>(),
            18
        );
        println!("E16 PASS (deterministic root C0 from additive authority)");
        println!("A09 PASS");
    }
}
