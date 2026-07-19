use std::collections::BTreeSet;
use std::fmt;

/// A canonical portable path relative to an emitter's output directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactPath(String);

impl ArtifactPath {
    pub fn new(path: impl Into<String>) -> Result<Self, ArtifactPathError> {
        let path = path.into();
        let kind = invalid_path_kind(&path);
        if let Some(kind) = kind {
            return Err(ArtifactPathError { path, kind });
        }
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for ArtifactPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ArtifactPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactPathErrorKind {
    Empty,
    Absolute,
    ParentComponent,
    NonCanonicalComponent,
    Backslash,
    NulByte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPathError {
    pub path: String,
    pub kind: ArtifactPathErrorKind,
}

impl fmt::Display for ArtifactPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid generated file path {:?}: ", self.path)?;
        formatter.write_str(match self.kind {
            ArtifactPathErrorKind::Empty => "path is empty",
            ArtifactPathErrorKind::Absolute => "path must be relative",
            ArtifactPathErrorKind::ParentComponent => "parent components are not allowed",
            ArtifactPathErrorKind::NonCanonicalComponent => {
                "empty and current-directory components are not allowed"
            }
            ArtifactPathErrorKind::Backslash => "use portable forward-slash separators",
            ArtifactPathErrorKind::NulByte => "NUL bytes are not allowed",
        })
    }
}

impl std::error::Error for ArtifactPathError {}

fn invalid_path_kind(path: &str) -> Option<ArtifactPathErrorKind> {
    if path.is_empty() {
        return Some(ArtifactPathErrorKind::Empty);
    }
    if path.contains('\0') {
        return Some(ArtifactPathErrorKind::NulByte);
    }
    if path.contains('\\') {
        return Some(ArtifactPathErrorKind::Backslash);
    }
    if path.starts_with('/')
        || matches!(path.as_bytes(), [drive, b':', ..] if drive.is_ascii_alphabetic())
    {
        return Some(ArtifactPathErrorKind::Absolute);
    }
    for component in path.split('/') {
        match component {
            ".." => return Some(ArtifactPathErrorKind::ParentComponent),
            "" | "." => return Some(ArtifactPathErrorKind::NonCanonicalComponent),
            _ => {}
        }
    }
    None
}

/// One generated file whose bytes are independent of filesystem encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: ArtifactPath,
    pub contents: Vec<u8>,
}

impl GeneratedFile {
    pub fn new(path: ArtifactPath, contents: impl Into<Vec<u8>>) -> Self {
        Self {
            path,
            contents: contents.into(),
        }
    }
}

/// A duplicate-free set of files in deterministic path order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactSet {
    files: Vec<GeneratedFile>,
}

impl ArtifactSet {
    pub fn new(files: impl IntoIterator<Item = GeneratedFile>) -> Result<Self, ArtifactSetError> {
        let mut files: Vec<_> = files.into_iter().collect();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut paths = BTreeSet::new();
        for file in &files {
            if !paths.insert(file.path.clone()) {
                return Err(ArtifactSetError::DuplicatePath(file.path.clone()));
            }
        }
        Ok(Self { files })
    }

    pub fn files(&self) -> &[GeneratedFile] {
        &self.files
    }

    pub fn into_files(self) -> Vec<GeneratedFile> {
        self.files
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactSetError {
    DuplicatePath(ArtifactPath),
}

impl fmt::Display for ArtifactSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePath(path) => {
                write!(formatter, "duplicate generated file path `{path}`")
            }
        }
    }
}

impl std::error::Error for ArtifactSetError {}
