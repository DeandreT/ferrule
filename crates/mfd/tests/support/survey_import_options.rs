//! Shared host-selected resource options for the read-only sample surveys.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

pub const PACKAGE_MANIFEST_ENV: &str = "FERRULE_MFD_SURVEY_PACKAGE_MANIFEST";
pub const EDI_CATALOG_ROOTS_ENV: &str = "FERRULE_MFD_SURVEY_EDI_CATALOG_ROOTS";
pub const JSON_SCHEMA_CATALOG_ROOTS_ENV: &str = "FERRULE_MFD_SURVEY_JSON_SCHEMA_CATALOG_ROOTS";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyResourceSelection {
    package_manifest: Option<OsString>,
    edi_catalog_roots: Option<OsString>,
    json_schema_catalog_roots: Option<OsString>,
}

impl SurveyResourceSelection {
    #[allow(dead_code)]
    pub fn from_environment() -> Self {
        Self {
            package_manifest: std::env::var_os(PACKAGE_MANIFEST_ENV),
            edi_catalog_roots: std::env::var_os(EDI_CATALOG_ROOTS_ENV),
            json_schema_catalog_roots: std::env::var_os(JSON_SCHEMA_CATALOG_ROOTS_ENV),
        }
    }

    #[allow(dead_code)]
    pub fn from_values(
        package_manifest: Option<OsString>,
        edi_catalog_roots: Option<OsString>,
        json_schema_catalog_roots: Option<OsString>,
    ) -> Self {
        Self {
            package_manifest,
            edi_catalog_roots,
            json_schema_catalog_roots,
        }
    }

    pub fn resolve(
        &self,
        default_package_root: &Path,
    ) -> Result<SurveyImportContext, Box<dyn Error>> {
        let direct_edi = parse_path_list(EDI_CATALOG_ROOTS_ENV, self.edi_catalog_roots.as_deref())?;
        let direct_json = parse_path_list(
            JSON_SCHEMA_CATALOG_ROOTS_ENV,
            self.json_schema_catalog_roots.as_deref(),
        )?;
        let direct_edi = canonical_catalog_roots(EDI_CATALOG_ROOTS_ENV, direct_edi)?;
        let direct_json = canonical_catalog_roots(JSON_SCHEMA_CATALOG_ROOTS_ENV, direct_json)?;
        let mut options = mfd::ImportOptions::default()
            .with_edi_catalog_roots(direct_edi.iter().cloned())
            .with_json_schema_catalog_roots(direct_json.iter().cloned());

        let (package_selection, manifest_edi_count, manifest_json_count) =
            if let Some(manifest_path) =
                nonempty_path(PACKAGE_MANIFEST_ENV, self.package_manifest.as_deref())?
            {
                let manifest = mfd::PackageManifest::load(&manifest_path)?;
                let edi_count = manifest.edi_catalog_roots().len();
                let json_count = manifest.json_schema_catalog_roots().len();
                options = options.with_package_manifest(&manifest_path)?;
                (PackageSelection::ExplicitManifest, edi_count, json_count)
            } else {
                options = options.with_package_root(default_package_root);
                (PackageSelection::DefaultSampleRoot, 0, 0)
            };

        Ok(SurveyImportContext {
            options,
            provenance: SurveyResourceProvenance {
                package_selection,
                direct_edi_catalog_count: direct_edi.len(),
                manifest_edi_catalog_count: manifest_edi_count,
                direct_json_schema_catalog_count: direct_json.len(),
                manifest_json_schema_catalog_count: manifest_json_count,
            },
        })
    }
}

#[derive(Debug)]
pub struct SurveyImportContext {
    pub options: mfd::ImportOptions,
    pub provenance: SurveyResourceProvenance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackageSelection {
    DefaultSampleRoot,
    ExplicitManifest,
}

impl PackageSelection {
    const fn label(self) -> &'static str {
        match self {
            Self::DefaultSampleRoot => "default_sample_root",
            Self::ExplicitManifest => "explicit_manifest",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurveyResourceProvenance {
    package_selection: PackageSelection,
    direct_edi_catalog_count: usize,
    manifest_edi_catalog_count: usize,
    direct_json_schema_catalog_count: usize,
    manifest_json_schema_catalog_count: usize,
}

impl SurveyResourceProvenance {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "package_selection": self.package_selection.label(),
            "catalog_search_order": [
                "package_root",
                "direct_catalog_roots",
                "manifest_catalog_roots",
            ],
            "edi_catalogs": {
                "direct_root_count": self.direct_edi_catalog_count,
                "manifest_root_count": self.manifest_edi_catalog_count,
            },
            "json_schema_catalogs": {
                "direct_root_count": self.direct_json_schema_catalog_count,
                "manifest_root_count": self.manifest_json_schema_catalog_count,
            },
            "resource_root_paths_disclosed": false,
        })
    }
}

fn nonempty_path(name: &str, value: Option<&OsStr>) -> Result<Option<PathBuf>, io::Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(invalid_input(format!("{name} must name a file")));
    }
    Ok(Some(PathBuf::from(value)))
}

fn parse_path_list(name: &str, value: Option<&OsStr>) -> Result<Vec<PathBuf>, io::Error> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_empty() {
        return Err(invalid_input(format!(
            "{name} must contain at least one catalog root"
        )));
    }
    let paths = std::env::split_paths(value).collect::<Vec<_>>();
    if paths.is_empty() || paths.iter().any(|path| path.as_os_str().is_empty()) {
        return Err(invalid_input(format!(
            "{name} contains an empty catalog root"
        )));
    }
    Ok(paths)
}

fn canonical_catalog_roots(name: &str, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, io::Error> {
    let mut roots = Vec::with_capacity(paths.len());
    for path in paths {
        let canonical = std::fs::canonicalize(&path).map_err(|error| {
            invalid_input(format!(
                "could not canonicalize catalog root from {name} ({error})"
            ))
        })?;
        if !canonical.is_dir() {
            return Err(invalid_input(format!(
                "catalog root from {name} is not a directory"
            )));
        }
        if !roots.contains(&canonical) {
            roots.push(canonical);
        }
    }
    Ok(roots)
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
