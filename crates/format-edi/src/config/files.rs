//! Bounded file loading and portable sibling configuration resolution.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::ConfigError;

const MAX_FILES: usize = 32;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_MESSAGE_SCAN_FILES: usize = 512;
const MAX_MESSAGE_SCAN_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct Files {
    root: PathBuf,
    paths: BTreeSet<PathBuf>,
    total_bytes: usize,
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
        })
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn confined_file(&self, path: &Path) -> Result<PathBuf, ConfigError> {
        confined_file(&self.root, path)
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub(super) fn read(&mut self, path: &Path) -> Result<String, ConfigError> {
        let canonical = self.confined_file(path)?;
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
