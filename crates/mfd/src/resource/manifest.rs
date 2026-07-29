//! Explicit, relocatable package manifests for mapping imports.

use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::MfdError;

const CURRENT_VERSION: u32 = 1;
const MANIFEST_KIND: &str = "ferrule.mapping-package";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_CATALOGS: usize = 64;
const MAX_PATH_BYTES: usize = 4096;

/// Canonical package and catalog roots from an explicitly selected manifest.
///
/// The manifest's directory is the package trust boundary. Catalog roots must
/// be portable relative paths contained by that directory. Successful imports
/// embed compiled schemas and do not retain access to these roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageManifest {
    path: PathBuf,
    package_root: PathBuf,
    edi_catalog_roots: Vec<PathBuf>,
    json_schema_catalog_roots: Vec<PathBuf>,
}

impl PackageManifest {
    /// Loads a bounded version-1 package manifest.
    pub fn load(path: &Path) -> Result<Self, MfdError> {
        let path = std::fs::canonicalize(path).map_err(|error| {
            MfdError::Resource(format!(
                "could not canonicalize package manifest `{}` ({error})",
                path.display()
            ))
        })?;
        if !path.is_file() {
            return Err(MfdError::Resource(format!(
                "package manifest `{}` is not a file",
                path.display()
            )));
        }
        let package_root = path
            .parent()
            .ok_or_else(|| MfdError::Resource("package manifest has no parent directory".into()))?
            .to_path_buf();
        let mut file = std::fs::File::open(&path)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(MfdError::Resource(format!(
                "package manifest `{}` exceeds the {} byte limit",
                path.display(),
                MAX_MANIFEST_BYTES
            )));
        }
        let document: ManifestDocument = serde_json::from_slice(&bytes).map_err(|error| {
            MfdError::Resource(format!(
                "could not parse package manifest `{}` ({error})",
                path.display()
            ))
        })?;
        if document.schema_version != CURRENT_VERSION {
            return Err(MfdError::Resource(format!(
                "package manifest `{}` has unsupported schema version {}; expected {}",
                path.display(),
                document.schema_version,
                CURRENT_VERSION
            )));
        }
        if document.kind != MANIFEST_KIND {
            return Err(MfdError::Resource(format!(
                "package manifest `{}` has kind `{}`; expected `{MANIFEST_KIND}`",
                path.display(),
                document.kind
            )));
        }
        if document.catalogs.len() > MAX_CATALOGS {
            return Err(MfdError::Resource(format!(
                "package manifest `{}` declares {} catalogs; the limit is {MAX_CATALOGS}",
                path.display(),
                document.catalogs.len()
            )));
        }

