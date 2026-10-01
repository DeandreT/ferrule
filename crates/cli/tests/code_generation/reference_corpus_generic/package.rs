use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 256 * 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_ENTRIES: usize = 8192;
const MAX_DEPTH: usize = 64;

#[derive(Debug)]
pub(crate) enum PackageError {
    Io(std::io::Error),
    UnsupportedFile,
    Depth,
    Files,
    Bytes,
    Sidecar,
    Changed,
    Xml,
    ConnectionEscape,
    CaseIdentity,
}
impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "generic corpus package rejected: {self:?}")
    }
}
impl std::error::Error for PackageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

struct Original {
    path: PathBuf,
    bytes: Vec<u8>,
    modified: SystemTime,
}

pub(crate) struct Snapshot {
    pub root: PathBuf,
    original_root: PathBuf,
    originals: Vec<Original>,
    entries: usize,
}
impl Snapshot {
    pub fn copy(source: &Path, target: &Path) -> Result<Self, PackageError> {
        if target.exists() {
            return Err(PackageError::UnsupportedFile);
        }
        let source = source.canonicalize()?;
        if target
            .parent()
            .ok_or(PackageError::UnsupportedFile)?
            .canonicalize()?
            .starts_with(&source)
        {
            return Err(PackageError::UnsupportedFile);
        }
        std::fs::create_dir(target)?;
        let mut snapshot = Self {
            root: target.canonicalize()?,
            original_root: source.clone(),
            originals: Vec::new(),
            entries: 0,
        };
        let mut total = 0;
        snapshot.copy_directory(&source, &source, 0, &mut total)?;
        snapshot.verify()?;
        Ok(snapshot)
    }

    fn copy_directory(
        &mut self,
        source_root: &Path,
        directory: &Path,
        depth: usize,
        total: &mut usize,
    ) -> Result<(), PackageError> {
        if depth > MAX_DEPTH {
            return Err(PackageError::Depth);
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            self.entries = self.entries.checked_add(1).ok_or(PackageError::Files)?;
            if self.entries > MAX_ENTRIES {
                return Err(PackageError::Files);
            }
            entries.push(entry?);
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(PackageError::UnsupportedFile);
            }
            let relative = path
                .strip_prefix(source_root)
                .map_err(|_| PackageError::UnsupportedFile)?;
            let destination = self.root.join(relative);
            if metadata.is_dir() {
                std::fs::create_dir(&destination)?;
                self.copy_directory(source_root, &path, depth + 1, total)?;
            } else if metadata.is_file() {
                if self.originals.len() >= MAX_FILES {
                    return Err(PackageError::Files);
                }
                clean_sidecars(&path)?;
                let bytes = read_bounded(&path)?;
                *total = total.checked_add(bytes.len()).ok_or(PackageError::Bytes)?;
                if *total > MAX_PACKAGE_BYTES {
                    return Err(PackageError::Bytes);
                }
                std::fs::write(destination, &bytes)?;
                let original = Original {
                    path,
                    bytes,
                    modified: metadata.modified()?,
                };
                verify_original(&original)?;
                self.originals.push(original);
            } else {
                return Err(PackageError::UnsupportedFile);
            }
        }
        Ok(())
    }

    pub fn resolve_cases(&self, reviewed: &[&str]) -> Result<Vec<String>, PackageError> {
        let names = self
            .originals
            .iter()
            .filter(|original| {
                original.path.extension().and_then(|value| value.to_str()) == Some("mfd")
            })
            .map(|original| {
                original
                    .path
                    .strip_prefix(&self.original_root)
                    .ok()
                    .and_then(|path| {
                        path.components()
                            .map(|part| part.as_os_str().to_str())
                            .collect::<Option<Vec<_>>>()
                    })
                    .map(|parts| parts.join("/"))
                    .ok_or(PackageError::CaseIdentity)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut resolved = Vec::with_capacity(reviewed.len());
        for identity in reviewed {
            let matches = names
                .iter()
                .filter(|name| {
                    identity.strip_prefix("sha256:").map_or_else(
                        || name.as_str() == *identity,
                        |digest| format!("{:x}", Sha256::digest(name.as_bytes())) == digest,
                    )
                })
                .collect::<Vec<_>>();
            let [name] = matches.as_slice() else {
                return Err(PackageError::CaseIdentity);
            };
            let name = name.as_str();
            if resolved.iter().any(|existing| existing == name) {
                return Err(PackageError::CaseIdentity);
            }
            resolved.push(name.to_owned());
        }
        Ok(resolved)
    }

    pub fn preflight_connections(&self, cases: &[&str]) -> Result<(), PackageError> {
        for case in cases {
            preflight_connection_file(&self.root, &self.root.join(case))?;
        }
        // Isolated library XML may also be loaded during import. Inspect all staged local libraries.
        for original in &self.originals {
            if original
                .path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("mfl"))
            {
                let relative = original
                    .path
                    .strip_prefix(&self.original_root)
                    .map_err(|_| PackageError::UnsupportedFile)?;
                preflight_connection_file(&self.root, &self.root.join(relative))?;
            }
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), PackageError> {
        for original in &self.originals {
            verify_original(original)?;
        }
        Ok(())
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, PackageError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PackageError::UnsupportedFile);
    }
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(PackageError::Bytes);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(path)?
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PackageError::Bytes);
    }
    Ok(bytes)
}

