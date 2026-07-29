use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[path = "support/survey_import_options.rs"]
mod survey_import_options;

use survey_import_options::SurveyResourceSelection;

#[test]
fn absent_host_configuration_uses_the_sample_root() -> Result<(), Box<dyn Error>> {
    let directory = TestDir::new("default")?;
    let context = SurveyResourceSelection::default().resolve(directory.path())?;

    assert_eq!(context.options.package_root(), Some(directory.path()));
    assert!(context.options.edi_catalog_roots().is_empty());
    assert!(context.options.json_schema_catalog_roots().is_empty());
    assert_eq!(
        context.provenance.to_json(),
        serde_json::json!({
            "package_selection": "default_sample_root",
            "catalog_search_order": [
                "package_root",
                "direct_catalog_roots",
                "manifest_catalog_roots",
            ],
            "edi_catalogs": {
                "direct_root_count": 0,
                "manifest_root_count": 0,
            },
            "json_schema_catalogs": {
                "direct_root_count": 0,
                "manifest_root_count": 0,
            },
            "resource_root_paths_disclosed": false,
        })
    );
    Ok(())
}

#[test]
fn manifest_and_direct_catalogs_retain_cli_precedence_and_order() -> Result<(), Box<dyn Error>> {
    let directory = TestDir::new("ordered")?;
    let package = directory.path().join("package");
    let manifest_edi = package.join("manifest/edi");
    let manifest_json = package.join("manifest/json");
    let direct_edi_a = directory.path().join("direct/edi-a");
    let direct_edi_b = directory.path().join("direct/edi-b");
    let direct_json_a = directory.path().join("direct/json-a");
    let direct_json_b = directory.path().join("direct/json-b");
    for root in [
        &manifest_edi,
        &manifest_json,
        &direct_edi_a,
        &direct_edi_b,
        &direct_json_a,
        &direct_json_b,
    ] {
        std::fs::create_dir_all(root)?;
    }
    let manifest_path = package.join("ferrule-package.json");
    std::fs::write(
        &manifest_path,
        r#"{
          "schemaVersion": 1,
          "kind": "ferrule.mapping-package",
          "catalogs": [
            {"kind": "edi-config", "root": "manifest/edi"},
            {"kind": "json-schema", "root": "manifest/json"}
          ]
        }"#,
    )?;
    let edi_paths = std::env::join_paths([&direct_edi_a, &direct_edi_b])?;
    let json_paths = std::env::join_paths([&direct_json_a, &direct_json_b])?;
    let selection = SurveyResourceSelection::from_values(
        Some(manifest_path.into_os_string()),
        Some(edi_paths),
        Some(json_paths),
    );

    let context = selection.resolve(directory.path())?;

    assert_eq!(
        context.options.package_root(),
        Some(std::fs::canonicalize(&package)?.as_path())
    );
    assert_eq!(
        context.options.edi_catalog_roots(),
        &[
            std::fs::canonicalize(direct_edi_a)?,
            std::fs::canonicalize(direct_edi_b)?,
        ]
    );
    assert_eq!(
        context.options.json_schema_catalog_roots(),
        &[
            std::fs::canonicalize(direct_json_a)?,
            std::fs::canonicalize(direct_json_b)?,
        ]
    );
    let provenance = context.provenance.to_json();
    assert_eq!(provenance["package_selection"], "explicit_manifest");
    assert_eq!(provenance["edi_catalogs"]["direct_root_count"], 2);
    assert_eq!(provenance["edi_catalogs"]["manifest_root_count"], 1);
    assert_eq!(provenance["json_schema_catalogs"]["direct_root_count"], 2);
    assert_eq!(provenance["json_schema_catalogs"]["manifest_root_count"], 1);
    assert_eq!(provenance["resource_root_paths_disclosed"], false);
    Ok(())
}

#[test]
fn package_manifest_catalogs_cannot_escape_the_selected_package() -> Result<(), Box<dyn Error>> {
    let directory = TestDir::new("escape")?;
    let package = directory.path().join("package");
    std::fs::create_dir_all(&package)?;
    std::fs::create_dir_all(directory.path().join("outside"))?;
    let manifest_path = package.join("ferrule-package.json");
    std::fs::write(
        &manifest_path,
        r#"{
          "schemaVersion": 1,
          "kind": "ferrule.mapping-package",
          "catalogs": [{"kind": "edi-config", "root": "../outside"}]
        }"#,
    )?;
    let selection =
        SurveyResourceSelection::from_values(Some(manifest_path.into_os_string()), None, None);

    let error = match selection.resolve(directory.path()) {
        Ok(_) => return Err("escaping manifest catalog was accepted".into()),
        Err(error) => error,
    };

    assert!(error.to_string().contains("contains parent traversal"));
    Ok(())
}

#[test]
fn empty_explicit_resource_values_are_rejected() -> Result<(), Box<dyn Error>> {
    let directory = TestDir::new("empty")?;
    let empty_manifest = SurveyResourceSelection::from_values(Some(OsString::new()), None, None);
    let empty_catalog = SurveyResourceSelection::from_values(None, Some(OsString::new()), None);

    let manifest_error = match empty_manifest.resolve(directory.path()) {
        Ok(_) => return Err("empty manifest selection was accepted".into()),
        Err(error) => error,
    };
    let catalog_error = match empty_catalog.resolve(directory.path()) {
        Ok(_) => return Err("empty catalog selection was accepted".into()),
        Err(error) => error,
    };

    assert!(manifest_error.to_string().contains("must name a file"));
    assert!(
        catalog_error
            .to_string()
            .contains("at least one catalog root")
    );
    Ok(())
}

#[test]
fn direct_catalog_roots_must_exist_when_the_survey_starts() -> Result<(), Box<dyn Error>> {
    let directory = TestDir::new("missing-catalog")?;
    let missing = directory.path().join("missing");
    let selection =
        SurveyResourceSelection::from_values(None, Some(missing.into_os_string()), None);

    let error = match selection.resolve(directory.path()) {
        Ok(_) => return Err("missing direct catalog was accepted".into()),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("could not canonicalize catalog root")
    );
    Ok(())
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        let base = std::env::temp_dir();
        for attempt in 0..1_024 {
            let path = base.join(format!(
                "ferrule-survey-options-{label}-{}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate survey options test directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
