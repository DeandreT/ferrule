use std::io::Write;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::error::{boolean, bounded_pdf, decode_diagnostics, decode_host, decode_pdf};
use crate::{
    CodecError, CodecErrorKind, Diagnostics, EncodedBody, HostError, InstanceDescriptor,
    MAX_CONTROL_FRAME_BYTES, MAX_RESULT_BYTES, PROTOCOL, PdfErrorRecord, RequestId,
    RequestIdentity, Sha256Digest, encode, identity, scan,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok(InstanceDescriptor),
    PdfError {
        error: PdfErrorRecord,
        diagnostics: Diagnostics,
    },
    HostError {
        error: HostError,
        diagnostics: Diagnostics,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultEnvelope {
    identity: RequestIdentity,
    outcome: Outcome,
}
impl ResultEnvelope {
    pub fn new(identity: RequestIdentity, outcome: Outcome) -> Result<Self, CodecError> {
        match &outcome {
            Outcome::PdfError { error, .. } => error.validate()?,
            Outcome::HostError {
                error:
                    HostError::ErrorEnvelopeTooLarge {
                        attempted_byte_len, ..
                    },
                ..
            } if *attempted_byte_len <= MAX_RESULT_BYTES as u64 => {
                return Err(CodecError::new(CodecErrorKind::InvalidLength));
            }
            _ => {}
        }
        Ok(Self { identity, outcome })
    }
    pub fn identity(&self) -> &RequestIdentity {
        &self.identity
    }
    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }
    /// Required association for a future owner; not proof of an actual exit/reap.
    pub fn required_exit_code(&self) -> i32 {
        if matches!(self.outcome, Outcome::Ok(_)) {
            0
        } else {
            2
        }
    }
}
pub fn decode_result(
    bytes: &[u8],
    expected: &RequestIdentity,
) -> Result<ResultEnvelope, CodecError> {
    scan::prepare(bytes, MAX_RESULT_BYTES, CodecErrorKind::ResultTooLarge)?;
    let f = scan::object(bytes, &["protocol", "request_id", "profile", "outcome"])?;
    f.exact(&["protocol", "request_id", "profile", "outcome"])?;
    let id = identity(&f)?;
    if id != *expected {
        return Err(CodecError::new(CodecErrorKind::IdentityMismatch));
    }
    let o = scan::object(
        f.required("outcome")?,
        &["kind", "name", "byte_len", "sha256", "error", "diagnostics"],
    )?;
    let outcome =
        match scan::small_text(o.required("kind")?, 32, CodecErrorKind::UnknownField)?.as_str() {
            "ok" => {
                o.exact(&["kind", "name", "byte_len", "sha256"])?;
                if scan::small_text(o.required("name")?, 13, CodecErrorKind::InvalidFixedName)?
                    != "instance.json"
                {
                    return Err(CodecError::new(CodecErrorKind::InvalidFixedName));
                }
                Outcome::Ok(InstanceDescriptor::new(
                    scan::uint(o.required("byte_len")?)?,
                    Sha256Digest::parse_hex(&scan::small_text(
                        o.required("sha256")?,
                        64,
                        CodecErrorKind::InvalidSha256,
                    )?)?,
                )?)
            }
            "pdf_error" => {
                o.exact(&["kind", "error", "diagnostics"])?;
                Outcome::PdfError {
                    error: decode_pdf(o.required("error")?)?,
                    diagnostics: decode_diagnostics(o.required("diagnostics")?)?,
                }
            }
            "host_error" => {
                o.exact(&["kind", "error", "diagnostics"])?;
                Outcome::HostError {
                    error: decode_host(o.required("error")?)?,
                    diagnostics: decode_diagnostics(o.required("diagnostics")?)?,
                }
            }
            _ => return Err(CodecError::new(CodecErrorKind::UnknownField)),
        };
    ResultEnvelope::new(id, outcome)
}
impl Serialize for Outcome {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::Ok(d) => {
                map.serialize_entry("kind", "ok")?;
                map.serialize_entry("name", "instance.json")?;
                map.serialize_entry("byte_len", &d.byte_len())?;
                map.serialize_entry("sha256", &d.sha256().to_hex())?;
            }
            Self::PdfError { error, diagnostics } => {
                map.serialize_entry("kind", "pdf_error")?;
                map.serialize_entry("error", error)?;
                map.serialize_entry("diagnostics", diagnostics)?;
            }
            Self::HostError { error, diagnostics } => {
                map.serialize_entry("kind", "host_error")?;
                map.serialize_entry("error", error)?;
                map.serialize_entry("diagnostics", diagnostics)?;
            }
        }
        map.end()
    }
}
#[derive(Serialize)]
struct ResultWire<'a> {
    protocol: &'static str,
    request_id: String,
    profile: &'static str,
    outcome: &'a Outcome,
}
pub fn encode_result<W: Write>(
    result: &ResultEnvelope,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    if let Outcome::PdfError { error, diagnostics } = &result.outcome {
        return bounded_pdf(
            &result.identity,
            error,
            error.variant(),
            diagnostics,
            output,
        );
    }
    encode(
        &ResultWire {
            protocol: PROTOCOL,
            request_id: result.identity.request_id().to_hex(),
            profile: result.identity.profile().as_str(),
            outcome: &result.outcome,
        },
        output,
        MAX_RESULT_BYTES,
        CodecErrorKind::ResultTooLarge,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EffectiveLimits {
    address_space_bytes: u64,
    cpu_soft_seconds: u64,
    cpu_hard_seconds: u64,
    file_size_bytes: u64,
    core_bytes: u64,
}
impl EffectiveLimits {
    pub fn fixed() -> Self {
        Self {
            address_space_bytes: 536_870_912,
            cpu_soft_seconds: 30,
            cpu_hard_seconds: 31,
            file_size_bytes: 67_108_864,
            core_bytes: 0,
        }
    }
    pub fn values(&self) -> [u64; 5] {
        [
            self.address_space_bytes,
            self.cpu_soft_seconds,
            self.cpu_hard_seconds,
            self.file_size_bytes,
            self.core_bytes,
        ]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyFrame {
    identity: RequestIdentity,
    effective_limits: EffectiveLimits,
}
impl ReadyFrame {
    pub fn new(identity: RequestIdentity) -> Self {
        Self {
            identity,
            effective_limits: EffectiveLimits::fixed(),
        }
    }
    pub fn identity(&self) -> &RequestIdentity {
        &self.identity
    }
    pub fn effective_limits(&self) -> &EffectiveLimits {
        &self.effective_limits
    }
}
#[derive(Serialize)]
struct ReadyWire {
    protocol: &'static str,
    request_id: String,
    profile: &'static str,
    effective_limits: EffectiveLimits,
}
pub fn encode_ready<W: Write>(
    frame: &ReadyFrame,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    encode(
        &ReadyWire {
            protocol: PROTOCOL,
            request_id: frame.identity.request_id().to_hex(),
            profile: frame.identity.profile().as_str(),
            effective_limits: frame.effective_limits,
        },
        output,
        MAX_CONTROL_FRAME_BYTES,
        CodecErrorKind::ControlFrameTooLarge,
    )
}
pub fn decode_ready(bytes: &[u8], expected: &RequestIdentity) -> Result<ReadyFrame, CodecError> {
    scan::prepare(
        bytes,
        MAX_CONTROL_FRAME_BYTES,
        CodecErrorKind::ControlFrameTooLarge,
    )?;
    let f = scan::object(
        bytes,
        &["protocol", "request_id", "profile", "effective_limits"],
    )?;
    f.exact(&["protocol", "request_id", "profile", "effective_limits"])?;
    let id = identity(&f)?;
    if id != *expected {
        return Err(CodecError::new(CodecErrorKind::IdentityMismatch));
    }
    let keys = [
        "address_space_bytes",
        "cpu_soft_seconds",
        "cpu_hard_seconds",
        "file_size_bytes",
        "core_bytes",
    ];
    let limits = scan::object(f.required("effective_limits")?, &keys)?;
    limits.exact(&keys)?;
    for (key, expected) in keys.iter().zip(EffectiveLimits::fixed().values()) {
        if scan::uint(limits.required(key)?)? != expected {
            return Err(CodecError::new(CodecErrorKind::EffectiveLimitsMismatch));
        }
    }
    Ok(ReadyFrame::new(id))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartFrame {
    request_id: RequestId,
}
impl StartFrame {
    pub fn new(request_id: RequestId) -> Self {
        Self { request_id }
    }
    pub fn request_id(&self) -> RequestId {
        self.request_id
    }
}
#[derive(Serialize)]
struct StartWire {
    protocol: &'static str,
    request_id: String,
    start: bool,
}
pub fn encode_start<W: Write>(
    frame: &StartFrame,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    encode(
        &StartWire {
            protocol: PROTOCOL,
            request_id: frame.request_id.to_hex(),
            start: true,
        },
        output,
        MAX_CONTROL_FRAME_BYTES,
        CodecErrorKind::ControlFrameTooLarge,
    )
}
pub fn decode_start(bytes: &[u8], expected: &RequestId) -> Result<StartFrame, CodecError> {
    scan::prepare(
        bytes,
        MAX_CONTROL_FRAME_BYTES,
        CodecErrorKind::ControlFrameTooLarge,
    )?;
    let f = scan::object(bytes, &["protocol", "request_id", "start"])?;
    f.exact(&["protocol", "request_id", "start"])?;
    if scan::small_text(f.required("protocol")?, 64, CodecErrorKind::UnknownVersion)? != PROTOCOL {
        return Err(CodecError::new(CodecErrorKind::UnknownVersion));
    }
    let id = RequestId::parse_hex(&scan::small_text(
        f.required("request_id")?,
        32,
        CodecErrorKind::InvalidRequestId,
    )?)?;
    if id != *expected {
        return Err(CodecError::new(CodecErrorKind::IdentityMismatch));
    }
    if !boolean(f.required("start")?)? {
        return Err(CodecError::new(CodecErrorKind::InvalidStartToken));
    }
    Ok(StartFrame::new(id))
}