        let mut edi_catalog_roots = Vec::new();
        let mut json_schema_catalog_roots = Vec::new();
        for catalog in document.catalogs {
            let root = resolve_catalog_root(&package_root, &catalog.root, catalog.kind.label())?;
            let roots = match catalog.kind {
                CatalogKind::EdiConfig => &mut edi_catalog_roots,
                CatalogKind::JsonSchema => &mut json_schema_catalog_roots,
            };
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        Ok(Self {
            path,
            package_root,
            edi_catalog_roots,
            json_schema_catalog_roots,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn edi_catalog_roots(&self) -> &[PathBuf] {
        &self.edi_catalog_roots
    }

    pub fn json_schema_catalog_roots(&self) -> &[PathBuf] {
        &self.json_schema_catalog_roots
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestDocument {
    schema_version: u32,
    kind: String,
    #[serde(default)]
    catalogs: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogEntry {
    kind: CatalogKind,
    root: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum CatalogKind {
    #[serde(rename = "edi-config")]
    EdiConfig,
    #[serde(rename = "json-schema")]
    JsonSchema,
}

impl CatalogKind {
    const fn label(self) -> &'static str {
        match self {
            Self::EdiConfig => "EDI configuration catalog",
            Self::JsonSchema => "JSON Schema catalog",
        }
    }
}

fn resolve_catalog_root(
    package_root: &Path,
    declared: &str,
    kind: &str,
) -> Result<PathBuf, MfdError> {
    if declared.is_empty() || declared.contains('\0') {
        return Err(MfdError::Resource(format!(
            "package manifest {kind} path is empty or contains NUL"
        )));
    }
    if declared.len() > MAX_PATH_BYTES {
        return Err(MfdError::Resource(format!(
            "package manifest {kind} path exceeds the {MAX_PATH_BYTES} byte limit"
        )));
    }
    let portable = declared.replace('\\', "/");
    let path = Path::new(&portable);
    if path.is_absolute() || looks_like_windows_absolute(&portable) {
        return Err(MfdError::Resource(format!(
            "package manifest {kind} `{declared}` must be relative"
        )));
    }

    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => relative.push(value),
            Component::ParentDir => {
                return Err(MfdError::Resource(format!(
                    "package manifest {kind} `{declared}` contains parent traversal"
                )));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(MfdError::Resource(format!(
                    "package manifest {kind} `{declared}` must be relative"
                )));
            }
        }
    }
    let candidate = package_root.join(relative);
    let root = std::fs::canonicalize(&candidate).map_err(|error| {
        MfdError::Resource(format!(
            "could not canonicalize package manifest {kind} `{declared}` ({error})"
        ))
    })?;
    if !root.starts_with(package_root) {
        return Err(MfdError::Resource(format!(
            "package manifest {kind} `{declared}` resolves outside package root `{}`",
            package_root.display()
        )));
    }
    if !root.is_dir() {
        return Err(MfdError::Resource(format!(
            "package manifest {kind} `{declared}` is not a directory"
        )));
    }
    Ok(root)
}

fn looks_like_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with("//")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[test]
    fn resolves_ordered_portable_catalogs_inside_package() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = TempDir::new()?;
        let edi = directory.path().join("resources/edi");
        let json = directory.path().join("resources/json");
        std::fs::create_dir_all(&edi)?;
        std::fs::create_dir_all(&json)?;
        let manifest_path = directory.path().join("ferrule-package.json");
        std::fs::write(
            &manifest_path,
            r#"{
  "schemaVersion": 1,
  "kind": "ferrule.mapping-package",
  "catalogs": [
    {"kind": "edi-config", "root": "resources\\edi"},
    {"kind": "json-schema", "root": "resources/json"},
    {"kind": "edi-config", "root": "resources/edi"}
  ]
}"#,
        )?;

        let manifest = PackageManifest::load(&manifest_path)?;

        assert_eq!(manifest.package_root(), directory.path());
        assert_eq!(
            manifest.path(),
            std::fs::canonicalize(&manifest_path)?.as_path()
        );
        assert_eq!(manifest.edi_catalog_roots(), &[edi]);
        assert_eq!(manifest.json_schema_catalog_roots(), &[json]);
        Ok(())
    }

    #[test]
    fn rejects_invalid_identity_paths_and_unknown_fields() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = TempDir::new()?;
        let manifest_path = directory.path().join("ferrule-package.json");
        for (document, expected) in [
            (
                r#"{"schemaVersion":2,"kind":"ferrule.mapping-package"}"#,
                "unsupported schema version 2",
            ),
            (r#"{"schemaVersion":1,"kind":"other"}"#, "has kind `other`"),
            (
                r#"{"schemaVersion":1,"kind":"ferrule.mapping-package","catalogs":[{"kind":"edi-config","root":"../edi"}]}"#,
                "contains parent traversal",
            ),
            (
                r#"{"schemaVersion":1,"kind":"ferrule.mapping-package","catalogs":[{"kind":"edi-config","root":"C:\\catalog"}]}"#,
                "must be relative",
            ),
            (
                r#"{"schemaVersion":1,"kind":"ferrule.mapping-package","packageRoot":"."}"#,
                "unknown field `packageRoot`",
            ),
        ] {
            std::fs::write(&manifest_path, document)?;
            let error =
                PackageManifest::load(&manifest_path).expect_err("invalid manifest must fail");
            assert!(
                error.to_string().contains(expected),
                "{error:?} did not contain {expected:?}"
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_catalog_symlink_escape() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new()?;
        let outside = TempDir::new()?;
        symlink(outside.path(), directory.path().join("catalog"))?;
        let manifest_path = directory.path().join("ferrule-package.json");
        std::fs::write(
            &manifest_path,
            r#"{"schemaVersion":1,"kind":"ferrule.mapping-package",
                "catalogs":[{"kind":"edi-config","root":"catalog"}]}"#,
        )?;

        let error = PackageManifest::load(&manifest_path)
            .expect_err("a catalog symlink outside the package must fail");

        assert!(error.to_string().contains("resolves outside package root"));
        Ok(())
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Result<Self, std::io::Error> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule_package_manifest_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
