//! Immutable, filesystem-independent MFD packages for isolated hosts.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::MfdError;

pub const MAX_PACKAGE_ENTRIES: usize = 128;
pub const MAX_PACKAGE_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PACKAGE_PATH_BYTES: usize = 256;

/// A stable admission/refusal category, independent of platform I/O errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    #[error("invalid package path")]
    InvalidPath,
    #[error("duplicate or aliased package path")]
    DuplicatePath,
    #[error("package count or byte limit exceeded")]
    LimitExceeded,
    #[error("required package entry is missing")]
    MissingEntry,
    #[error("package resource is not valid UTF-8")]
    InvalidText,
    #[error("package resource is invalid or incomplete")]
    InvalidResource,
    #[error("resource type requires filesystem access")]
    FilesystemResource,
}

/// All entries are regular byte buffers: links, directories and implicit host
/// paths are unrepresentable. Paths are case-distinct on every host, but aliases
/// differing only by ASCII case are refused to avoid platform-dependent lookup.
#[derive(Debug, Clone)]
pub struct MfdPackage {
    design_path: String,
    entries: Arc<BTreeMap<String, Arc<[u8]>>>,
}

impl MfdPackage {
    pub fn new<I, P, B>(design_path: &str, entries: I) -> Result<Self, PackageError>
    where
        I: IntoIterator<Item = (P, B)>,
        P: AsRef<str>,
        B: AsRef<[u8]>,
    {
        validate_path(design_path)?;
        let mut files = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        for (path, contents) in entries {
            let path = path.as_ref();
            validate_path(path)?;
            let bytes = contents.as_ref();
            total = total
                .checked_add(bytes.len())
                .ok_or(PackageError::LimitExceeded)?;
            if files.len() >= MAX_PACKAGE_ENTRIES
                || bytes.len() > MAX_PACKAGE_FILE_BYTES
                || total > MAX_PACKAGE_BYTES
            {
                return Err(PackageError::LimitExceeded);
            }
            if !names.insert(path.to_ascii_lowercase()) {
                return Err(PackageError::DuplicatePath);
            }
            files.insert(path.to_owned(), Arc::from(bytes));
        }
        if !files.contains_key(design_path) {
            return Err(PackageError::MissingEntry);
        }
        Ok(Self {
            design_path: design_path.to_owned(),
            entries: Arc::new(files),
        })
    }

    pub fn design_path(&self) -> &str {
        &self.design_path
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = (&str, &[u8])> {
        self.entries
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_ref()))
    }

    pub fn get(&self, path: &str) -> Result<Arc<[u8]>, PackageError> {
        validate_path(path)?;
        self.entries
            .get(path)
            .cloned()
            .ok_or(PackageError::MissingEntry)
    }

    pub(crate) fn resolve(&self, reference: &str) -> Result<PathBuf, PackageError> {
        validate_path(reference)?;
        let base = Path::new(&self.design_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let joined = base.join(reference);
        let path = portable_path(&joined)?;
        self.get(&path)?;
        Ok(joined)
    }

    pub(crate) fn text(&self, path: &Path) -> Result<String, PackageError> {
        let bytes = self.get(path.to_str().ok_or(PackageError::InvalidPath)?)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| PackageError::InvalidText)
    }
}

pub(crate) fn validate_path(path: &str) -> Result<(), PackageError> {
    if path.is_empty()
        || path.len() > MAX_PACKAGE_PATH_BYTES
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | "..") || part.ends_with([' ', '.']))
    {
        return Err(PackageError::InvalidPath);
    }
    Ok(())
}

pub(crate) fn portable_path(path: &Path) -> Result<String, PackageError> {
    let parts = path
        .components()
        .map(|component| match component {
            std::path::Component::Normal(name) => name.to_str().ok_or(PackageError::InvalidPath),
            _ => Err(PackageError::InvalidPath),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let name = parts.join("/");
    validate_path(&name)?;
    Ok(name)
}

/// Virtual absolute identities satisfy the schema readers' authority contract;
/// they are never passed to a filesystem loader.
pub(crate) fn virtual_root() -> &'static Path {
    #[cfg(windows)]
    {
        Path::new("C:/__ferrule_package__")
    }
    #[cfg(not(windows))]
    {
        Path::new("/__ferrule_package__")
    }
}

#[derive(Debug)]
pub(crate) struct PackageReader {
    pub(crate) package: MfdPackage,
    failure: Cell<Option<PackageError>>,
}

impl PackageReader {
    pub(crate) fn new(package: &MfdPackage) -> Self {
        Self {
            package: package.clone(),
            failure: Cell::new(None),
        }
    }

    pub(crate) fn refuse(&self, error: PackageError) -> PackageError {
        if self.failure.get().is_none() {
            self.failure.set(Some(error));
        }
        error
    }

    pub(crate) fn finish(&self) -> Result<(), MfdError> {
        self.failure.get().map_or(Ok(()), |error| Err(error.into()))
    }

    pub(crate) fn bytes(&self, path: &Path) -> Result<Arc<[u8]>, PackageError> {
        let result = path
            .strip_prefix(virtual_root())
            .map_err(|_| PackageError::InvalidPath)
            .and_then(portable_path)
            .and_then(|name| self.package.get(&name));
        result.map_err(|error| self.refuse(error))
    }

    fn read(&self, path: &Path) -> std::io::Result<Arc<[u8]>> {
        self.bytes(path)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::PermissionDenied, error))
    }

    fn validate_reference(&self, reference: &str) -> std::io::Result<()> {
        validate_path(reference).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, self.refuse(error))
        })
    }
}

impl format_xml::xsd::XmlSchemaByteReader for PackageReader {
    fn read(&self, path: &Path) -> std::io::Result<Arc<[u8]>> {
        self.read(path)
    }
    fn validate_reference(&self, reference: &str) -> std::io::Result<()> {
        self.validate_reference(reference)
    }
}
impl format_json::json_schema::JsonSchemaByteReader for PackageReader {
    fn read(&self, path: &Path) -> std::io::Result<Arc<[u8]>> {
        self.read(path)
    }
    fn validate_reference(&self, reference: &str) -> std::io::Result<()> {
        self.validate_reference(reference)
    }
    fn validate_reference_target(&self, _reference: &str, exists: bool) -> std::io::Result<()> {
        if exists {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                self.refuse(PackageError::InvalidResource),
            ))
        }
    }
}
impl format_edi_config::EdiConfigByteReader for PackageReader {
    fn read(&self, path: &Path) -> std::io::Result<Arc<[u8]>> {
        self.read(path)
    }
    fn validate_reference(&self, reference: &str) -> std::io::Result<()> {
        self.validate_reference(reference)
    }
}

/// Export produces the same complete artifact set as filesystem export, without
/// creating a directory or opening a file. No schema sibling may be omitted.
pub struct ExportedPackage {
    pub package: MfdPackage,
    pub report: crate::ExportReport,
}

pub fn export_package(
    project: &mapping::Project,
    design_path: &str,
    profile: crate::ExportProfile,
) -> Result<ExportedPackage, MfdError> {
    validate_path(design_path)?;
    crate::export::prepare_package(project, design_path, profile)
}

pub fn import_package(package: &MfdPackage) -> Result<crate::Imported, MfdError> {
    crate::import::import_package(package)
}
