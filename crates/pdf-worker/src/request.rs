use std::io::Write;

use mapping::PdfLayout;
use serde::Serialize;

use crate::{
    CodecError, CodecErrorKind, EncodedBody, MAX_REQUEST_BYTES, PROTOCOL, RequestIdentity,
    Sha256Digest, encode, identity, scan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputDescriptor {
    byte_len: u64,
    sha256: Sha256Digest,
}
impl InputDescriptor {
    pub fn new(byte_len: u64, sha256: Sha256Digest) -> Result<Self, CodecError> {
        if byte_len > format_pdf::MAX_INPUT_BYTES as u64 {
            return Err(CodecError::new(CodecErrorKind::InputTooLarge));
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
#[derive(Debug, Clone, PartialEq)]
pub struct RequestEnvelope {
    identity: RequestIdentity,
    layout: PdfLayout,
    input: InputDescriptor,
}
impl RequestEnvelope {
    pub fn new(
        identity: RequestIdentity,
        layout: PdfLayout,
        byte_len: u64,
        sha256: Sha256Digest,
    ) -> Result<Self, CodecError> {
        if layout.repair_dependency().is_some() {
            return Err(CodecError::new(CodecErrorKind::RepairDependency));
        }
        let input = InputDescriptor::new(byte_len, sha256)?;
        Ok(Self {
            identity,
            layout,
            input,
        })
    }
    pub fn identity(&self) -> &RequestIdentity {
        &self.identity
    }
    pub fn layout(&self) -> &PdfLayout {
        &self.layout
    }
    pub fn input(&self) -> &InputDescriptor {
        &self.input
    }
}
pub fn decode_request(bytes: &[u8]) -> Result<RequestEnvelope, CodecError> {
    scan::prepare(bytes, MAX_REQUEST_BYTES, CodecErrorKind::RequestTooLarge)?;
    let fields = scan::object(
        bytes,
        &["protocol", "request_id", "profile", "layout", "input"],
    )?;
    fields.exact(&["protocol", "request_id", "profile", "layout", "input"])?;
    let id = identity(&fields)?;
    let layout = scan::layout(fields.required("layout")?)?;
    if layout.repair_dependency().is_some() {
        return Err(CodecError::new(CodecErrorKind::RepairDependency));
    }
    let input = scan::object(fields.required("input")?, &["name", "byte_len", "sha256"])?;
    input.exact(&["name", "byte_len", "sha256"])?;
    if scan::small_text(input.required("name")?, 9, CodecErrorKind::InvalidFixedName)?
        != "input.pdf"
    {
        return Err(CodecError::new(CodecErrorKind::InvalidFixedName));
    }
    RequestEnvelope::new(
        id,
        layout,
        scan::uint(input.required("byte_len")?)?,
        Sha256Digest::parse_hex(&scan::small_text(
            input.required("sha256")?,
            64,
            CodecErrorKind::InvalidSha256,
        )?)?,
    )
}
#[derive(Serialize)]
struct InputWire {
    name: &'static str,
    byte_len: u64,
    sha256: String,
}
#[derive(Serialize)]
struct RequestWire<'a> {
    protocol: &'static str,
    request_id: String,
    profile: &'static str,
    layout: &'a PdfLayout,
    input: InputWire,
}
pub fn encode_request<W: Write>(
    request: &RequestEnvelope,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    let mut original = Vec::new();
    let encoded = encode(
        &RequestWire {
            protocol: PROTOCOL,
            request_id: request.identity.request_id().to_hex(),
            profile: request.identity.profile().as_str(),
            layout: &request.layout,
            input: InputWire {
                name: "input.pdf",
                byte_len: request.input.byte_len,
                sha256: request.input.sha256.to_hex(),
            },
        },
        &mut original,
        MAX_REQUEST_BYTES,
        CodecErrorKind::RequestTooLarge,
    )?;
    scan::prepare(
        &original,
        MAX_REQUEST_BYTES,
        CodecErrorKind::RequestTooLarge,
    )?;
    output
        .write_all(&original)
        .map_err(|e| CodecError::sourced(CodecErrorKind::WriteIo, e))?;
    Ok(encoded)
}
