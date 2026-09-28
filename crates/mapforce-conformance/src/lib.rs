//! A capability inventory, not a percentage estimate of product parity.
//!
//! Every execution and authoring surface has an independent, evidence-backed
//! assessment. In particular, Ferrule self-roundtrips do not prove that a
//! generated design can execute in the vendor application.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const BUNDLED_LEDGER: &str = include_str!("../../../conformance/mapforce-2026r2.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub schema_version: u32,
    pub target: Target,
    pub inventory_complete: bool,
    pub description: String,
    pub profiles: Vec<Profile>,
    pub evidence: Vec<Evidence>,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub product: String,
    pub release: String,
    pub edition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub kind: ProfileKind,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    MapforceNative,
    FerruleNativeExtensions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: String,
    pub kind: EvidenceKind,
    /// Repository-relative file, or an HTTPS URL for vendor documentation.
    pub location: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Repository,
    VendorDocumentation,
    VendorExecution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub title: String,
    pub category: Category,
    pub profiles: Vec<String>,
    pub depends_on: Vec<String>,
    pub support: Support,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Interoperability,
    Mapping,
    Functions,
    Formats,
    Connectors,
    Authoring,
    Debugging,
    Deployment,
    Extensions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Support {
    pub import: Assessment,
    pub interpreter: Assessment,
    pub native_export: Assessment,
    pub ferrule_roundtrip: Assessment,
    pub gui: Assessment,
    pub debug: Assessment,
    pub rust: Assessment,
    pub csharp: Assessment,
    pub vendor_backends: VendorBackends,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VendorBackends {
    pub xslt1: Assessment,
    pub xslt2: Assessment,
    pub xquery1: Assessment,
    pub cpp: Assessment,
    pub java: Assessment,
    pub csharp: Assessment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub status: Status,
    pub evidence: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unassessed,
    Unverified,
    Unsupported,
    Partial,
    Supported,
    Blocked,
    NotApplicable,
}

impl Support {
    /// Stable dimension names used in diagnostics and external reports.
    pub fn assessments(&self) -> [(&'static str, &Assessment); 14] {
        [
            ("import", &self.import),
            ("interpreter", &self.interpreter),
            ("native_export", &self.native_export),
            ("ferrule_roundtrip", &self.ferrule_roundtrip),
            ("gui", &self.gui),
            ("debug", &self.debug),
            ("rust", &self.rust),
            ("csharp", &self.csharp),
            ("vendor_backends.xslt1", &self.vendor_backends.xslt1),
            ("vendor_backends.xslt2", &self.vendor_backends.xslt2),
            ("vendor_backends.xquery1", &self.vendor_backends.xquery1),
            ("vendor_backends.cpp", &self.vendor_backends.cpp),
            ("vendor_backends.java", &self.vendor_backends.java),
            ("vendor_backends.csharp", &self.vendor_backends.csharp),
        ]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid conformance ledger JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid conformance ledger:\n{}", .0.join("\n"))]
    Invalid(Vec<String>),
}

/// Deserialize strictly and validate all internal references and assessments.
pub fn parse(input: &str) -> Result<Ledger, Error> {
    let ledger: Ledger = serde_json::from_str(input)?;
    ledger.validate()?;
    Ok(ledger)
}

impl Ledger {
    pub fn validate(&self) -> Result<(), Error> {
        let mut issues = Vec::new();
        if self.schema_version != SCHEMA_VERSION {
            issues.push(format!("schema_version must be {SCHEMA_VERSION}"));
        }
        for (field, value) in [
            ("target.product", self.target.product.as_str()),
            ("target.release", self.target.release.as_str()),
            ("target.edition", self.target.edition.as_str()),
            ("description", self.description.as_str()),
        ] {
            nonempty(field, value, &mut issues);
        }
        if self.capabilities.is_empty() {
            issues.push("capabilities must not be empty".into());
        }
        let profiles = unique_ids(
            "profiles",
            self.profiles.iter().map(|p| p.id.as_str()),
            &mut issues,
        );
        let evidence = unique_ids(
            "evidence",
            self.evidence.iter().map(|e| e.id.as_str()),
            &mut issues,
        );
        let capabilities = unique_ids(
            "capabilities",
            self.capabilities.iter().map(|c| c.id.as_str()),
            &mut issues,
        );
        let mut kinds = BTreeSet::new();
        for profile in &self.profiles {
            nonempty(
                &format!("profile {} description", profile.id),
                &profile.description,
                &mut issues,
            );
            if !kinds.insert(profile.kind) {
                issues.push(format!("duplicate profile kind {:?}", profile.kind));
            }
        }
        if kinds.len() != 2 {
            issues
                .push("profiles must include mapforce_native and ferrule_native_extensions".into());
        }
        for entry in &self.evidence {
            nonempty(
                &format!("evidence {} description", entry.id),
                &entry.description,
                &mut issues,
            );
            let valid = match entry.kind {
                EvidenceKind::VendorDocumentation => valid_https_url(&entry.location),
                EvidenceKind::Repository | EvidenceKind::VendorExecution => {
                    portable_path(&entry.location)
                }
            };
            if !valid {
                issues.push(format!("evidence {} has an invalid location", entry.id));
            }
        }
        let vendor_evidence: BTreeSet<_> = self
            .evidence
            .iter()
            .filter(|e| e.kind == EvidenceKind::VendorExecution)
            .map(|e| e.id.as_str())
            .collect();
        let mut used_profiles = BTreeSet::new();
        for capability in &self.capabilities {
            let id = &capability.id;
            nonempty(
                &format!("capability {id} title"),
                &capability.title,
                &mut issues,
            );
            if capability.profiles.is_empty() {
                issues.push(format!("capability {id} must belong to a profile"));
            }
            references(
                &format!("capability {id} profiles"),
                &capability.profiles,
                &profiles,
                &mut issues,
            );
            used_profiles.extend(capability.profiles.iter().map(String::as_str));
            references(
                &format!("capability {id} depends_on"),
                &capability.depends_on,
                &capabilities,
                &mut issues,
            );
            for (dimension, assessment) in capability.support.assessments() {
                let field = format!("capability {id} {dimension}");
                nonempty(&format!("{field} detail"), &assessment.detail, &mut issues);
                references(
                    &format!("{field} evidence"),
                    &assessment.evidence,
                    &evidence,
                    &mut issues,
                );
                let claimed = matches!(assessment.status, Status::Supported | Status::Partial);
                if claimed && assessment.evidence.is_empty() {
                    issues.push(format!(
                        "{field} requires evidence for supported/partial status"
                    ));
                }
                if claimed
                    && (dimension == "native_export" || dimension.starts_with("vendor_backends."))
                    && !assessment
                        .evidence
                        .iter()
                        .any(|id| vendor_evidence.contains(id.as_str()))
                {
                    issues.push(format!(
                        "{field} requires vendor_execution evidence; self-roundtrip is insufficient"
                    ));
                }
            }
        }
        for profile in profiles.difference(&used_profiles) {
            issues.push(format!("profile {profile} has no capabilities"));
        }
        validate_dependencies(&self.capabilities, &mut issues);
        finish(issues)
    }

    /// Verify repository and vendor-run evidence exists inside the supplied root.
    /// HTTPS documentation references are syntax-checked, never fetched here.
    pub fn validate_files(&self, root: &Path) -> Result<(), Error> {
        self.validate()?;
        let root = root
            .canonicalize()
            .map_err(|error| Error::Invalid(vec![format!("repository root: {error}")]))?;
        let mut issues = Vec::new();
        for entry in &self.evidence {
            if entry.kind == EvidenceKind::VendorDocumentation {
                continue;
            }
            match root.join(&entry.location).canonicalize() {
                Ok(path) if path.starts_with(&root) && path.is_file() => {}
                _ => issues.push(format!(
                    "evidence {} references a missing file or a path outside the repository: {}",
                    entry.id, entry.location
                )),
            }
        }
        finish(issues)
    }
}

fn nonempty(field: &str, value: &str, issues: &mut Vec<String>) {
    if value.trim().is_empty() {
        issues.push(format!("{field} must not be empty"));
    }
}

fn unique_ids<'a>(
    field: &str,
    ids: impl Iterator<Item = &'a str>,
    issues: &mut Vec<String>,
) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.is_empty()
            || !id.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'.' | b'_')
            })
        {
            issues.push(format!("{field} has invalid id {id:?}"));
        }
        if !seen.insert(id) {
            issues.push(format!("{field} has duplicate id {id}"));
        }
    }
    seen
}

