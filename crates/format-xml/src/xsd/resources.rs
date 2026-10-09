use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::XmlFormatError;

/// One authorizing root for an XSD import, shared by every dependency lookup.
/// A refusal is retained because optional declaration resolution must not turn
/// a resource-boundary failure into a successful schema with default types.
#[derive(Clone, Default)]
pub(super) struct SchemaReader {
    root: Option<PathBuf>,
    refusal: Rc<RefCell<Option<String>>>,
}

impl SchemaReader {
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
