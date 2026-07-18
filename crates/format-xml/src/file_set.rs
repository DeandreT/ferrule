use std::io::Read;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::{Component, Path, PathBuf};

use ir::{DocumentMember, Instance, SchemaNode};
use thiserror::Error;

use crate::{XmlFormatError, from_str};

/// Host limits for one local XML wildcard expansion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalFileSetLimits {
    max_files: NonZeroUsize,
    max_total_bytes: NonZeroU64,
}

impl LocalFileSetLimits {
    pub const fn new(max_files: NonZeroUsize, max_total_bytes: NonZeroU64) -> Self {
        Self {
            max_files,
            max_total_bytes,
        }
    }

    pub const fn max_files(self) -> usize {
        self.max_files.get()
    }

    pub const fn max_total_bytes(self) -> u64 {
        self.max_total_bytes.get()
    }
}

impl Default for LocalFileSetLimits {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(1_024).unwrap_or(NonZeroUsize::MIN),
            NonZeroU64::new(64 * 1024 * 1024).unwrap_or(NonZeroU64::MIN),
        )
    }
}

/// A deterministically ordered local XML source set.
#[derive(Debug)]
pub struct LocalXmlFileSet {
    pub instance: Instance,
    pub paths: Vec<PathBuf>,
    pub total_bytes: u64,
}

#[derive(Debug, Error)]
pub enum LocalFileSetError {
    #[error("invalid local XML file-set pattern `{pattern}`: {reason}")]
    InvalidPattern {
        pattern: String,
        reason: &'static str,
    },
    #[error("resolving local XML file-set base `{path}` failed: {source}")]
    BaseIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("reading local XML file-set directory `{path}` failed: {source}")]
    DirectoryIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("local XML file-set directory `{path}` escapes base `{base}`")]
    EscapedBase { path: PathBuf, base: PathBuf },
    #[error("local XML file-set pattern `{0}` matched no files")]
    NoMatches(String),
    #[error("local XML file set exceeds its {limit}-file limit")]
    TooManyFiles { limit: usize },
    #[error("local XML file set exceeds its {limit}-byte aggregate limit")]
    TooManyBytes { limit: u64 },
    #[error("reading local XML file-set member `{path}` failed: {source}")]
    MemberIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing local XML file-set member `{path}` failed: {source}")]
    MemberXml {
        path: PathBuf,
        #[source]
        source: XmlFormatError,
    },
}

