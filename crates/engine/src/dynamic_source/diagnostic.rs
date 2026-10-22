//! Stable diagnostic projection for dynamic-source failures.
//!
//! Phase and category tokens are the portable contract the generated Rust and
//! C# runtimes also use (`DynamicSourceDiagnostic` in `codegen-runtime`,
//! `FerruleDynamicSourceDiagnostic` in C#); add tokens, never rename them.

use std::error::Error;

use super::DynamicSourceResourceLimit;
use crate::EngineError;

/// Where a dynamic-source request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicSourcePhase {
    /// Before any host callback: loader and path checks.
    Request,
    /// The host could not acquire the document, including a policy refusal.
    Acquisition,
    /// The returned document failed its endpoint codec.
    Codec,
    /// A host resource limit refused the document.
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

/// The specific failure within its phase. The interpreter itself raises only
/// `MissingLoader` and `PathType` before a request; the other request
/// categories belong to generated boundary limits and are listed so that the
/// token set stays identical across runtimes.
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
    pub endpoint: String,
    pub category: DynamicSourceCategory,
    /// The requested logical path, when the failure reached a request.
    pub path: Option<String>,
    pub resource: Option<DynamicSourceResourceLimit>,
}

impl DynamicSourceDiagnostic {
    fn request(endpoint: &str, category: DynamicSourceCategory) -> Self {
        Self {
            phase: DynamicSourcePhase::Request,
            endpoint: endpoint.to_owned(),
            category,
            path: None,
            resource: None,
        }
    }

    fn host(endpoint: &str, path: &str) -> Self {
        Self {
            phase: DynamicSourcePhase::Acquisition,
            endpoint: endpoint.to_owned(),
            category: DynamicSourceCategory::Host,
            path: Some(path.to_owned()),
            resource: None,
        }
    }

    /// Projects the engine's own dynamic-source failures; other errors are `None`.
    ///
    /// A restricted-policy refusal projects as a host acquisition failure: the
    /// generated runtimes have no policy of their own, so the same refusal made
    /// by a confining host loader reaches them as an acquisition fault. The
    /// typed [`crate::PolicyRefusal`] stays on [`EngineError::DynamicSourceRefused`].
    pub fn from_engine_error(error: &EngineError) -> Option<Self> {
        Some(match error {
            EngineError::MissingDynamicSourceLoader { source_name } => {
                Self::request(source_name, DynamicSourceCategory::MissingLoader)
            }
            EngineError::DynamicSourcePath { source_name, .. } => {
                Self::request(source_name, DynamicSourceCategory::PathType)
            }
            EngineError::DynamicSourceLoad {
                source_name, path, ..
            } => Self::host(source_name, path),
            EngineError::DynamicSourceRefused(refused) => {
                Self::host(&refused.source_name, &refused.path)
            }
            EngineError::DynamicSourceFailure(failure) => Self {
                phase: failure.fault.phase(),
                endpoint: failure.source_name.clone(),
                category: failure.fault.category(),
                path: Some(failure.path.clone()),
                resource: failure.fault.resource_limit(),
            },
            _ => return None,
        })
    }

    /// Projects the first engine dynamic-source failure in an error and its
    /// source chain, such as a CLI `anyhow` error or a pipeline stage error.
    pub fn find(error: &(dyn Error + 'static)) -> Option<Self> {
        let mut current = Some(error);
        while let Some(error) = current {
            if let Some(projected) = error
                .downcast_ref::<EngineError>()
                .and_then(Self::from_engine_error)
            {
                return Some(projected);
            }
            current = error.source();
        }
        None
    }
}
