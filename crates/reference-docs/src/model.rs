//! Canonical reference data and its repository-level invariants.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::Error;
use crate::text::{RepoPath, Text};

pub const FORMATS_SOURCE: &str = "docs/reference/formats.toml";
pub const WORKSPACE_SOURCE: &str = "docs/reference/workspace.toml";
const MANIFEST: &str = "Cargo.toml";

/// Every canonical source, validated against the repository it describes.
#[derive(Debug)]
pub struct Reference {
    pub formats: Vec<Format>,
    pub areas: Vec<Area>,
    /// Repository URL from the workspace manifest, used for site links.
    pub repository: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatFile {
    format: Vec<Format>,
}

/// One format family's direction, selection, and capability categories.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    pub name: Text,
    pub source: Support,
    pub target: Support,
    pub selection: Text,
    /// Implemented capability backed by `evidence`.
    pub summary: Text,
    pub evidence: Vec<RepoPath>,
    #[serde(default)]
    pub qualified: Vec<QualifiedRoute>,
    #[serde(default)]
    pub unsupported: Vec<Text>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    Yes,
    No,
}

impl Support {
    pub fn label(self) -> &'static str {
        match self {
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }
}

/// A separately recorded route; it covers only its own scope.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedRoute {
    pub route: Text,
    pub scope: Text,
    pub record: RepoPath,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceFile {
    area: Vec<Area>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    pub name: Text,
    pub component: Vec<Component>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub path: RepoPath,
    pub role: Text,
}

#[derive(Deserialize)]
struct Manifest {
    workspace: ManifestWorkspace,
}

#[derive(Deserialize)]
struct ManifestWorkspace {
    members: Vec<String>,
    package: ManifestPackage,
}

#[derive(Deserialize)]
struct ManifestPackage {
    repository: String,
}

impl Reference {
    pub fn load(root: &Path) -> Result<Self, Error> {
        let formats = parse::<FormatFile>(root, FORMATS_SOURCE)?.format;
        let areas = parse::<WorkspaceFile>(root, WORKSPACE_SOURCE)?.area;
        let manifest = parse::<Manifest>(root, MANIFEST)?.workspace;
        validate_formats(root, &formats)?;
        validate_workspace(root, &areas, &manifest.members)?;
        Ok(Self {
            formats,
            areas,
            repository: manifest.package.repository,
        })
    }
}

fn parse<T: for<'de> Deserialize<'de>>(root: &Path, relative: &str) -> Result<T, Error> {
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path,
        source: Box::new(source),
    })
}

fn invalid(file: &str, message: String) -> Error {
    Error::Invalid {
        file: file.to_owned(),
        message,
    }
}

fn require_existing(root: &Path, source: &str, path: &RepoPath) -> Result<(), Error> {
    if root.join(path.as_str()).exists() {
        Ok(())
    } else {
        Err(invalid(
            source,
            format!("`{}` does not exist", path.as_str()),
        ))
    }
}

fn validate_formats(root: &Path, formats: &[Format]) -> Result<(), Error> {
    let mut names = BTreeSet::new();
    for format in formats {
        let name = format.name.as_str();
        if !names.insert(name) {
            return Err(invalid(
                FORMATS_SOURCE,
                format!("duplicate format `{name}`"),
            ));
        }
        if format.source == Support::No && format.target == Support::No {
            return Err(invalid(
                FORMATS_SOURCE,
                format!("`{name}` must support at least one direction"),
            ));
        }
        if format.evidence.is_empty() {
            return Err(invalid(
                FORMATS_SOURCE,
                format!("`{name}` must cite implementation evidence"),
            ));
        }
        let records = format.qualified.iter().map(|route| &route.record);
        for path in format.evidence.iter().chain(records) {
            require_existing(root, FORMATS_SOURCE, path)?;
        }
    }
    if formats.is_empty() {
        return Err(invalid(FORMATS_SOURCE, "no formats are listed".to_owned()));
    }
    Ok(())
}

fn validate_workspace(root: &Path, areas: &[Area], members: &[String]) -> Result<(), Error> {
    let members = members.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut listed = BTreeSet::new();
    for area in areas {
        if area.component.is_empty() {
            return Err(invalid(
                WORKSPACE_SOURCE,
                format!("area `{}` has no components", area.name.as_str()),
            ));
        }
        for component in &area.component {
            let path = component.path.as_str();
            if !listed.insert(path) {
                return Err(invalid(
                    WORKSPACE_SOURCE,
                    format!("`{path}` is listed twice"),
                ));
            }
            require_existing(root, WORKSPACE_SOURCE, &component.path)?;
            let is_package = root.join(path).join(MANIFEST).exists();
            if is_package && !members.contains(path) {
                return Err(invalid(
                    WORKSPACE_SOURCE,
                    format!("`{path}` is a package but not a workspace member"),
                ));
            }
        }
    }
    let missing = members.difference(&listed).copied().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(invalid(
            WORKSPACE_SOURCE,
            format!("workspace members are not listed: {}", missing.join(", ")),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    fn formats(toml: &str) -> Result<(), Error> {
        let file = toml::from_str::<FormatFile>(toml).unwrap();
        validate_formats(root(), &file.format)
    }

    fn workspace(toml: &str, members: &[&str]) -> Result<(), Error> {
        let file = toml::from_str::<WorkspaceFile>(toml).unwrap();
        let members = members
            .iter()
            .map(|member| (*member).to_owned())
            .collect::<Vec<_>>();
        validate_workspace(root(), &file.area, &members)
    }

    const FORMAT: &str = r#"
        [[format]]
        name = "XML"
        source = "yes"
        target = "SUPPORT"
        selection = "`.xml`"
        summary = "Instance I/O"
        evidence = ["EVIDENCE"]
    "#;

    #[test]
    fn formats_require_a_direction_and_existing_evidence() {
        let format = |target: &str, evidence: &str| {
            formats(
                &FORMAT
                    .replace("SUPPORT", target)
                    .replace("EVIDENCE", evidence),
            )
        };
        assert!(format("no", "crates/format-xml/src/xsd.rs").is_ok());
        assert!(format("no", "crates/format-xml/src/missing.rs").is_err());
        let neither = FORMAT
            .replace("source = \"yes\"", "source = \"no\"")
            .replace("SUPPORT", "no")
            .replace("EVIDENCE", "crates/format-xml/src/xsd.rs");
        assert!(formats(&neither).is_err());
        assert!(toml::from_str::<FormatFile>(&format!("{FORMAT}\nextra = 1")).is_err());
    }

    #[test]
    fn workspace_lists_every_member_exactly_once() {
        let area = |paths: &[&str]| {
            let components = paths
                .iter()
                .map(|path| format!("[[area.component]]\npath = \"{path}\"\nrole = \"Role\"\n"))
                .collect::<String>();
            format!("[[area]]\nname = \"Core\"\n{components}")
        };
        assert!(workspace(&area(&["crates/ir", "site"]), &["crates/ir"]).is_ok());
        assert!(workspace(&area(&["crates/ir"]), &["crates/ir", "crates/engine"]).is_err());
        assert!(workspace(&area(&["crates/ir", "crates/ir"]), &["crates/ir"]).is_err());
        assert!(workspace(&area(&["crates/ir", "crates/engine"]), &["crates/ir"]).is_err());
        assert!(workspace(&area(&["crates/ir", "missing"]), &["crates/ir"]).is_err());
    }
}
