//! A capability inventory, not a percentage estimate of product parity.
//!
//! Every execution and authoring surface has an independent, evidence-backed
//! assessment. In particular, Ferrule self-roundtrips do not prove that a
//! generated design can execute in the vendor application.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const BUNDLED_LEDGER: &str = include_str!("../../../conformance/mfd-2026r2.json");

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
    NativeMfd,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

pub const DIMENSIONS: [&str; 14] = [
    "import",
    "interpreter",
    "native_export",
    "ferrule_roundtrip",
    "gui",
    "debug",
    "rust",
    "csharp",
    "vendor_backends.xslt1",
    "vendor_backends.xslt2",
    "vendor_backends.xquery1",
    "vendor_backends.cpp",
    "vendor_backends.java",
    "vendor_backends.csharp",
];

impl Status {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unassessed => "unassessed",
            Self::Unverified => "unverified",
            Self::Unsupported => "unsupported",
            Self::Partial => "partial",
            Self::Supported => "supported",
            Self::Blocked => "blocked",
            Self::NotApplicable => "not_applicable",
        }
    }
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

/// Inclusive filters. Empty fields select all known values in that axis.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Selection {
    pub profiles: Vec<String>,
    pub capabilities: Vec<String>,
    pub dimensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSelection {
    pub profiles: Vec<String>,
    pub capabilities: Vec<String>,
    pub dimensions: Vec<String>,
    /// True if every capability and dimension is selected within each profile.
    pub full_profiles: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimensionSummary {
    pub profile: String,
    pub dimension: String,
    pub capabilities: usize,
    pub statuses: BTreeMap<Status, usize>,
    pub evidenced_not_applicable: usize,
    pub unevidenced_not_applicable: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub schema_version: u32,
    pub target: Target,
    pub inventory_complete: bool,
    pub selection: ResolvedSelection,
    pub selected_cells: usize,
    pub by_profile_dimension: Vec<DimensionSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateFailureReason {
    IncompleteInventory,
    StatusNotSupported,
    MissingEvidence,
    MissingVendorExecution,
    UnevidencedNotApplicable,
    NoInScopeCells,
}

#[derive(Debug, Clone, Serialize)]
pub struct GateFailure {
    pub profile: Option<String>,
    pub capability: Option<String>,
    pub dimension: Option<String>,
    pub status: Option<Status>,
    pub reason: GateFailureReason,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GateReport {
    pub summary: Summary,
    pub passed: bool,
    pub in_scope_cells: usize,
    pub excluded_not_applicable_cells: usize,
    pub failures: Vec<GateFailure>,
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
            issues.push("profiles must include native_mfd and ferrule_native_extensions".into());
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

struct SelectionPlan<'a> {
    profiles: Vec<&'a Profile>,
    capabilities: Vec<&'a Capability>,
    dimensions: Vec<&'static str>,
    full_profiles: bool,
}

impl Ledger {
    fn select(&self, selection: &Selection) -> Result<SelectionPlan<'_>, Error> {
        self.validate()?;
        let mut issues = Vec::new();
        let known_profiles: BTreeSet<_> = self.profiles.iter().map(|p| p.id.as_str()).collect();
        let known_capabilities: BTreeSet<_> =
            self.capabilities.iter().map(|c| c.id.as_str()).collect();
        let known_dimensions: BTreeSet<_> = DIMENSIONS.into_iter().collect();
        check_filters("profile", &selection.profiles, &known_profiles, &mut issues);
        check_filters(
            "capability",
            &selection.capabilities,
            &known_capabilities,
            &mut issues,
        );
        check_filters(
            "dimension",
            &selection.dimensions,
            &known_dimensions,
            &mut issues,
        );
        finish(issues)?;

        let mut profiles: Vec<_> = self
            .profiles
            .iter()
            .filter(|profile| {
                selection.profiles.is_empty() || selection.profiles.contains(&profile.id)
            })
            .collect();
        profiles.sort_by(|left, right| left.id.cmp(&right.id));
        let mut capabilities: Vec<_> = self
            .capabilities
            .iter()
            .filter(|capability| {
                (selection.capabilities.is_empty()
                    || selection.capabilities.contains(&capability.id))
                    && profiles
                        .iter()
                        .any(|profile| capability.profiles.contains(&profile.id))
            })
            .collect();
        capabilities.sort_by(|left, right| left.id.cmp(&right.id));
        let dimensions: Vec<_> = DIMENSIONS
            .into_iter()
            .filter(|dimension| {
                selection.dimensions.is_empty()
                    || selection
                        .dimensions
                        .iter()
                        .any(|selected| selected == dimension)
            })
            .collect();
        let missing_profiles: Vec<_> = profiles
            .iter()
            .filter(|profile| {
                !capabilities
                    .iter()
                    .any(|capability| capability.profiles.contains(&profile.id))
            })
            .map(|profile| profile.id.as_str())
            .collect();
        if !missing_profiles.is_empty() {
            return Err(Error::Invalid(vec![format!(
                "selection has no capabilities in profile(s): {}",
                missing_profiles.join(", ")
            )]));
        }
        let full_profiles = dimensions.len() == DIMENSIONS.len()
            && profiles.iter().all(|profile| {
                self.capabilities
                    .iter()
                    .filter(|capability| capability.profiles.contains(&profile.id))
                    .count()
                    == capabilities
                        .iter()
                        .filter(|capability| capability.profiles.contains(&profile.id))
                        .count()
            });
        Ok(SelectionPlan {
            profiles,
            capabilities,
            dimensions,
            full_profiles,
        })
    }
}

fn check_filters(
    field: &str,
    filters: &[String],
    known: &BTreeSet<&str>,
    issues: &mut Vec<String>,
) {
    let mut seen = BTreeSet::new();
    for value in filters {
        if !known.contains(value.as_str()) {
            issues.push(format!("selection has unknown {field} {value}"));
        }
        if !seen.insert(value) {
            issues.push(format!("selection repeats {field} {value}"));
        }
    }
}

fn selected_assessment<'a>(capability: &'a Capability, dimension: &str) -> &'a Assessment {
    capability
        .support
        .assessments()
        .into_iter()
        .find(|(name, _)| *name == dimension)
        .expect("selected dimensions come from Support::assessments")
        .1
}

