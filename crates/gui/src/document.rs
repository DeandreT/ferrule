use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentLocation {
    Untitled { suggested_path: PathBuf },
    Saved(PathBuf),
}

impl DocumentLocation {
    pub fn untitled(suggested_path: impl Into<PathBuf>) -> Self {
        Self::Untitled {
            suggested_path: suggested_path.into(),
        }
    }

    pub fn saved(path: impl Into<PathBuf>) -> Self {
        Self::Saved(path.into())
    }

    pub fn saved_path(&self) -> Option<&Path> {
        match self {
            Self::Saved(path) => Some(path),
            Self::Untitled { .. } => None,
        }
    }

    pub fn suggested_path(&self) -> &Path {
        match self {
            Self::Untitled { suggested_path } | Self::Saved(suggested_path) => suggested_path,
        }
    }

    pub fn display_name(&self) -> String {
        let path = self.suggested_path();
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| path.display().to_string())
    }

    pub fn display_path(&self) -> String {
        self.suggested_path().display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untitled_documents_have_no_writable_path() {
        let location = DocumentLocation::untitled("project.json");
        assert!(location.saved_path().is_none());
        assert_eq!(location.suggested_path(), Path::new("project.json"));
        assert_eq!(location.display_name(), "project.json");
    }
}
