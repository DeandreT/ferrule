//! An explicit local filesystem host for primary documents.
//!
//! Filenames are relative paths beneath one configured root. `/` and `\` both
//! separate components; empty and `.` components are ignored. URI schemes,
//! rooted or drive paths, parent components and colons are refused, so this
//! host never reaches a network or another volume. Every component beneath the
//! root must be a real directory or regular file, not a symbolic link. These
//! checks are not atomic against concurrent changes to the tree.

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use mapping::{EmptyFileSelection, PrimaryFileSelection};
use thiserror::Error;

use crate::{CsvSourceData, PrimarySourceHost};

/// Default per-document byte limit, matching the bounded CSV reader.
pub const DEFAULT_LOCAL_DOCUMENT_BYTES: u64 = crate::MAX_CSV_INPUT_BYTES as u64;

#[derive(Debug, Error)]
pub enum LocalFileError {
    #[error("local primary file root {path:?} is unavailable: {source}")]
    Root {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "filename uses the {scheme:?} URI scheme; local files never use URI or network sources"
    )]
    Uri { scheme: String },
    #[error("filename {filename:?} is rooted; local files resolve beneath the configured root")]
    Rooted { filename: String },
    #[error("filename {filename:?} has a parent component")]
    Parent { filename: String },
    #[error("filename {filename:?} is not a usable relative path")]
    InvalidName { filename: String },
    #[error("wildcard filename {filename:?} has a wildcard in a directory component")]
    WildcardDirectory { filename: String },
    #[error("wildcard filename {filename:?} matched no files")]
    NoMatches { filename: String },
    #[error("local file {path:?} does not exist")]
    Missing { path: String },
    #[error("local file {path:?} is reached through a symbolic link")]
    Symlink { path: String },
    #[error("local file {path:?} is not a regular file beneath directories")]
    NotRegular { path: String },
    #[error("local file {path:?} exceeds the {limit}-byte document limit")]
    TooLarge { path: String, limit: u64 },
    #[error("local file {path:?}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// Local files beneath one root, with a per-document byte limit.
#[derive(Debug, Clone)]
pub struct LocalPrimaryFiles {
    root: PathBuf,
    max_document_bytes: u64,
}

impl LocalPrimaryFiles {
    pub fn new(root: &Path) -> Result<Self, LocalFileError> {
        let canonical = std::fs::canonicalize(root).map_err(|source| LocalFileError::Root {
            path: root.to_owned(),
            source,
        })?;
        if !canonical.is_dir() {
            return Err(LocalFileError::Root {
                path: root.to_owned(),
                source: io::Error::new(io::ErrorKind::NotADirectory, "not a directory"),
            });
        }
        Ok(Self {
            root: canonical,
            max_document_bytes: DEFAULT_LOCAL_DOCUMENT_BYTES,
        })
    }

    pub fn with_max_document_bytes(mut self, limit: u64) -> Self {
        self.max_document_bytes = limit;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, components: &[&str]) -> PathBuf {
        components
            .iter()
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    /// Check every component beneath the root without following links. The
    /// final component must be a directory when `directory`, else a file.
    fn check(&self, components: &[&str], directory: bool) -> Result<PathBuf, LocalFileError> {
        let request = components.join("/");
        let mut path = self.root.clone();
        for (index, part) in components.iter().enumerate() {
            path.push(part);
            let metadata = std::fs::symlink_metadata(&path).map_err(|source| {
                if source.kind() == io::ErrorKind::NotFound {
                    LocalFileError::Missing {
                        path: request.clone(),
                    }
                } else {
                    LocalFileError::Io {
                        path: request.clone(),
                        source,
                    }
                }
            })?;
            let kind = metadata.file_type();
            if kind.is_symlink() {
                return Err(LocalFileError::Symlink { path: request });
            }
            let expect_directory = directory || index + 1 < components.len();
            if (expect_directory && !kind.is_dir()) || (!expect_directory && !kind.is_file()) {
                return Err(LocalFileError::NotRegular { path: request });
            }
        }
        Ok(path)
    }
}

/// Split a filename into normalized components under the shared lexical rules.
pub(crate) fn components(filename: &str) -> Result<Vec<&str>, LocalFileError> {
    let invalid = || LocalFileError::InvalidName {
        filename: filename.into(),
    };
    if filename.is_empty() || filename.contains('\0') {
        return Err(invalid());
    }
    if let Some((prefix, _)) = filename.split_once(':') {
        let mut characters = prefix.chars();
        let scheme = characters.next().is_some_and(|c| c.is_ascii_alphabetic())
            && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        return Err(match prefix.len() {
            1 if scheme => LocalFileError::Rooted {
                filename: filename.into(),
            },
            2.. if scheme => LocalFileError::Uri {
                scheme: prefix.into(),
            },
            _ => invalid(),
        });
    }
    if filename.starts_with(['/', '\\']) {
        return Err(LocalFileError::Rooted {
            filename: filename.into(),
        });
    }
    let parts: Vec<_> = filename
        .split(['/', '\\'])
        .filter(|part| !matches!(*part, "" | "."))
        .collect();
    if parts.contains(&"..") {
        return Err(LocalFileError::Parent {
            filename: filename.into(),
        });
    }
    if parts.is_empty() {
        return Err(invalid());
    }
    Ok(parts)
}

/// `*` matches any run of characters and `?` exactly one, over Unicode scalars.
pub(crate) fn wildcard_matches(pattern: &str, name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let mut previous = vec![false; name.len() + 1];
    previous[0] = true;
    for token in pattern.chars() {
        let mut current = vec![false; name.len() + 1];
        current[0] = token == '*' && previous[0];
        for index in 1..=name.len() {
            current[index] = match token {
                '*' => previous[index] || current[index - 1],
                '?' => previous[index - 1],
                literal => previous[index - 1] && literal == name[index - 1],
            };
        }
        previous = current;
    }
    previous[name.len()]
}

impl PrimarySourceHost for LocalPrimaryFiles {
    type Error = LocalFileError;

