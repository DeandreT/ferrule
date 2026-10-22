//! Tagged dynamic-source load failures and their request ownership.

use std::error::Error;
use std::fmt;

use super::{DynamicSourceCategory, DynamicSourcePhase};
use crate::{EngineError, PolicyRefusal};

/// Original typed cause retained by an acquisition or codec fault.
pub type DynamicSourceCause = Box<dyn Error + Send + Sync + 'static>;

/// How an acquired document failed its endpoint codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicSourceCodecKind {
    /// The document bytes are not valid text in the endpoint encoding.
    Encoding,
    /// The decoded document does not satisfy the endpoint format or schema.
    Document,
    /// The loader declares no codec for the requested endpoint.
    UndeclaredSource,
}

/// A host resource that refused a dynamic document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicSourceResource {
    /// Original bytes of one returned document.
    DocumentBytes,
    /// Combined original bytes admitted by one execution.
    TotalBytes,
    /// Host callbacks reserved by one execution.
    RequestCount,
}

/// One refused resource with its observed amount and inclusive limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DynamicSourceResourceLimit {
    pub resource: DynamicSourceResource,
    pub observed: u64,
    pub limit: u64,
}

impl fmt::Display for DynamicSourceResourceLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let limit = self.limit;
        match self.resource {
            DynamicSourceResource::DocumentBytes => write!(
                formatter,
                "document exceeds the {limit}-byte dynamic-source limit"
            ),
            DynamicSourceResource::TotalBytes => write!(
                formatter,
                "documents exceed the {limit}-byte combined dynamic-source limit"
            ),
            DynamicSourceResource::RequestCount => write!(
                formatter,
                "requests exceed the {limit}-request dynamic-source limit"
            ),
        }
    }
}

/// A string failure from a legacy loader. Its original allocation is moved,
/// never copied, back into [`EngineError::DynamicSourceLoad`].
#[derive(Debug)]
pub struct LegacyDynamicSourceMessage(String);

impl LegacyDynamicSourceMessage {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LegacyDynamicSourceMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for LegacyDynamicSourceMessage {}

/// Tagged failure a host loader reports for one dynamic document.
///
/// Acquisition means the host could not obtain the document. Codec and
/// resource faults mean it obtained something it could not admit as the
/// endpoint's document. The message of an acquisition or codec fault is its
/// cause's own text.
#[derive(Debug)]
pub enum DynamicSourceFault {
    Acquisition(DynamicSourceCause),
    Codec {
        kind: DynamicSourceCodecKind,
        cause: DynamicSourceCause,
    },
    Resource(DynamicSourceResourceLimit),
}

impl DynamicSourceFault {
    pub fn acquisition(cause: impl Into<DynamicSourceCause>) -> Self {
        Self::Acquisition(cause.into())
    }

    /// Adapts one legacy string failure without copying it.
    pub fn legacy(message: String) -> Self {
        Self::Acquisition(Box::new(LegacyDynamicSourceMessage(message)))
    }

    pub fn codec(kind: DynamicSourceCodecKind, cause: impl Into<DynamicSourceCause>) -> Self {
        Self::Codec {
            kind,
            cause: cause.into(),
        }
    }

    pub fn resource(resource: DynamicSourceResource, observed: u64, limit: u64) -> Self {
        Self::Resource(DynamicSourceResourceLimit {
            resource,
            observed,
            limit,
        })
    }

    pub fn phase(&self) -> DynamicSourcePhase {
        match self {
            Self::Acquisition(_) => DynamicSourcePhase::Acquisition,
            Self::Codec { .. } => DynamicSourcePhase::Codec,
            Self::Resource(_) => DynamicSourcePhase::Resource,
        }
    }

    pub fn category(&self) -> DynamicSourceCategory {
        match self {
            Self::Acquisition(_) => DynamicSourceCategory::Host,
            Self::Codec { kind, .. } => match kind {
                DynamicSourceCodecKind::Encoding => DynamicSourceCategory::Encoding,
                DynamicSourceCodecKind::Document => DynamicSourceCategory::Document,
                DynamicSourceCodecKind::UndeclaredSource => DynamicSourceCategory::UndeclaredSource,
            },
            Self::Resource(limit) => match limit.resource {
                DynamicSourceResource::DocumentBytes => DynamicSourceCategory::DocumentBytes,
                DynamicSourceResource::TotalBytes => DynamicSourceCategory::TotalBytes,
                DynamicSourceResource::RequestCount => DynamicSourceCategory::RequestCount,
            },
        }
    }

    /// The original acquisition or codec cause; resource faults have none.
    pub fn cause(&self) -> Option<&(dyn Error + Send + Sync + 'static)> {
        match self {
            Self::Acquisition(cause) | Self::Codec { cause, .. } => Some(cause.as_ref()),
            Self::Resource(_) => None,
        }
    }

