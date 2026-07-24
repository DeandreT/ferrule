//! Canonical, package-confined resource resolution for `.mfd` imports.

use std::path::{Component, Path, PathBuf};

use crate::MfdError;

/// Filesystem boundary shared by every resource referenced from one mapping.
#[derive(Debug)]
pub(crate) struct ResourceResolver {
    mapping_path: PathBuf,
    mapping_directory: PathBuf,
    package_root: PathBuf,
}

impl ResourceResolver {
    pub(crate) fn new(mapping_path: &Path, package_root: Option<&Path>) -> Result<Self, MfdError> {
        let mapping_path = std::fs::canonicalize(mapping_path).map_err(|error| {
            MfdError::Resource(format!(
                "could not canonicalize mapping `{}` ({error})",
                mapping_path.display()
            ))
        })?;
        let mapping_directory = mapping_path
            .parent()
            .ok_or_else(|| MfdError::Resource("mapping has no parent directory".to_string()))?
            .to_path_buf();
        let package_root = std::fs::canonicalize(package_root.unwrap_or(&mapping_directory))
            .map_err(|error| {
                MfdError::Resource(format!(
                    "could not canonicalize package root `{}` ({error})",
                    package_root.unwrap_or(&mapping_directory).display()
                ))
            })?;
        if !package_root.is_dir() {
            return Err(MfdError::Resource(format!(
                "package root `{}` is not a directory",
                package_root.display()
            )));
        }
        if !mapping_path.starts_with(&package_root) {
            return Err(MfdError::Resource(format!(
                "mapping `{}` is outside package root `{}`",
                mapping_path.display(),
                package_root.display()
            )));
        }
        Ok(Self {
            mapping_path,
            mapping_directory,
            package_root,
        })
    }

    pub(crate) fn mapping_path(&self) -> &Path {
        &self.mapping_path
    }

    pub(crate) fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub(crate) fn package_relative_path(
        &self,
        declared: &str,
        description: &str,
    ) -> Result<PathBuf, String> {
        if declared.is_empty() || declared.contains('\0') {
            return Err(format!("{description} path is empty or contains NUL"));
        }
        let portable = declared.replace('\\', "/");
        if looks_like_windows_absolute(&portable) || Path::new(&portable).is_absolute() {
            return Err(format!("{description} `{declared}` uses an absolute path"));
        }
        let mut normalized = self
            .mapping_directory
            .strip_prefix(&self.package_root)
            .map_err(|_| "mapping directory is outside the package root".to_string())?
            .to_path_buf();
        for component in Path::new(&portable).components() {
            match component {
                Component::CurDir => {}
                Component::Normal(value) => normalized.push(value),
                Component::ParentDir if normalized.pop() => {}
                Component::ParentDir => {
                    return Err(format!(
                        "{description} `{declared}` traverses above package root `{}`",
                        self.package_root.display()
                    ));
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(format!("{description} `{declared}` uses an absolute path"));
                }
            }
        }
        if normalized.as_os_str().is_empty() {
            return Err(format!("{description} `{declared}` does not name a file"));
        }
        Ok(normalized)
    }

    /// Resolves one declared file using portable separators and canonical
    /// containment. Parent components are allowed only when their final target
    /// remains inside the trusted package root.
    pub(crate) fn resolve_file(
        &self,
        declared: &str,
        description: &str,
    ) -> Result<PathBuf, String> {
        if declared.is_empty() || declared.contains('\0') {
            return Err(format!("{description} path is empty or contains NUL"));
        }
        let portable = declared.replace('\\', "/");
        if looks_like_windows_absolute(&portable) {
            return Err(format!(
                "{description} `{declared}` uses an absolute Windows path"
            ));
        }
        let declared_path = Path::new(&portable);
        let candidate = if declared_path.is_absolute() {
            declared_path.to_path_buf()
        } else {
            self.package_root
                .join(self.package_relative_path(declared, description)?)
        };
        let resolved = std::fs::canonicalize(&candidate)
            .or_else(|_| resolve_case_insensitive(&candidate))
            .map_err(|error| {
                format!(
                    "could not resolve {description} `{declared}` from package `{}` ({error})",
                    self.package_root.display()
                )
            })?;
        if !resolved.starts_with(&self.package_root) {
            return Err(format!(
                "{description} `{declared}` resolves outside package root `{}`",
                self.package_root.display()
            ));
        }
        if !resolved.is_file() {
            return Err(format!(
                "{description} `{declared}` does not resolve to a file"
            ));
        }
        Ok(resolved)
    }
}

fn looks_like_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/')
        || path.starts_with("//")
}

fn resolve_case_insensitive(candidate: &Path) -> std::io::Result<PathBuf> {
    let mut current = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(expected) => {
                let direct = current.join(expected);
                if direct.exists() {
                    current = direct;
                    continue;
                }
                let expected = expected.to_str().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "resource path is not valid Unicode",
                    )
                })?;
                let mut matches = std::fs::read_dir(&current)?
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
                    })
                    .map(|entry| entry.path());
                current = matches.next().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "case-insensitive resource path was not found",
                    )
                })?;
                if matches.next().is_some() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "resource has multiple case-insensitive matches",
                    ));
                }
            }
        }
    }
    std::fs::canonicalize(current)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-mfd-resource-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_windows_separators_and_safe_parent_paths_inside_package() {
        let package = TempDir::new();
        let design = package.0.join("Maps");
        let schemas = package.0.join("Resources").join("Schemas");
        std::fs::create_dir_all(&design).unwrap();
        std::fs::create_dir_all(&schemas).unwrap();
        let mapping = design.join("mapping.mfd");
        std::fs::write(&mapping, "<mapping/>").unwrap();
        let schema = schemas.join("Order.JSON");
        std::fs::write(&schema, "{}").unwrap();

        let resolver = ResourceResolver::new(&mapping, Some(&package.0)).unwrap();
        assert_eq!(
            resolver
                .resolve_file(r"..\Resources\Schemas\order.json", "JSON Schema")
                .unwrap(),
            std::fs::canonicalize(schema).unwrap()
        );
    }

    #[test]
    fn rejects_package_escape_absolute_windows_paths_and_outside_mappings() {
        let package = TempDir::new();
        let outside = TempDir::new();
        let mapping = package.0.join("mapping.mfd");
        std::fs::write(&mapping, "<mapping/>").unwrap();
        let escaped = outside.0.join("schema.json");
        std::fs::write(&escaped, "{}").unwrap();
        let resolver = ResourceResolver::new(&mapping, Some(&package.0)).unwrap();

        assert!(
            resolver
                .resolve_file(
                    &format!(
                        "../{}/schema.json",
                        outside.0.file_name().unwrap().to_string_lossy()
                    ),
                    "JSON Schema",
                )
                .is_err()
        );
        assert!(
            resolver
                .resolve_file(r"C:\Resources\schema.json", "JSON Schema")
                .is_err()
        );
        assert!(ResourceResolver::new(&escaped, Some(&package.0)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape_after_canonicalization() {
        use std::os::unix::fs::symlink;

        let package = TempDir::new();
        let outside = TempDir::new();
        let mapping = package.0.join("mapping.mfd");
        std::fs::write(&mapping, "<mapping/>").unwrap();
        let escaped = outside.0.join("schema.json");
        std::fs::write(&escaped, "{}").unwrap();
        symlink(&escaped, package.0.join("schema.json")).unwrap();

        let resolver = ResourceResolver::new(&mapping, Some(&package.0)).unwrap();
        assert!(resolver.resolve_file("schema.json", "JSON Schema").is_err());
    }
}