    fn select(
        &self,
        filename: &str,
        selection: PrimaryFileSelection,
    ) -> Result<Vec<String>, LocalFileError> {
        let parts = components(filename)?;
        let PrimaryFileSelection::FileNameWildcard { empty } = selection else {
            return Ok(vec![parts.join("/")]);
        };
        let Some((pattern, directories)) = parts.split_last() else {
            unreachable!("components are nonempty")
        };
        if directories.iter().any(|part| part.contains(['*', '?'])) {
            return Err(LocalFileError::WildcardDirectory {
                filename: filename.into(),
            });
        }
        if !pattern.contains(['*', '?']) {
            return Ok(vec![parts.join("/")]);
        }
        let directory = self.check(directories, true)?;
        let location = directories.join("/");
        let io = |source| LocalFileError::Io {
            path: location.clone(),
            source,
        };
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&directory).map_err(io)? {
            let entry = entry.map_err(io)?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !wildcard_matches(pattern, &name) {
                continue;
            }
            let kind = entry.file_type().map_err(io)?;
            if kind.is_symlink() {
                let mut path = directories.to_vec();
                path.push(&name);
                return Err(LocalFileError::Symlink {
                    path: path.join("/"),
                });
            }
            if kind.is_file() {
                names.push(name);
            }
        }
        if names.is_empty() && empty == EmptyFileSelection::Refuse {
            return Err(LocalFileError::NoMatches {
                filename: filename.into(),
            });
        }
        names.sort();
        Ok(names
            .into_iter()
            .map(|name| {
                if location.is_empty() {
                    name
                } else {
                    format!("{location}/{name}")
                }
            })
            .collect())
    }

    fn load(&self, request: &str) -> Result<CsvSourceData, LocalFileError> {
        let parts = components(request)?;
        let path = self.check(&parts, false)?;
        let request = parts.join("/");
        let io = |source| LocalFileError::Io {
            path: request.clone(),
            source,
        };
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(io)?
            .take(self.max_document_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.max_document_bytes {
            return Err(LocalFileError::TooLarge {
                path: request,
                limit: self.max_document_bytes,
            });
        }
        Ok(CsvSourceData {
            bytes,
            resolved_path: Some(self.resolve(&parts).display().to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_rules_refuse_uris_roots_parents_and_colons() {
        for (filename, expected) in [
            ("https://example.test/a.csv", "uri"),
            ("file:a.csv", "uri"),
            ("C:\\data\\a.csv", "rooted"),
            ("/etc/a.csv", "rooted"),
            ("\\\\server\\share\\a.csv", "rooted"),
            ("in/../a.csv", "parent"),
            ("..\\a.csv", "parent"),
            ("", "invalid"),
            ("././/", "invalid"),
            ("a\0.csv", "invalid"),
            ("a.csv:stream", "uri"),
            ("a b:c.csv", "invalid"),
            ("1x:a.csv", "invalid"),
        ] {
            let found = match components(filename) {
                Err(LocalFileError::Uri { .. }) => "uri",
                Err(LocalFileError::Rooted { .. }) => "rooted",
                Err(LocalFileError::Parent { .. }) => "parent",
                Err(LocalFileError::InvalidName { .. }) => "invalid",
                other => panic!("{filename:?}: {other:?}"),
            };
            assert_eq!(found, expected, "{filename:?}");
        }
        assert_eq!(
            components(".\\in//./ a b .csv").unwrap(),
            ["in", " a b .csv"]
        );
    }

    #[test]
    fn wildcards_match_unicode_scalars() {
        assert!(wildcard_matches("*.csv", ".csv"));
        assert!(wildcard_matches("a?c*", "a雪c"));
        assert!(!wildcard_matches("a?c", "a雪雪c"));
        assert!(wildcard_matches("**", ""));
        assert!(!wildcard_matches("*.CSV", "a.csv"));
    }
}
