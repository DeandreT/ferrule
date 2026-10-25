//! Filesystem-host loading for per-item dynamic named sources.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::{
    DynamicSourceFault, DynamicSourceLoader, ExecutionPolicy, RestrictedDynamicSourceError,
    RestrictedPolicy,
};
use ir::Instance;

#[cfg(test)]
use crate::FilesystemRunPolicy;
use crate::dynamic_source_fault;
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

    /// Records one request and resolves its location. A relative path
    /// resolves against the project directory.
    fn request(
        &self,
        source_name: &str,
        path: &str,
    ) -> (Option<&'a mapping::NamedSource>, PathBuf) {
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
        (source, resolved)
    }

    /// Loads one admitted location, reusing an earlier document for the same
    /// source and path. Failures are not cached.
    fn load_admitted(
        &self,
        source_name: &str,
        path: &str,
        source: Option<&mapping::NamedSource>,
        resolved: &Path,
    ) -> Result<Arc<Instance>, DynamicSourceFault> {
        let key = (source_name.to_string(), path.to_string());
        if let Some(instance) = self.cache.borrow().get(&key).cloned() {
            return Ok(instance);
        }
        let source = source.ok_or_else(|| dynamic_source_fault::undeclared(source_name))?;
        let instance = Arc::new(
            read_instance_with_pdf_reader(
                resolved,
                &source.schema,
                &source.options,
                self.reader,
                self.policy,
            )
            .map_err(dynamic_source_fault::read_fault)?,
        );
        self.cache.borrow_mut().insert(key, Arc::clone(&instance));
        Ok(instance)
    }
}

impl DynamicSourceLoader for ProjectDynamicSourceLoader<'_> {
    fn load(&self, source_name: &str, path: &str) -> Result<Arc<Instance>, String> {
        self.load_tagged(source_name, path)
            .map_err(DynamicSourceFault::into_message)
    }

    fn load_restricted(
        &self,
        source_name: &str,
        path: &str,
        policy: &RestrictedPolicy,
    ) -> Result<Arc<Instance>, engine::DynamicSourceLoadError> {
        use engine::DynamicSourceLoadError::{Failed, Refused};
        self.load_restricted_tagged(source_name, path, policy)
            .map_err(|error| match error {
                RestrictedDynamicSourceError::Refused(refusal) => Refused(refusal),
                RestrictedDynamicSourceError::Fault(fault) => Failed(fault.into_message()),
            })
    }

    fn load_tagged(
        &self,
        source_name: &str,
        path: &str,
    ) -> Result<Arc<Instance>, DynamicSourceFault> {
        let (source, resolved) = self.request(source_name, path);
        self.load_admitted(source_name, path, source, &resolved)
    }

    /// The policy check runs before the cache so every request is
    /// authorized, not only the first.
    fn load_restricted_tagged(
        &self,
        source_name: &str,
        path: &str,
        policy: &RestrictedPolicy,
    ) -> Result<Arc<Instance>, RestrictedDynamicSourceError> {
        let (source, resolved) = self.request(source_name, path);
        if let Some(source) = source {
            policy.authorize_read(execution_policy::read_location(&resolved, &source.options))?;
        }
        Ok(self.load_admitted(source_name, path, source, &resolved)?)
    }
}
