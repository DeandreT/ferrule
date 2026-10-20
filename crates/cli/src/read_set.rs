//! Local files consumed by one run: the design, primary and named inputs,
//! file-set members, and dynamically loaded documents.
//!
//! Mapping outputs and host-owned artifacts, such as a JSON trace, are
//! published after the run by replacing their destination. Both use the same
//! normalized path identity as ordinary output collision checks, so a symlinked
//! ancestor or `..` alias cannot make a consumed file look unrelated. A hard
//! link is a separate directory entry: atomic replacement swaps only that
//! entry and never rewrites the consumed file's contents.

use std::path::{Path, PathBuf};

use anyhow::bail;
use ir::Instance;

use crate::output_documents::{PathConflict, normalized_absolute, protected_conflict};
use crate::{ProjectDynamicSourceLoader, http_url};

pub(crate) struct ReadSet {
    paths: Vec<PathBuf>,
    /// Paths before this index already passed [`Self::reject_protected`].
    checked: usize,
}

impl ReadSet {
    pub(crate) fn new(design: &Path) -> Self {
        Self {
            paths: vec![design.to_path_buf()],
            checked: 0,
        }
    }

    /// Records one local file. HTTP sources have no filesystem identity.
    pub(crate) fn push(&mut self, path: &Path) {
        if http_url(path).is_none() {
            self.paths.push(path.to_path_buf());
        }
    }

    /// Records a loaded source and the resolved location of each member of a
    /// local document set.
    pub(crate) fn push_source(&mut self, path: &Path, instance: &Instance) {
        if http_url(path).is_some() {
            return;
        }
        self.paths.push(path.to_path_buf());
        if let Instance::DocumentSet(documents) = instance {
            self.paths.extend(
                documents
                    .iter()
                    .map(|member| PathBuf::from(member.source_path())),
            );
        }
    }

    /// Records every document the loader read successfully during evaluation.
    pub(crate) fn push_dynamic_loads(&mut self, loader: &ProjectDynamicSourceLoader<'_>) {
        for ((_, requested), instance) in loader.cache.borrow().iter() {
            let requested = Path::new(requested);
            let path = if requested.is_absolute() {
                requested.to_path_buf()
            } else {
                loader.project_dir.join(requested)
            };
            self.push_source(&path, instance);
        }
    }

    pub(crate) fn paths(&self) -> Vec<&Path> {
        self.paths.iter().map(PathBuf::as_path).collect()
    }

    /// Refuses a host-reserved artifact path that names or overlaps a consumed
    /// file. Only paths recorded since the previous call are examined.
    pub(crate) fn reject_protected(&mut self, protected: &[&Path]) -> anyhow::Result<()> {
        let unchecked = &self.paths[self.checked..];
        if protected.is_empty() || unchecked.is_empty() {
            self.checked = self.paths.len();
            return Ok(());
        }
        let reserved = protected
            .iter()
            .map(|path| normalized_absolute(path))
            .collect::<anyhow::Result<Vec<_>>>()?;
        for path in unchecked {
            if let Some((index, conflict)) =
                protected_conflict(&normalized_absolute(path)?, &reserved)
            {
                let relation = match conflict {
                    PathConflict::Same => "would replace",
                    PathConflict::Overlap => "overlaps",
                };
                bail!(
                    "host-reserved path {} {relation} consumed input {}",
                    protected[index].display(),
                    path.display()
                );
            }
        }
        self.checked = self.paths.len();
        Ok(())
    }
}
