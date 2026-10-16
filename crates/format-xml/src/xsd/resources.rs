use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::XmlFormatError;

/// Supplies immutable raw bytes for an already authorized, absolute resource
/// identity. The host owns filesystem authorization and any digest policy.
/// Supplied-reader imports never reopen these identities as filesystem paths.
pub trait XmlSchemaByteReader {
    fn read(&self, resource: &Path) -> io::Result<Arc<[u8]>>;
}

#[derive(Default)]
struct SuppliedCache {
    files: BTreeMap<PathBuf, Arc<[u8]>>,
    bytes: usize,
    requests: usize,
}

/// One authorizing root for an XSD import, shared by every dependency lookup.
/// A refusal is retained because optional declaration resolution must not turn
/// a resource-boundary failure into a successful schema with default types.
#[derive(Clone, Default)]
pub(super) struct SchemaReader {
    root: Option<PathBuf>,
    refusal: Rc<RefCell<Option<String>>>,
    supplied: Option<Rc<dyn XmlSchemaByteReader>>,
    cache: Rc<RefCell<SuppliedCache>>,
}

impl SchemaReader {
    pub(super) fn supplied(root: &Path, source: Rc<dyn XmlSchemaByteReader>) -> io::Result<Self> {
        if !root.is_absolute()
            || root.as_os_str().len() > 4096
            || root.as_os_str().as_encoded_bytes().contains(&0)
            || root.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "supplied XML root must be an approved absolute identity",
            ));
        }
        Ok(Self {
            root: Some(root.to_path_buf()),
            supplied: Some(source),
            ..Self::default()
        })
    }

    pub(super) fn is_supplied(&self) -> bool {
        self.supplied.is_some()
    }

    pub(super) fn refuse(&self, message: impl Into<String>) -> io::Error {
        let message = message.into();
        self.refusal.borrow_mut().get_or_insert(message.clone());
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    }

    pub(super) fn identity(&self, path: &Path) -> PathBuf {
        if self.is_supplied() {
            lexical_path(path)
        } else {
            super::normalized_path(path)
        }
    }

    pub(super) fn dependency(&self, parent: &Path, reference: &str) -> Option<PathBuf> {
        if !self.is_supplied() {
            return (!reference.contains("://")).then(|| {
                parent
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(reference)
            });
        }
        if reference.is_empty()
            || reference.len() > 4096
            || reference.contains(['\0', ':'])
            || reference.starts_with(['/', '\\'])
        {
            self.refuse("supplied XML dependency is not a bounded relative reference");
            return None;
        }
        let path = lexical_path(
            &parent
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(reference.replace('\\', "/")),
        );
        if !path.starts_with(self.root.as_ref().expect("supplied reader root")) {
            self.refuse("supplied XML dependency escapes its authorized root");
            return None;
        }
        Some(path)
    }

    pub(super) fn confined(root: &Path) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "XML Schema resource root is not a directory",
            ));
        }
        Ok(Self {
            root: Some(root),
            ..Self::default()
        })
    }

    pub(super) fn read(&self, path: &Path) -> std::io::Result<String> {
        if let Some(source) = &self.supplied {
            {
                let mut cache = self.cache.borrow_mut();
                if cache.requests >= 4096 {
                    return Err(self.refuse("supplied XML resource request-count limit exceeded"));
                }
                cache.requests += 1;
            }
            let path = lexical_path(path);
            if path.as_os_str().len() > 4096
                || path.as_os_str().as_encoded_bytes().contains(&0)
                || !path.starts_with(self.root.as_ref().expect("supplied reader root"))
            {
                return Err(self.refuse("supplied XML resource escapes its authorized root"));
            }
            let cached = self.cache.borrow().files.get(&path).cloned();
            let bytes = match cached {
                Some(bytes) => bytes,
                None => {
                    if self.cache.borrow().files.len() >= 64 {
                        return Err(self.refuse("supplied XML resource file-count limit exceeded"));
                    }
                    let bytes = source.read(&path).map_err(|error| {
                        self.refuse(format!("supplied XML resource refused: {error}"))
                    })?;
                    let mut cache = self.cache.borrow_mut();
                    let size = cache
                        .bytes
                        .checked_add(bytes.len())
                        .ok_or_else(|| self.refuse("supplied XML resource byte limit exceeded"))?;
                    if size > super::MAX_SCHEMA_BYTES as usize {
                        return Err(self.refuse("supplied XML resource byte limit exceeded"));
                    }
                    cache.bytes = size;
                    cache.files.insert(path, bytes.clone());
                    bytes
                }
            };
            let text = super::decode_schema_bytes(&bytes, super::MAX_SCHEMA_BYTES)
                .map_err(|error| self.refuse(format!("supplied XML decoding refused: {error}")))?;
            {
                let doc = roxmltree::Document::parse(&text).map_err(|error| {
                    self.refuse(format!("supplied XML parsing refused: {error}"))
                })?;
                let root = doc.root_element();
                if root.tag_name().name() != "schema"
                    || root.tag_name().namespace() != Some("http://www.w3.org/2001/XMLSchema")
                {
                    return Err(self.refuse("supplied XML Schema resource is not an XSD document"));
                }
            }
            return Ok(text);
        }
        let canonical;
        let path = if let Some(root) = &self.root {
            canonical = std::fs::canonicalize(path)?;
            if !canonical.starts_with(root) {
                let message = format!(
                    "XML Schema resource `{}` resolves outside authorizing root `{}`",
                    path.display(),
                    root.display()
                );
                self.refusal.borrow_mut().get_or_insert(message.clone());
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    message,
                ));
            }
            canonical.as_path()
        } else {
            path
        };
        super::read_text(path, super::MAX_SCHEMA_BYTES)
    }

    pub(super) fn finish<T>(&self, result: Result<T, XmlFormatError>) -> Result<T, XmlFormatError> {
        if let Some(message) = self.refusal.borrow().as_ref() {
            return Err(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, message.clone()).into(),
            );
        }
        result
    }
}

pub(super) fn lexical_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}
