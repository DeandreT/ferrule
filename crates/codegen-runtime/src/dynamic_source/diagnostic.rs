//! Stable diagnostic projection for dynamic-source failures.
//!
//! Phase and category tokens are a portable contract shared with the C#
//! runtime's `FerruleDynamicSourceDiagnostic`; add tokens, never rename them.

use std::error::Error;

use super::{DynamicSourceResource, DynamicSourceResourceLimit};
use crate::{RuntimeError, XmlBoundaryErrorKind, XmlExecutionError, XmlInputSetResourceError};

/// Where a dynamic-source request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicSourcePhase {
    /// Before any host callback: loader, path or request-count checks.
    Request,
    /// The host could not acquire the document.
    Acquisition,
    /// The returned document failed its endpoint codec.
    Codec,
    /// A generated boundary resource limit refused the document.
    Resource,
}

impl DynamicSourcePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Acquisition => "acquisition",
            Self::Codec => "codec",
            Self::Resource => "resource",
        }
    }
}

/// The specific failure within its phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicSourceCategory {
    MissingLoader,
    PathType,
    PathTooLong,
    TooManyRequests,
    Host,
    Encoding,
    Document,
    UndeclaredSource,
    DocumentBytes,
    TotalBytes,
    RequestCount,
}

impl DynamicSourceCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingLoader => "missing_loader",
            Self::PathType => "path_type",
            Self::PathTooLong => "path_too_long",
            Self::TooManyRequests => "too_many_requests",
            Self::Host => "host",
            Self::Encoding => "encoding",
            Self::Document => "document",
            Self::UndeclaredSource => "undeclared_source",
            Self::DocumentBytes => "document_bytes",
            Self::TotalBytes => "total_bytes",
            Self::RequestCount => "request_count",
        }
    }
}

/// A small, stable view of one dynamic-source failure. Messages and causes
/// stay on the original error; this projection never parses text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSourceDiagnostic {
    pub phase: DynamicSourcePhase,
    pub endpoint: &'static str,
    pub category: DynamicSourceCategory,
    /// The requested logical path, when the failure reached a request.
    pub path: Option<String>,
    pub resource: Option<DynamicSourceResourceLimit>,
}

impl DynamicSourceDiagnostic {
    fn request(endpoint: &'static str, category: DynamicSourceCategory) -> Self {
        Self {
            phase: DynamicSourcePhase::Request,
            endpoint,
            category,
            path: None,
            resource: None,
        }
    }

    /// Projects the runtime's own dynamic-source failures; other errors are `None`.
    pub fn from_runtime_error(error: &RuntimeError) -> Option<Self> {
        Some(match error {
            RuntimeError::MissingDynamicSourceLoader { source } => {
                Self::request(source, DynamicSourceCategory::MissingLoader)
            }
            RuntimeError::DynamicSourcePath { source, .. } => {
                Self::request(source, DynamicSourceCategory::PathType)
            }
            RuntimeError::DynamicSourcePathTooLong { source, .. } => {
                Self::request(source, DynamicSourceCategory::PathTooLong)
            }
            RuntimeError::DynamicSourceTooMany { source, .. } => {
                Self::request(source, DynamicSourceCategory::TooManyRequests)
            }
            RuntimeError::DynamicSourceLoad { source, path, .. } => Self {
                phase: DynamicSourcePhase::Acquisition,
                endpoint: source,
                category: DynamicSourceCategory::Host,
                path: Some(path.clone()),
                resource: None,
            },
            RuntimeError::DynamicSourceFailure(failure) => Self {
                phase: failure.fault.phase(),
                endpoint: failure.source,
                category: failure.fault.category(),
                path: Some(failure.path.clone()),
                resource: failure.fault.resource_limit(),
            },
            _ => return None,
        })
    }

    /// Projects a generated XML adapter's owned dynamic-input refusal. Unowned
    /// XML failures are `None`; [`Self::find`] continues to their cause.
    pub fn from_xml_execution_error(error: &XmlExecutionError) -> Option<Self> {
        let request = error.request.as_deref()?;
        let boundary = error.boundary.as_ref();
        let counter = boundary
            .source()
            .and_then(|source| source.downcast_ref::<XmlInputSetResourceError>());
        let resource = |resource, observed: Option<u64>, limit: Option<u64>| {
            Some(DynamicSourceResourceLimit {
                resource,
                observed: observed?,
                limit: limit?,
            })
        };
        let (phase, category, resource) = match (boundary.kind, counter) {
            (XmlBoundaryErrorKind::DocumentLimit, _) => (
                DynamicSourcePhase::Resource,
                DynamicSourceCategory::DocumentBytes,
                resource(
                    DynamicSourceResource::DocumentBytes,
                    boundary.bytes.map(|bytes| bytes as u64),
                    boundary.limit.map(|limit| limit as u64),
                ),
            ),
            (_, Some(counter)) => {
                let (category, kind) = if counter.resource == "xml_input_artifact_count" {
                    (
                        DynamicSourceCategory::RequestCount,
                        DynamicSourceResource::RequestCount,
                    )
                } else {
                    (
                        DynamicSourceCategory::TotalBytes,
                        DynamicSourceResource::TotalBytes,
                    )
                };
                (
                    DynamicSourcePhase::Resource,
                    category,
                    resource(kind, Some(counter.observed_count), Some(counter.limit)),
                )
            }
            (XmlBoundaryErrorKind::Utf8, None) => (
                DynamicSourcePhase::Codec,
                DynamicSourceCategory::Encoding,
                None,
            ),
            (_, None) => (
                DynamicSourcePhase::Codec,
                DynamicSourceCategory::Document,
                None,
            ),
        };
        Some(Self {
            phase,
            endpoint: request.source,
            category,
            path: Some(request.path.clone()),
            resource,
        })
    }

    /// Projects the first dynamic-source failure in an error and its source chain,
    /// such as a generated JSON, CSV or XML boundary error.
    pub fn find(error: &(dyn Error + 'static)) -> Option<Self> {
        let mut current = Some(error);
        while let Some(error) = current {
            let projected = if let Some(error) = error.downcast_ref::<RuntimeError>() {
                Self::from_runtime_error(error)
            } else if let Some(error) = error.downcast_ref::<XmlExecutionError>() {
                Self::from_xml_execution_error(error)
            } else {
                None
            };
            if projected.is_some() {
                return projected;
            }
            current = error.source();
        }
        None
    }
}
