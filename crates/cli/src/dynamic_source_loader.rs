//! Filesystem-host loading for per-item dynamic named sources.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::{ExecutionPolicy, RestrictedPolicy};
use ir::Instance;

#[cfg(test)]
use crate::FilesystemRunPolicy;
use crate::execution_policy::{self, TRUSTED};
use crate::{FilesystemPdfReader, TraceSink, http_url, read_instance_with_pdf_reader};

pub(crate) struct ProjectDynamicSourceLoader<'a> {
    pub(crate) project_dir: &'a Path,
    sources: &'a [mapping::NamedSource],
    pub(crate) cache: RefCell<BTreeMap<(String, String), Arc<Instance>>>,
    reader: Option<&'a dyn FilesystemPdfReader>,
    pub(crate) trace_sink: Option<&'a dyn TraceSink>,
    policy: &'a ExecutionPolicy,
}

impl<'a> ProjectDynamicSourceLoader<'a> {
    #[cfg(test)]
    pub(crate) fn new(project_dir: &'a Path, sources: &'a [mapping::NamedSource]) -> Self {
        Self::with_policy(project_dir, sources, None)
    }

    #[cfg(test)]
    pub(crate) fn with_policy(
        project_dir: &'a Path,
        sources: &'a [mapping::NamedSource],
        policy: Option<&'a FilesystemRunPolicy>,
    ) -> Self {
        Self::with_pdf_reader(
            project_dir,
            sources,
            policy.map(|policy| policy as &dyn FilesystemPdfReader),
        )
    }

    pub(crate) fn with_pdf_reader(
        project_dir: &'a Path,
        sources: &'a [mapping::NamedSource],
        reader: Option<&'a dyn FilesystemPdfReader>,
    ) -> Self {
        Self {
            project_dir,
            sources,
            cache: RefCell::new(BTreeMap::new()),
            reader,
            trace_sink: None,
            policy: &TRUSTED,
        }
    }

    pub(crate) fn with_trace_sink(mut self, trace_sink: Option<&'a dyn TraceSink>) -> Self {
        self.trace_sink = trace_sink;
        self
    }

    pub(crate) fn with_execution_policy(mut self, policy: &'a ExecutionPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Loads one admitted location. A restricted check runs before the cache
    /// so every request is authorized, not only the first.
    fn load_checked(
        &self,
        source_name: &str,
        path: &str,
        policy: Option<&RestrictedPolicy>,
    ) -> Result<Arc<Instance>, engine::DynamicSourceLoadError> {
        use engine::DynamicSourceLoadError::{Failed, Refused};
        if let Some(sink) = self.trace_sink {
            sink.dynamic_source_load_requested(source_name, path);
        }
        let source = self
            .sources
            .iter()
            .find(|source| source.name == source_name);
        let resolved = PathBuf::from(path);
        let resolved = if resolved.is_absolute() || http_url(&resolved).is_some() {
            resolved
        } else {
            self.project_dir.join(resolved)
        };
        if let (Some(policy), Some(source)) = (policy, source) {
            policy
                .authorize_read(execution_policy::read_location(&resolved, &source.options))
                .map_err(Refused)?;
        }
        let key = (source_name.to_string(), path.to_string());
        if let Some(instance) = self.cache.borrow().get(&key).cloned() {
            return Ok(instance);
        }
        let source = source
            .ok_or_else(|| Failed(format!("dynamic source `{source_name}` is not declared")))?;
        let instance = Arc::new(
            read_instance_with_pdf_reader(
                &resolved,
                &source.schema,
                &source.options,
                self.reader,
                self.policy,
            )
            .map_err(|error| Failed(error.to_string()))?,
        );
        self.cache.borrow_mut().insert(key, Arc::clone(&instance));
        Ok(instance)
    }
}

impl engine::DynamicSourceLoader for ProjectDynamicSourceLoader<'_> {
    fn load(&self, source_name: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.load_checked(source_name, path, None)
            .map_err(|error| match error {
                engine::DynamicSourceLoadError::Failed(message) => message,
                engine::DynamicSourceLoadError::Refused(refusal) => refusal.to_string(),
            })
    }

    fn load_restricted(
        &self,
        source_name: &str,
        path: &str,
        policy: &RestrictedPolicy,
    ) -> Result<Arc<Instance>, engine::DynamicSourceLoadError> {
        self.load_checked(source_name, path, Some(policy))
    }
}
