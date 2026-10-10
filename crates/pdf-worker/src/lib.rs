//! Pure, bounded PDF worker transport. This crate does not launch a worker,
//! handle paths, install limits, or establish process closure.

mod error;
mod instance;
mod request;
mod result;
mod scan;

use std::io::{self, Write};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use error::{
    DiagnosticText, Diagnostics, HostError, PdfErrorRecord, PdfErrorVariant, encode_pdf_error,
};
pub use instance::{decode_verified_instance, encode_instance};
pub use request::{InputDescriptor, RequestEnvelope, decode_request, encode_request};
pub use result::{
    EffectiveLimits, Outcome, ReadyFrame, ResultEnvelope, StartFrame, decode_ready, decode_result,
    decode_start, encode_ready, encode_result, encode_start,
};

pub const PROTOCOL: &str = "ferrule.pdf-worker/1";
pub const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 64 * 1024;
pub const MAX_INSTANCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CONTROL_FRAME_BYTES: usize = 4096;
pub const MAX_JSON_CONTAINER_DEPTH: usize = 256;
pub const MAX_DIAGNOSTIC_TEXT_BYTES: usize = 4096;
pub const MAX_DIAGNOSTIC_CAUSES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecErrorKind {
    RequestTooLarge,
    InputTooLarge,
    ResultTooLarge,
    InstanceTooLarge,
    ControlFrameTooLarge,
    TransportDepth,
    Syntax,
    TrailingData,
    DuplicateField,
    UnknownField,
    MissingField,
    UnknownVersion,
    UnknownProfile,
    InvalidRequestId,
    InvalidSha256,
    InvalidFixedName,
    InvalidLength,
    IdentityMismatch,
    InvalidLayout,
    RepairDependency,
    UnsupportedInstanceKind,
    UnsupportedXmlOrigin,
    InvalidPairArity,
    InstanceNodes,
    InstanceDepth,
    InstanceValueBytes,
    DigestMismatch,
    LengthMismatch,
    EffectiveLimitsMismatch,
    InvalidStartToken,
    DiagnosticLimit,
    WriteIo,
}

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}: {detail}")]
pub struct CodecError {
    kind: CodecErrorKind,
    byte_offset: Option<usize>,
    detail: String,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl CodecError {
    pub(crate) fn new(kind: CodecErrorKind) -> Self {
        Self {
            kind,
            byte_offset: None,
            detail: String::new(),
            source: None,
        }
    }
    pub(crate) fn at(kind: CodecErrorKind, offset: usize) -> Self {
        Self {
            byte_offset: Some(offset),
            ..Self::new(kind)
        }
    }
    pub(crate) fn sourced<E: std::error::Error + Send + Sync + 'static>(
        kind: CodecErrorKind,
        error: E,
    ) -> Self {
        Self {
            source: Some(Box::new(error)),
            ..Self::new(kind)
        }
    }
    pub fn kind(&self) -> CodecErrorKind {
        self.kind
    }
    pub fn byte_offset(&self) -> Option<usize> {
        self.byte_offset
    }
    pub fn bounded_detail(&self) -> &str {
        &self.detail
    }
}

fn parse_hex<const N: usize>(s: &str, kind: CodecErrorKind) -> Result<[u8; N], CodecError> {
    if s.len() != N * 2 {
        return Err(CodecError::new(kind));
    }
    let mut result = [0; N];
    for (i, pair) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        result[i] = digit(pair[0])
            .and_then(|a| digit(pair[1]).map(|b| a * 16 + b))
            .ok_or_else(|| CodecError::new(kind))?;
    }
    Ok(result)
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    text
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId([u8; 16]);
impl RequestId {
    pub fn parse_hex(s: &str) -> Result<Self, CodecError> {
        parse_hex(s, CodecErrorKind::InvalidRequestId).map(Self)
    }
    pub fn to_hex(self) -> String {
        hex(&self.0)
    }
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sha256Digest([u8; 32]);
impl Sha256Digest {
    pub fn parse_hex(s: &str) -> Result<Self, CodecError> {
        parse_hex(s, CodecErrorKind::InvalidSha256).map(Self)
    }
    pub fn to_hex(self) -> String {
        hex(&self.0)
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    LinuxX86_64V1,
}
impl Profile {
    pub fn as_str(self) -> &'static str {
        "linux-x86_64-v1"
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestIdentity {
    request_id: RequestId,
    profile: Profile,
}
impl RequestIdentity {
    pub fn new(request_id: RequestId, profile: Profile) -> Self {
        Self {
            request_id,
            profile,
        }
    }
    pub fn request_id(&self) -> RequestId {
        self.request_id
    }
    pub fn profile(&self) -> Profile {
        self.profile
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceDescriptor {
    byte_len: u64,
    sha256: Sha256Digest,
}
impl InstanceDescriptor {
    pub fn new(byte_len: u64, sha256: Sha256Digest) -> Result<Self, CodecError> {
        if byte_len > MAX_INSTANCE_BYTES as u64 {
            return Err(CodecError::new(CodecErrorKind::InstanceTooLarge));
        }
        Ok(Self { byte_len, sha256 })
    }
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
    pub fn sha256(&self) -> Sha256Digest {
        self.sha256
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodedBody {
    pub byte_len: u64,
    pub sha256: Sha256Digest,
}

pub(crate) fn identity(fields: &scan::Fields<'_>) -> Result<RequestIdentity, CodecError> {
    if scan::small_text(
        fields.required("protocol")?,
        64,
        CodecErrorKind::UnknownVersion,
    )? != PROTOCOL
    {
        return Err(CodecError::new(CodecErrorKind::UnknownVersion));
    }
    let id = RequestId::parse_hex(&scan::small_text(
        fields.required("request_id")?,
        32,
        CodecErrorKind::InvalidRequestId,
    )?)?;
    if scan::small_text(
        fields.required("profile")?,
        64,
        CodecErrorKind::UnknownProfile,
    )? != Profile::LinuxX86_64V1.as_str()
    {
        return Err(CodecError::new(CodecErrorKind::UnknownProfile));
    }
    Ok(RequestIdentity::new(id, Profile::LinuxX86_64V1))
}

struct Output<'a, W> {
    writer: &'a mut W,
    limit: usize,
    written: usize,
    hash: Sha256,
    limited: bool,
}
impl<W: Write> Write for Output<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.written {
            self.limited = true;
            return Err(io::Error::other("wire byte cap"));
        }
        let n = self.writer.write(bytes)?;
        if n > bytes.len() {
            return Err(io::Error::other("invalid writer count"));
        }
        self.hash.update(&bytes[..n]);
        self.written += n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
pub(crate) fn encode<T: Serialize + ?Sized, W: Write>(
    value: &T,
    writer: &mut W,
    limit: usize,
    kind: CodecErrorKind,
) -> Result<EncodedBody, CodecError> {
    let mut output = Output {
        writer,
        limit,
        written: 0,
        hash: Sha256::new(),
        limited: false,
    };
    if let Err(e) = serde_json::to_writer(&mut output, value) {
        return Err(CodecError::sourced(
            if output.limited {
                kind
            } else {
                CodecErrorKind::WriteIo
            },
            e,
        ));
    }
    Ok(EncodedBody {
        byte_len: output.written as u64,
        sha256: Sha256Digest(output.hash.finalize().into()),
    })
}