fn is_vendor_dimension(dimension: &str) -> bool {
    dimension == "native_export" || dimension.starts_with("vendor_backends.")
}

fn has_evidence_kind(ledger: &Ledger, assessment: &Assessment, kinds: &[EvidenceKind]) -> bool {
    assessment.evidence.iter().any(|reference| {
        ledger
            .evidence
            .iter()
            .any(|entry| entry.id == *reference && kinds.contains(&entry.kind))
    })
}

/// Summarize every selected profile/dimension independently in stable order.
pub fn summarize(ledger: &Ledger, selection: &Selection) -> Result<Summary, Error> {
    let plan = ledger.select(selection)?;
    let mut by_profile_dimension = Vec::new();
    let mut selected_cells = 0;
    for profile in &plan.profiles {
        let capabilities: Vec<_> = plan
            .capabilities
            .iter()
            .filter(|capability| capability.profiles.contains(&profile.id))
            .collect();
        for dimension in &plan.dimensions {
            let mut statuses = BTreeMap::new();
            let mut evidenced_not_applicable = 0;
            let mut unevidenced_not_applicable = 0;
            for capability in &capabilities {
                let assessment = selected_assessment(capability, dimension);
                *statuses.entry(assessment.status).or_insert(0) += 1;
                if assessment.status == Status::NotApplicable {
                    if not_applicable_is_evidenced(ledger, profile.kind, assessment, dimension) {
                        evidenced_not_applicable += 1;
                    } else {
                        unevidenced_not_applicable += 1;
                    }
                }
            }
            selected_cells += capabilities.len();
            by_profile_dimension.push(DimensionSummary {
                profile: profile.id.clone(),
                dimension: (*dimension).to_owned(),
                capabilities: capabilities.len(),
                statuses,
                evidenced_not_applicable,
                unevidenced_not_applicable,
            });
        }
    }
    Ok(Summary {
        schema_version: SCHEMA_VERSION,
        target: ledger.target.clone(),
        inventory_complete: ledger.inventory_complete,
        selection: ResolvedSelection {
            profiles: plan.profiles.iter().map(|p| p.id.clone()).collect(),
            capabilities: plan.capabilities.iter().map(|c| c.id.clone()).collect(),
            dimensions: plan.dimensions.iter().map(|d| (*d).to_owned()).collect(),
            full_profiles: plan.full_profiles,
        },
        selected_cells,
        by_profile_dimension,
    })
}