fn references(field: &str, refs: &[String], known: &BTreeSet<&str>, issues: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for reference in refs {
        if !known.contains(reference.as_str()) {
            issues.push(format!("{field} references unknown id {reference}"));
        }
        if !seen.insert(reference) {
            issues.push(format!("{field} repeats reference {reference}"));
        }
    }
}

fn portable_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn valid_https_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority.contains('.')
            && authority.split('.').all(|label| {
                label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                    && label
                        .as_bytes()
                        .last()
                        .is_some_and(u8::is_ascii_alphanumeric)
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
            && !url.chars().any(char::is_whitespace)
            && !url.contains(['\\', '\0'])
    })
}

fn validate_dependencies(capabilities: &[Capability], issues: &mut Vec<String>) {
    // Kahn's algorithm avoids recursion for inventories of arbitrary depth.
    let mut remaining: BTreeMap<_, _> = capabilities
        .iter()
        .map(|capability| {
            (
                capability.id.as_str(),
                capability
                    .depends_on
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect();
    loop {
        let ready: Vec<_> = remaining
            .iter()
            .filter(|(_, refs)| refs.is_empty())
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            break;
        }
        for id in ready {
            remaining.remove(id);
            for refs in remaining.values_mut() {
                refs.remove(id);
            }
        }
    }
    // Unknown references already have precise diagnostics; only report cycles
    // when every remaining dependency points to an existing capability.
    let known: BTreeSet<_> = capabilities.iter().map(|c| c.id.as_str()).collect();
    if !remaining.is_empty() && remaining.values().flatten().all(|id| known.contains(id)) {
        issues.push(format!(
            "capability dependency cycle involves {}",
            remaining.keys().copied().collect::<Vec<_>>().join(", ")
        ));
    }
}

fn finish(issues: Vec<String>) -> Result<(), Error> {
    if issues.is_empty() {
        Ok(())
    } else {
        Err(Error::Invalid(issues))
    }
}