/// Expands `pattern` beneath `base`, sorts matches by path, and parses every
/// member without ever following a match outside the canonical base.
pub fn read_local_file_set(
    base: &Path,
    pattern: &Path,
    schema: &SchemaNode,
    limits: LocalFileSetLimits,
) -> Result<LocalXmlFileSet, LocalFileSetError> {
    let canonical_base =
        std::fs::canonicalize(base).map_err(|source| LocalFileSetError::BaseIo {
            path: base.to_owned(),
            source,
        })?;
    let (directory, name_pattern) = split_pattern(&canonical_base, pattern)?;
    let canonical_directory =
        std::fs::canonicalize(&directory).map_err(|source| LocalFileSetError::DirectoryIo {
            path: directory.clone(),
            source,
        })?;
    if !canonical_directory.starts_with(&canonical_base) {
        return Err(LocalFileSetError::EscapedBase {
            path: canonical_directory,
            base: canonical_base,
        });
    }

    let entries = std::fs::read_dir(&canonical_directory).map_err(|source| {
        LocalFileSetError::DirectoryIo {
            path: canonical_directory.clone(),
            source,
        }
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| LocalFileSetError::DirectoryIo {
            path: canonical_directory.clone(),
            source,
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !wildcard_matches(&name_pattern, &name) {
            continue;
        }
        let path =
            std::fs::canonicalize(entry.path()).map_err(|source| LocalFileSetError::MemberIo {
                path: entry.path(),
                source,
            })?;
        if !path.starts_with(&canonical_base) {
            return Err(LocalFileSetError::EscapedBase {
                path,
                base: canonical_base,
            });
        }
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(LocalFileSetError::NoMatches(pattern.display().to_string()));
    }
    if paths.len() > limits.max_files() {
        return Err(LocalFileSetError::TooManyFiles {
            limit: limits.max_files(),
        });
    }

    let mut total_bytes = 0_u64;
    let mut documents = Vec::with_capacity(paths.len());
    for path in &paths {
        let remaining = limits.max_total_bytes().saturating_sub(total_bytes);
        let mut file = std::fs::File::open(path).map_err(|source| LocalFileSetError::MemberIo {
            path: path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(remaining.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| LocalFileSetError::MemberIo {
                path: path.clone(),
                source,
            })?;
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if byte_count > remaining {
            return Err(LocalFileSetError::TooManyBytes {
                limit: limits.max_total_bytes(),
            });
        }
        total_bytes += byte_count;
        let text = std::str::from_utf8(&bytes).map_err(|error| LocalFileSetError::MemberIo {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        })?;
        let value = from_str(text, schema).map_err(|source| LocalFileSetError::MemberXml {
            path: path.clone(),
            source,
        })?;
        let relative =
            path.strip_prefix(&canonical_base)
                .map_err(|_| LocalFileSetError::EscapedBase {
                    path: path.clone(),
                    base: canonical_base.clone(),
                })?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        let source_path = path.to_string_lossy().into_owned();
        let member = DocumentMember::new_source(relative, source_path, value).ok_or_else(|| {
            LocalFileSetError::InvalidPattern {
                pattern: pattern.display().to_string(),
                reason: "a matched file has no usable document path",
            }
        })?;
        documents.push(member);
    }

    Ok(LocalXmlFileSet {
        instance: Instance::DocumentSet(documents),
        paths,
        total_bytes,
    })
}

fn split_pattern(base: &Path, pattern: &Path) -> Result<(PathBuf, String), LocalFileSetError> {
    let invalid = |reason| LocalFileSetError::InvalidPattern {
        pattern: pattern.display().to_string(),
        reason,
    };
    if pattern.as_os_str().is_empty() || pattern.is_absolute() {
        return Err(invalid("the pattern must be a non-empty relative path"));
    }
    if pattern.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(invalid("parent and rooted path components are not allowed"));
    }
    let name = pattern
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("the filename pattern must be UTF-8"))?;
    if !name.contains(['*', '?']) {
        return Err(invalid("the filename must contain `*` or `?`"));
    }
    let parent = pattern.parent().unwrap_or_else(|| Path::new(""));
    if parent
        .to_string_lossy()
        .chars()
        .any(|character| matches!(character, '*' | '?'))
    {
        return Err(invalid("wildcards are allowed only in the filename"));
    }
    Ok((base.join(parent), name.to_string()))
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let value = value.chars().collect::<Vec<_>>();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for token in pattern {
        let mut current = vec![false; value.len() + 1];
        if token == '*' {
            current[0] = previous[0];
        }
        for index in 1..=value.len() {
            current[index] = match token {
                '*' => previous[index] || current[index - 1],
                '?' => previous[index - 1],
                literal => previous[index - 1] && literal == value[index - 1],
            };
        }
        previous = current;
    }
    previous[value.len()]
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::num::{NonZeroU64, NonZeroUsize};
    use std::time::{SystemTime, UNIX_EPOCH};

    use ir::{ScalarType, SchemaNode};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> std::io::Result<Self> {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ferrule-local-xml-file-set-{}-{suffix}",
                std::process::id()
            ));
            std::fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "root",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        )
    }

    #[test]
    fn reads_matches_in_stable_path_order() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        std::fs::write(
            directory.0.join("item-b.xml"),
            "<root><value>b</value></root>",
        )?;
        std::fs::write(
            directory.0.join("item-a.xml"),
            "<root><value>a</value></root>",
        )?;
        std::fs::write(
            directory.0.join("other.xml"),
            "<root><value>x</value></root>",
        )?;

        let loaded = read_local_file_set(
            &directory.0,
            Path::new("item-*.xml"),
            &schema(),
            LocalFileSetLimits::default(),
        )?;

        assert_eq!(
            loaded
                .paths
                .iter()
                .filter_map(|path| path.file_name()?.to_str())
                .collect::<Vec<_>>(),
            ["item-a.xml", "item-b.xml"]
        );
        let Instance::DocumentSet(documents) = loaded.instance else {
            return Err("file-set result was not a document set".into());
        };
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].path(), "item-a.xml");
        assert_eq!(
            documents[0].source_path(),
            directory.0.join("item-a.xml").to_string_lossy()
        );
        assert_eq!(
            documents[0]
                .value()
                .field("value")
                .and_then(Instance::as_scalar),
            Some(&ir::Value::String("a".into()))
        );
        Ok(())
    }

    #[test]
    fn rejects_escape_and_enforces_file_and_byte_limits() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        std::fs::write(
            directory.0.join("item-a.xml"),
            "<root><value>a</value></root>",
        )?;
        std::fs::write(
            directory.0.join("item-b.xml"),
            "<root><value>b</value></root>",
        )?;

        assert!(matches!(
            read_local_file_set(
                &directory.0,
                Path::new("../item-*.xml"),
                &schema(),
                LocalFileSetLimits::default(),
            ),
            Err(LocalFileSetError::InvalidPattern { .. })
        ));
        let one_file = LocalFileSetLimits::new(
            NonZeroUsize::new(1).ok_or("invalid test file limit")?,
            NonZeroU64::new(1_024).ok_or("invalid test byte limit")?,
        );
        assert!(matches!(
            read_local_file_set(&directory.0, Path::new("item-*.xml"), &schema(), one_file),
            Err(LocalFileSetError::TooManyFiles { limit: 1 })
        ));
        let one_byte = LocalFileSetLimits::new(
            NonZeroUsize::new(2).ok_or("invalid test file limit")?,
            NonZeroU64::new(1).ok_or("invalid test byte limit")?,
        );
        assert!(matches!(
            read_local_file_set(&directory.0, Path::new("item-*.xml"), &schema(), one_byte),
            Err(LocalFileSetError::TooManyBytes { limit: 1 })
        ));
        Ok(())
    }
}
