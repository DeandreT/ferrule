//! Bounded file loading and portable sibling configuration resolution.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::ConfigError;

const MAX_FILES: usize = 32;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_MESSAGE_SCAN_FILES: usize = 512;
const MAX_MESSAGE_SCAN_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Files {
    paths: BTreeSet<PathBuf>,
    total_bytes: usize,
}

impl Files {
    pub(super) fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub(super) fn read(&mut self, path: &Path) -> Result<String, ConfigError> {
        let canonical = std::fs::canonicalize(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
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

pub(super) fn resolve_sibling(path: &Path, relative: &str) -> Result<PathBuf, ConfigError> {
    let portable = relative.replace('\\', "/");
    let relative = Path::new(&portable);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(ConfigError::Invalid(format!(
            "include path `{portable}` is not a bounded relative path"
        )));
    }
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    resolve_case_insensitive(base, relative).ok_or_else(|| {
        ConfigError::Invalid(format!(
            "configuration `{portable}` was not found beside `{}`",
            path.display()
        ))
    })
}

fn resolve_case_insensitive(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(expected) = component else {
            continue;
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

pub(super) fn resolve_message_config(
    envelope_path: &Path,
    message_type: &str,
) -> Result<PathBuf, ConfigError> {
    let direct = format!("{message_type}.Config");
    if let Ok(path) = resolve_sibling(envelope_path, &direct) {
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