fn not_applicable_is_evidenced(
    ledger: &Ledger,
    profile_kind: ProfileKind,
    assessment: &Assessment,
    dimension: &str,
) -> bool {
    if profile_kind == ProfileKind::NativeMfd && is_vendor_dimension(dimension) {
        has_evidence_kind(
            ledger,
            assessment,
            &[
                EvidenceKind::VendorDocumentation,
                EvidenceKind::VendorExecution,
            ],
        )
    } else {
        !assessment.evidence.is_empty()
    }
}

/// A strict release gate. Only supported cells with evidence pass. Documented
/// non-applicability is excluded; unassessed or blocked cells always fail.
pub fn gate(ledger: &Ledger, selection: &Selection) -> Result<GateReport, Error> {
    let summary = summarize(ledger, selection)?;
    let mut failures = Vec::new();
    let mut in_scope_cells = 0;
    let mut excluded_not_applicable_cells = 0;
    if summary.selection.full_profiles && !ledger.inventory_complete {
        failures.push(GateFailure {
            profile: None,
            capability: None,
            dimension: None,
            status: None,
            reason: GateFailureReason::IncompleteInventory,
            detail: "full-profile gate requires inventory_complete=true".into(),
        });
    }
    for profile in &summary.selection.profiles {
        let profile_kind = ledger
            .profiles
            .iter()
            .find(|entry| entry.id == *profile)
            .expect("resolved profile exists in ledger")
            .kind;
        for capability_id in &summary.selection.capabilities {
            let capability = ledger
                .capabilities
                .iter()
                .find(|candidate| candidate.id == *capability_id)
                .expect("resolved capability exists in ledger");
            if !capability.profiles.contains(profile) {
                continue;
            }
            for dimension in &summary.selection.dimensions {
                let assessment = selected_assessment(capability, dimension);
                if assessment.status == Status::NotApplicable {
                    if not_applicable_is_evidenced(ledger, profile_kind, assessment, dimension) {
                        excluded_not_applicable_cells += 1;
                    } else {
                        failures.push(cell_failure(
                            profile,
                            capability,
                            dimension,
                            assessment,
                            GateFailureReason::UnevidencedNotApplicable,
                            if profile_kind == ProfileKind::NativeMfd
                                && is_vendor_dimension(dimension)
                            {
                                "not_applicable requires vendor documentation or vendor execution evidence"
                            } else {
                                "not_applicable requires evidence"
                            },
                        ));
                    }
                    continue;
                }
                in_scope_cells += 1;
                if assessment.status != Status::Supported {
                    failures.push(cell_failure(
                        profile,
                        capability,
                        dimension,
                        assessment,
                        GateFailureReason::StatusNotSupported,
                        &format!("status {} is not supported", assessment.status.as_str()),
                    ));
                } else if assessment.evidence.is_empty() {
                    failures.push(cell_failure(
                        profile,
                        capability,
                        dimension,
                        assessment,
                        GateFailureReason::MissingEvidence,
                        "supported status requires evidence",
                    ));
                } else if is_vendor_dimension(dimension)
                    && !has_evidence_kind(ledger, assessment, &[EvidenceKind::VendorExecution])
                {
                    failures.push(cell_failure(
                        profile,
                        capability,
                        dimension,
                        assessment,
                        GateFailureReason::MissingVendorExecution,
                        "native export and vendor backends require vendor execution evidence",
                    ));
                }
            }
        }
    }
    if in_scope_cells == 0 {
        failures.push(GateFailure {
            profile: None,
            capability: None,
            dimension: None,
            status: None,
            reason: GateFailureReason::NoInScopeCells,
            detail: "selection contains no applicable cells to qualify".into(),
        });
    }
    Ok(GateReport {
        passed: failures.is_empty(),
        summary,
        in_scope_cells,
        excluded_not_applicable_cells,
        failures,
    })
}

fn cell_failure(
    profile: &str,
    capability: &Capability,
    dimension: &str,
    assessment: &Assessment,
    reason: GateFailureReason,
    detail: &str,
) -> GateFailure {
    GateFailure {
        profile: Some(profile.to_owned()),
        capability: Some(capability.id.clone()),
        dimension: Some(dimension.to_owned()),
        status: Some(assessment.status),
        reason,
        detail: detail.to_owned(),
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
