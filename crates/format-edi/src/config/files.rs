//! Bounded file loading and portable sibling configuration resolution.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use super::ConfigError;

/// Immutable raw bytes for an exact host-authorized resource identity. The
/// implementation owns confinement and digest checks; supplied imports never
/// reopen the returned identity or scan for a replacement configuration.
pub trait EdiConfigByteReader {
    fn read(&self, resource: &Path) -> std::io::Result<Arc<[u8]>>;
}

const MAX_FILES: usize = 32;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_MESSAGE_SCAN_FILES: usize = 512;
const MAX_MESSAGE_SCAN_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct Files {
    root: PathBuf,
    paths: BTreeSet<PathBuf>,
    total_bytes: usize,
    supplied: Option<Rc<dyn EdiConfigByteReader>>,
    cache: BTreeMap<PathBuf, Arc<[u8]>>,
}

impl Files {
    pub(super) fn new(root: &Path) -> Result<Self, ConfigError> {
        let root = std::fs::canonicalize(root).map_err(|source| ConfigError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        if !root.is_dir() {
            return Err(ConfigError::Invalid(format!(
                "configuration authorization root `{}` is not a directory",
                root.display()
            )));
        }
        Ok(Self {
            root,
            paths: BTreeSet::new(),
            total_bytes: 0,
            supplied: None,
            cache: BTreeMap::new(),
        })
    }

    pub(super) fn supplied(
        root: &Path,
        reader: Rc<dyn EdiConfigByteReader>,
    ) -> Result<Self, ConfigError> {
        if !root.is_absolute()
            || root.as_os_str().len() > 4096
            || root.as_os_str().as_encoded_bytes().contains(&0)
            || root.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err(ConfigError::Invalid(
                "supplied EDI root must be an approved absolute identity".into(),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            paths: BTreeSet::new(),
            total_bytes: 0,
            supplied: Some(reader),
            cache: BTreeMap::new(),
        })
    }

    pub(super) fn confined_file(&self, path: &Path) -> Result<PathBuf, ConfigError> {
        if self.supplied.is_some() {
            let path = lexical_path(path);
            if path.as_os_str().len() > 4096
                || path.as_os_str().as_encoded_bytes().contains(&0)
                || !path.starts_with(&self.root)
            {
                return Err(ConfigError::Invalid(
                    "supplied EDI resource escapes its authorized root".into(),
                ));
            }
            return Ok(path);
        }
        confined_file(&self.root, path)
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub(super) fn read(&mut self, path: &Path) -> Result<String, ConfigError> {
        let canonical = self.confined_file(path)?;
        if let Some(reader) = &self.supplied {
            let bytes = match self.cache.get(&canonical) {
                Some(bytes) => bytes.clone(),
                None => {
                    if self.cache.len() >= MAX_FILES {
                        return Err(ConfigError::Limit("included file count"));
                    }
                    let bytes = reader.read(&canonical).map_err(|source| ConfigError::Io {
                        path: canonical.clone(),
                        source,
                    })?;
                    let total = self
                        .total_bytes
                        .checked_add(bytes.len())
                        .ok_or(ConfigError::Limit("total input size"))?;
                    if total > MAX_TOTAL_BYTES {
                        return Err(ConfigError::Limit("total input size"));
                    }
                    self.total_bytes = total;
                    self.paths.insert(canonical.clone());
                    self.cache.insert(canonical, bytes.clone());
                    bytes
                }
            };
            let text = String::from_utf8(bytes.to_vec()).map_err(|source| {
                ConfigError::Invalid(format!("supplied EDI resource is not UTF-8: {source}"))
            })?;
            {
                let doc = parse_document(path, &text)?;
                let root = doc.root_element();
                if root.tag_name().name() != "Config" || root.tag_name().namespace().is_some() {
                    return Err(ConfigError::Invalid(
                        "supplied EDI resource is not an EDI Config document".into(),
                    ));
                }
            }
            return Ok(text);
        }
        let text = read_bounded_text(&canonical, MAX_TOTAL_BYTES, "total input size")?;
        if self.paths.insert(canonical) {
            if self.paths.len() > MAX_FILES {
                return Err(ConfigError::Limit("included file count"));
            }
            self.total_bytes = self
                .total_bytes
                .checked_add(text.len())
                .ok_or(ConfigError::Limit("total input size"))?;
            if self.total_bytes > MAX_TOTAL_BYTES {
                return Err(ConfigError::Limit("total input size"));
            }
        }
        Ok(text)
    }

    pub(super) fn resolve_sibling(
        &self,
        path: &Path,
        relative: &str,
    ) -> Result<PathBuf, ConfigError> {
        if self.supplied.is_none() {
            return resolve_sibling(path, relative, &self.root);
        }
        if relative.is_empty()
            || relative.len() > 4096
            || relative.contains(['\0', ':'])
            || relative.starts_with(['/', '\\'])
        {
            return Err(ConfigError::Invalid(
                "supplied EDI include is not a bounded relative reference".into(),
            ));
        }
        self.confined_file(
            &path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(relative.replace('\\', "/")),
        )
    }

    pub(super) fn resolve_message(
        &self,
        envelope: &Path,
        message: &str,
    ) -> Result<PathBuf, ConfigError> {
        if self.supplied.is_none() {
            return resolve_message_config(envelope, message, &self.root);
        }
        if message.is_empty()
            || message.len() > 128
            || !message
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(ConfigError::Invalid(
                "supplied EDI selected-message identity is invalid".into(),
            ));
        }
        // This is the format's exact selected-message convention, not a scan.
        // Failure must not discover a different file by its MessageType content.
        self.resolve_sibling(envelope, &format!("{message}.Config"))
    }
}

fn lexical_path(path: &Path) -> PathBuf {
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

fn confined_file(root: &Path, path: &Path) -> Result<PathBuf, ConfigError> {
    let canonical = std::fs::canonicalize(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(root) {
        return Err(ConfigError::Invalid(format!(
            "configuration `{}` resolves outside authorization root `{}`",
            path.display(),
            root.display()
        )));
    }
    if !canonical.is_file() {
        return Err(ConfigError::Invalid(format!(
            "configuration `{}` does not resolve to a file",
            path.display()
        )));
    }
    Ok(canonical)
}

fn read_bounded_text(
    path: &Path,
    max_bytes: usize,
    limit: &'static str,
) -> Result<String, ConfigError> {
    let file = std::fs::File::open(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut text = String::new();
    file.take((max_bytes + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if text.len() > max_bytes {
        return Err(ConfigError::Limit(limit));
    }
    Ok(text)
}

pub(super) fn parse_document<'a>(
    path: &Path,
    text: &'a str,
) -> Result<roxmltree::Document<'a>, ConfigError> {
    roxmltree::Document::parse(text).map_err(|source| ConfigError::Xml {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn resolve_sibling(
    path: &Path,
    relative: &str,
    root: &Path,
) -> Result<PathBuf, ConfigError> {
    if relative.is_empty() || relative.contains('\0') {
        return Err(ConfigError::Invalid(
            "include path is empty or contains NUL".to_string(),
        ));
    }
    let portable = relative.replace('\\', "/");
    let relative = Path::new(&portable);
    if relative.is_absolute() || looks_like_windows_absolute(&portable) {
        return Err(ConfigError::Invalid(format!(
            "include path `{portable}` is not a bounded relative path"
        )));
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolved = resolve_case_insensitive(base, relative).ok_or_else(|| {
        ConfigError::Invalid(format!(
            "configuration `{portable}` was not found beside `{}`",
            path.display()
        ))
    })?;
    confined_file(root, &resolved)
}

fn resolve_case_insensitive(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let expected = match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                current.pop();
                continue;
            }
            Component::Normal(expected) => expected,
            Component::Prefix(_) | Component::RootDir => return None,
        };
        let direct = current.join(expected);
        if direct.exists() {
            current = direct;
            continue;
        }
        let expected = expected.to_str()?;
        let mut matches = std::fs::read_dir(&current)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(expected))
            })
            .map(|entry| entry.path());
        let found = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        current = found;
    }
    current.is_file().then_some(current)
}

fn looks_like_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with("//")
}

pub(super) fn resolve_message_config(
    envelope_path: &Path,
    message_type: &str,
    root: &Path,
) -> Result<PathBuf, ConfigError> {
    let direct = format!("{message_type}.Config");
    if let Ok(path) = resolve_sibling(envelope_path, &direct, root) {
        return Ok(path);
    }
    let directory = envelope_path.parent().unwrap_or_else(|| Path::new("."));
    let entries = std::fs::read_dir(directory).map_err(|source| ConfigError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut files = 0usize;
    let mut bytes = 0usize;
    let mut found = None;
    for entry in entries {
        let entry = entry.map_err(|source| ConfigError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("Config") {
            continue;
        }
        let path = confined_file(root, &path)?;
        files += 1;
        if files > MAX_MESSAGE_SCAN_FILES {
            return Err(ConfigError::Limit("message configuration scan file count"));
        }
        let text = read_bounded_text(
            &path,
            MAX_MESSAGE_SCAN_BYTES,
            "message configuration scan size",
        )?;
        bytes = bytes
            .checked_add(text.len())
            .ok_or(ConfigError::Limit("message configuration scan size"))?;
        if bytes > MAX_MESSAGE_SCAN_BYTES {
            return Err(ConfigError::Limit("message configuration scan size"));
        }
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let matches = doc
            .descendants()
            .any(|node| node.has_tag_name("MessageType") && node.text() == Some(message_type));
        if matches {
            if found.is_some() {
                return Err(ConfigError::Invalid(format!(
                    "message type `{message_type}` has multiple configuration files"
                )));
            }
            found = Some(path);
        }
    }
    found.ok_or_else(|| {
        ConfigError::Invalid(format!(
            "message type `{message_type}` has no sibling configuration"
        ))
    })
}