    /// The first `T` in the cause and its source chain, such as the
    /// `std::io::Error` beneath a host's contextual read failure.
    pub fn find_cause<T: Error + 'static>(&self) -> Option<&T> {
        let mut current = self.cause().map(|cause| cause as &(dyn Error + 'static));
        while let Some(error) = current {
            if let Some(found) = error.downcast_ref::<T>() {
                return Some(found);
            }
            current = error.source();
        }
        None
    }

    pub fn resource_limit(&self) -> Option<DynamicSourceResourceLimit> {
        match self {
            Self::Resource(limit) => Some(*limit),
            Self::Acquisition(_) | Self::Codec { .. } => None,
        }
    }

    /// Recovers the original legacy `String`, or returns this fault unchanged.
    pub fn into_legacy_message(self) -> Result<String, Self> {
        match self {
            Self::Acquisition(cause) => match cause.downcast::<LegacyDynamicSourceMessage>() {
                Ok(message) => Ok(message.0),
                Err(cause) => Err(Self::Acquisition(cause)),
            },
            fault => Err(fault),
        }
    }

    /// Renders this fault for a legacy string boundary. A legacy message is
    /// moved unchanged; every other fault uses its display text.
    pub fn into_message(self) -> String {
        self.into_legacy_message()
            .unwrap_or_else(|fault| fault.to_string())
    }

    /// Attaches request ownership. Legacy messages keep their legacy variant.
    pub(crate) fn into_engine_error(self, source_name: String, path: String) -> EngineError {
        match self.into_legacy_message() {
            Ok(message) => EngineError::DynamicSourceLoad {
                source_name,
                path,
                message,
            },
            Err(fault) => EngineError::DynamicSourceFailure(Box::new(DynamicSourceLoadFailure {
                source_name,
                path,
                fault,
            })),
        }
    }
}

impl fmt::Display for DynamicSourceFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Acquisition(cause) | Self::Codec { cause, .. } => cause.fmt(formatter),
            Self::Resource(limit) => limit.fmt(formatter),
        }
    }
}

impl Error for DynamicSourceFault {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause().map(|cause| cause as &(dyn Error + 'static))
    }
}

/// Opaque causes compare by their rendered text; inspect [`DynamicSourceFault::cause`]
/// for the original typed value.
impl PartialEq for DynamicSourceFault {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Acquisition(left), Self::Acquisition(right)) => {
                left.to_string() == right.to_string()
            }
            (
                Self::Codec {
                    kind: left_kind,
                    cause: left,
                },
                Self::Codec {
                    kind: right_kind,
                    cause: right,
                },
            ) => left_kind == right_kind && left.to_string() == right.to_string(),
            (Self::Resource(left), Self::Resource(right)) => left == right,
            _ => false,
        }
    }
}

/// A tagged dynamic-source failure owned by one endpoint and logical path.
///
/// Its display text is the legacy [`EngineError::DynamicSourceLoad`] text.
/// [`EngineError::DynamicSourceFailure`] does not report the fault as its
/// `source()`, so chain renderers print exactly what they printed for a string
/// failure; the fault and its cause remain reachable through this value.
#[derive(Debug, PartialEq)]
pub struct DynamicSourceLoadFailure {
    pub source_name: String,
    pub path: String,
    pub fault: DynamicSourceFault,
}

impl fmt::Display for DynamicSourceLoadFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "loading dynamic source `{}` from `{}` failed: {}",
            self.source_name, self.path, self.fault
        )
    }
}

impl Error for DynamicSourceLoadFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.fault)
    }
}

/// A restricted-policy refusal of one dynamic-source request, raised before
/// acquisition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSourceRefusal {
    pub source_name: String,
    /// The requested logical path, before host resolution.
    pub path: String,
    pub refusal: PolicyRefusal,
}

impl fmt::Display for DynamicSourceRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "dynamic source `{}`: {}",
            self.source_name, self.refusal
        )
    }
}

impl Error for DynamicSourceRefusal {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.refusal)
    }
}

/// Why a tagged policy-aware load did not produce a document.
#[derive(Debug, PartialEq)]
pub enum RestrictedDynamicSourceError {
    /// The policy refused the location before acquisition.
    Refused(PolicyRefusal),
    /// The admitted location failed acquisition, decoding or a resource limit.
    Fault(DynamicSourceFault),
}

impl From<PolicyRefusal> for RestrictedDynamicSourceError {
    fn from(refusal: PolicyRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<DynamicSourceFault> for RestrictedDynamicSourceError {
    fn from(fault: DynamicSourceFault) -> Self {
        Self::Fault(fault)
    }
}
