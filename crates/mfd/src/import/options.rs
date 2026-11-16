use std::path::{Path, PathBuf};

use crate::{MfdError, resource::PackageManifest};

/// Filesystem policy for importing one mapping package.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub(super) package_root: Option<PathBuf>,
    pub(super) package_manifest: Option<PathBuf>,
    pub(super) edi_catalog_roots: Vec<PathBuf>,
    pub(super) json_schema_catalog_roots: Vec<PathBuf>,
    pub(super) item_ordered_exceptions: bool,
    pub(super) resource_bindings_v1: Vec<crate::resource::ResourceBindingV1>,
    pub(super) resource_readset_v1: Option<crate::resource::ResourceReadSetV1>,
}

impl ImportOptions {
    /// Explicit host authorization only; never enabled by discovered manifests.
    /// A matching binding is authoritative, including in best-effort mode.
    pub fn with_resource_bindings_v1<I>(mut self, bindings: I) -> Result<Self, MfdError>
    where
        I: IntoIterator<Item = crate::resource::ResourceBindingV1>,
    {
        for binding in bindings {
            if self.resource_bindings_v1.len() >= crate::resource::MAX_RESOURCE_BINDINGS_V1 {
                return Err(MfdError::Resource("binding count limit exceeded".into()));
            }
            self.resource_bindings_v1.push(binding);
        }
        crate::resource::validate_bindings(&self.resource_bindings_v1)?;
        Ok(self)
    }

    /// Private provenance sink. Records identify actual supplied raw buffers;
    /// a root-only pin does not imply that transitive files were pinned.
    pub fn with_resource_readset_v1(mut self, readset: crate::resource::ResourceReadSetV1) -> Self {
        self.resource_readset_v1 = Some(readset);
        self
    }

    /// Imports a supported native exception branch as a lazy per-item guard.
    ///
    /// The default preserves legacy pre-target failure rules. Unsupported
    /// opted-in branches produce diagnostics rather than falling back to a
    /// global rule; the executable import profile refuses those diagnostics.
    pub fn with_item_ordered_exceptions(mut self) -> Self {
        self.item_ordered_exceptions = true;
        self
    }

    /// Confines all mapping resources to this trusted directory.
    pub fn with_package_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.package_root = Some(root.into());
        self.package_manifest = None;
        self
    }

    pub fn package_root(&self) -> Option<&Path> {
        self.package_root.as_deref()
    }

    /// Applies roots from an explicitly trusted mapping package manifest.
    ///
    /// Directly configured catalog roots keep search precedence. The
    /// manifest's directory becomes the package root.
    pub fn with_package_manifest(mut self, path: impl AsRef<Path>) -> Result<Self, MfdError> {
        let manifest = PackageManifest::load(path.as_ref())?;
        self.package_root = Some(manifest.package_root().to_path_buf());
        self.package_manifest = Some(manifest.path().to_path_buf());
        Ok(self)
    }

    /// Adds an explicitly trusted EDI configuration catalog.
    ///
    /// Catalogs are searched in declaration order after package-contained
    /// resources. Each resolved configuration remains confined to its
    /// canonical catalog directory.
    pub fn with_edi_catalog_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.edi_catalog_roots.push(root.into());
        self
    }

    /// Adds explicitly trusted EDI configuration catalogs in search order.
    pub fn with_edi_catalog_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.edi_catalog_roots
            .extend(roots.into_iter().map(Into::into));
        self
    }

    pub fn edi_catalog_roots(&self) -> &[PathBuf] {
        &self.edi_catalog_roots
    }

    /// Adds an explicitly trusted JSON Schema catalog.
    ///
    /// Catalogs are searched in declaration order after package-contained
    /// resources. Each resolved schema and its local-file references remain
    /// confined to the matched canonical catalog directory.
    pub fn with_json_schema_catalog_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.json_schema_catalog_roots.push(root.into());
        self
    }

    /// Adds explicitly trusted JSON Schema catalogs in search order.
    pub fn with_json_schema_catalog_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.json_schema_catalog_roots
            .extend(roots.into_iter().map(Into::into));
        self
    }

    pub fn json_schema_catalog_roots(&self) -> &[PathBuf] {
        &self.json_schema_catalog_roots
    }
}