fn verify_original(original: &Original) -> Result<(), PackageError> {
    clean_sidecars(&original.path)?;
    let metadata = std::fs::symlink_metadata(&original.path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != original.bytes.len() as u64
        || metadata.modified()? != original.modified
        || read_bounded(&original.path)? != original.bytes
    {
        return Err(PackageError::Changed);
    }
    Ok(())
}

fn clean_sidecars(path: &Path) -> Result<(), PackageError> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["db", "sqlite", "sqlite3"]
                .iter()
                .any(|kind| extension.eq_ignore_ascii_case(kind))
        })
    {
        return Ok(());
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        match std::fs::symlink_metadata(Path::new(&sidecar)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PackageError::Io(error)),
            Ok(_) => return Err(PackageError::Sidecar),
        }
    }
    Ok(())
}

fn attribute_schema() -> ir::SchemaNode {
    ir::SchemaNode::group(
        ir::XML_ATTRIBUTES_FIELD,
        vec![
            ir::SchemaNode::scalar(ir::XML_LOCAL_NAME_FIELD, ir::ScalarType::String),
            ir::SchemaNode::scalar(ir::XML_TEXT_FIELD, ir::ScalarType::String).text(),
        ],
    )
    .repeating()
}

fn connection_schema() -> ir::SchemaNode {
    let generic = ir::SchemaNode::group(
        ir::XML_ELEMENTS_FIELD,
        vec![
            ir::SchemaNode::scalar(ir::XML_LOCAL_NAME_FIELD, ir::ScalarType::String),
            attribute_schema(),
            ir::SchemaNode::recursive_group(ir::XML_ELEMENTS_FIELD, ir::XML_ELEMENTS_FIELD)
                .repeating(),
        ],
    )
    .repeating();
    ir::SchemaNode::group("mapping", vec![attribute_schema(), generic])
}

pub(super) fn preflight_connection_file(root: &Path, mapping: &Path) -> Result<(), PackageError> {
    let bytes = read_bounded(mapping)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| PackageError::Xml)?;
    let mut schema = connection_schema();
    let value = match format_xml::from_str(text, &schema) {
        Ok(value) => value,
        Err(format_xml::XmlFormatError::UnexpectedRoot { found, .. })
            if !found.starts_with('{') =>
        {
            schema.name = found;
            format_xml::from_str(text, &schema).map_err(|_| PackageError::Xml)?
        }
        Err(_) => return Err(PackageError::Xml),
    };
    check_connections(
        root,
        mapping.parent().ok_or(PackageError::UnsupportedFile)?,
        &value,
    )
}

fn check_connections(root: &Path, base: &Path, value: &ir::Instance) -> Result<(), PackageError> {
    match value {
        ir::Instance::Group(fields) => {
            if let Some(ir::Instance::Repeated(attributes)) = value.field(ir::XML_ATTRIBUTES_FIELD)
            {
                for attribute in attributes {
                    if matches!(attribute.field(ir::XML_LOCAL_NAME_FIELD).and_then(ir::Instance::as_scalar), Some(ir::Value::String(name)) if name == "ConnectionString")
                    {
                        let Some(ir::Value::String(connection)) = attribute
                            .field(ir::XML_TEXT_FIELD)
                            .and_then(ir::Instance::as_scalar)
                        else {
                            return Err(PackageError::Xml);
                        };
                        confined_connection(root, base, connection)?;
                    }
                }
            }
            for (_, value) in fields {
                check_connections(root, base, value)?;
            }
        }
        ir::Instance::Repeated(items) | ir::Instance::MappedSequence(items) => {
            for item in items {
                check_connections(root, base, item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn confined_connection(root: &Path, base: &Path, connection: &str) -> Result<(), PackageError> {
    if connection.trim().is_empty() {
        return Err(PackageError::ConnectionEscape);
    }
    // Query import uses a direct platform path join, unlike the normal confined resource resolver.
    // Check that exact path and the portable resource spelling before any SQLite introspection.
    for spelling in [connection.to_owned(), connection.replace('\\', "/")] {
        let path = Path::new(&spelling);
        if path.is_absolute() {
            return Err(PackageError::ConnectionEscape);
        }
        let mut candidate = base.to_path_buf();
        for component in path.components() {
            match component {
                std::path::Component::Normal(value) => candidate.push(value),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if candidate == root || !candidate.pop() {
                        return Err(PackageError::ConnectionEscape);
                    }
                }
                _ => return Err(PackageError::ConnectionEscape),
            }
        }
        if !candidate.starts_with(root) {
            return Err(PackageError::ConnectionEscape);
        }
        match std::fs::canonicalize(&candidate) {
            Ok(path) if !path.starts_with(root) => return Err(PackageError::ConnectionEscape),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PackageError::Io(error)),
        }
    }
    Ok(())
}
