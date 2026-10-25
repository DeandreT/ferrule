//! Host loader boundary for dynamic named sources and its tagged failures.

use std::sync::Arc;

use ir::Instance;

use crate::{
    DynamicSourceLoadError, EngineError, PolicyRefusal, RestrictedCapability, RestrictedPolicy,
};

mod diagnostic;
mod fault;

pub use diagnostic::{DynamicSourceCategory, DynamicSourceDiagnostic, DynamicSourcePhase};
pub use fault::{
    DynamicSourceCause, DynamicSourceCodecKind, DynamicSourceFault, DynamicSourceLoadFailure,
    DynamicSourceRefusal, DynamicSourceResource, DynamicSourceResourceLimit,
    LegacyDynamicSourceMessage, RestrictedDynamicSourceError,
};

/// Host boundary for typed secondary sources whose path is computed during
/// mapping execution. Implementations should cache by source name and path.
///
/// The engine calls the tagged methods. Their provided defaults adapt the
/// string methods, so a loader that implements only [`Self::load`] (and
/// optionally [`Self::load_restricted`]) behaves exactly as before: its
/// message reaches [`EngineError::DynamicSourceLoad`] unchanged. A loader with
/// typed failures overrides [`Self::load_tagged`] and
/// [`Self::load_restricted_tagged`], and its faults reach
/// [`EngineError::DynamicSourceFailure`].
pub trait DynamicSourceLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String>;

    /// Restricted runs call this instead of [`Self::load`]. A loader must check
    /// the resolved location against `policy` before acquiring it; the default
    /// refuses because the engine cannot see where an unaware loader reads.
    fn load_restricted(
        &self,
        source: &str,
        path: &str,
        policy: &RestrictedPolicy,
    ) -> Result<Arc<Instance>, DynamicSourceLoadError> {
        let _ = (source, path, policy);
        Err(DynamicSourceLoadError::Refused(
            PolicyRefusal::UnsupportedCapability {
                capability: RestrictedCapability::UnconfinedDynamicSourceLoader,
            },
        ))
    }

    /// The tagged form trusted runs call. The default adapts [`Self::load`]:
    /// its message becomes a legacy acquisition fault.
    fn load_tagged(&self, source: &str, path: &str) -> Result<Arc<Instance>, DynamicSourceFault> {
        self.load(source, path).map_err(DynamicSourceFault::legacy)
    }

    /// The tagged form restricted runs call. The default adapts
    /// [`Self::load_restricted`], so an unaware loader is still refused before
    /// it is called.
    fn load_restricted_tagged(
        &self,
        source: &str,
        path: &str,
        policy: &RestrictedPolicy,
    ) -> Result<Arc<Instance>, RestrictedDynamicSourceError> {
        self.load_restricted(source, path, policy)
            .map_err(|error| match error {
                DynamicSourceLoadError::Refused(refusal) => {
                    RestrictedDynamicSourceError::Refused(refusal)
                }
                DynamicSourceLoadError::Failed(message) => {
                    RestrictedDynamicSourceError::Fault(DynamicSourceFault::legacy(message))
                }
            })
    }
}

/// Requests one dynamic document and attaches its endpoint and logical path
/// to any failure.
pub(crate) fn load_document(
    loader: &dyn DynamicSourceLoader,
    restricted: Option<&RestrictedPolicy>,
    source_name: &str,
    path: String,
) -> Result<Arc<Instance>, EngineError> {
    let loaded = match restricted {
        None => loader
            .load_tagged(source_name, &path)
            .map_err(RestrictedDynamicSourceError::Fault),
        Some(policy) => loader.load_restricted_tagged(source_name, &path, policy),
    };
    loaded.map_err(|error| match error {
        RestrictedDynamicSourceError::Refused(refusal) => {
            EngineError::DynamicSourceRefused(Box::new(DynamicSourceRefusal {
                source_name: source_name.to_owned(),
                path,
                refusal,
            }))
        }
        RestrictedDynamicSourceError::Fault(fault) => {
            fault.into_engine_error(source_name.to_owned(), path)
        }
    })
}
