//! Strict page-accounting metadata, deliberately separate from capabilities.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Inventory {
    pub format: String,
    pub format_version: u32,
    pub purpose: String,
    pub inventory_complete: bool,
    pub page_accounting_complete: bool,
    pub semantic_itemization_complete: bool,
    pub page_units_are_capabilities: bool,
    pub sources: Sources,
    pub compared_registries: BTreeMap<Registry, String>,
    pub assessment_boundary: Boundary,
    pub limitations: Vec<String>,
    pub counts: Counts,
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Boundary {
    pub status: String,
    pub applicability: String,
    pub runtime_proof: bool,
    pub execution_evidence: Vec<String>,
    pub execution_receipts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sources {
    pub toc: Toc,
    pub pdf: Pdf,
    pub layout: Layout,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Toc {
    pub official_url: String,
    pub sha256: String,
    pub bytes: usize,
    pub companion_pointer: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pdf {
    pub official_url: String,
    pub sha256: String,
    pub bytes: usize,
    pub physical_pages: usize,
    pub companion_pointer: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Layout {
    pub sha256: String,
    pub bytes: usize,
    pub lf_count: usize,
    pub companion_pointer: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Counts {
    pub pages: usize,
    pub leaf_pages: usize,
    pub parent_pages: usize,
    pub chapters: usize,
    pub represented: usize,
    pub reference_only: usize,
    pub absent: usize,
    pub authored_scoped_disposition: usize,
    pub compared_body_or_decomposition: usize,
    pub compared_context_or_control_only: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    pub index: usize,
    pub source_file: String,
    pub manual_path: Vec<String>,
    pub section: String,
    pub leaf: bool,
    #[serde(deserialize_with = "required_option")]
    pub parent_index: Option<usize>,
    pub toc_position: [usize; 2],
    pub heading_lines: [usize; 2],
    pub physical_pdf_page: usize,
    pub body_lines: [usize; 2],
    pub joins: Vec<Join>,
    pub prior_representation: Representation,
    pub review_level: ReviewLevel,
    pub review_question: ReviewQuestion,
    #[serde(deserialize_with = "required_option")]
    pub disposition: Option<Disposition>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Disposition {
    pub kind: String,
    pub temporal_scope: String,
    pub authored_reason: String,
    pub observations: Vec<Observation>,
    pub open_questions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub facet: String,
    pub text_lines: [usize; 2],
    pub authored_claim: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Representation {
    Represented,
    ReferenceOnly,
    Absent,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewLevel {
    ComparedBodyOrDecomposition,
    ComparedContextOrControlOnly,
    AuthoredScopedDisposition,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewQuestion {
    SemanticExhaustivenessNotEstablished,
    ComparedRegistryReviewLevelUnresolved,
    ScopedDispositionNotContractItemization,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum Registry {
    Contracts,
    Ledger,
    Nonleaf,
    Functions,
    Constructors,
    Supporting,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(super) struct Join {
    pub registry: Registry,
    pub pointer: String,
    pub kind: JoinKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(super) enum JoinKind {
    ResearchContext,
    NonleafBodyReview,
    FunctionBodyReview,
    ConstructorReview,
    SupportingScopedReview,
    ApiDecomposition,
    AppendixDecomposition,
    SourceReference,
    EvidenceReference,
}

impl JoinKind {
    pub fn is_body_or_decomposition(self) -> bool {
        matches!(
            self,
            Self::NonleafBodyReview
                | Self::FunctionBodyReview
                | Self::ConstructorReview
                | Self::SupportingScopedReview
                | Self::ApiDecomposition
                | Self::AppendixDecomposition
        )
    }

    pub fn is_reference(self) -> bool {
        matches!(self, Self::SourceReference | Self::EvidenceReference)
    }
}

/// Independent source tuple; not serialized from candidate page objects.
#[derive(Debug, Deserialize)]
pub(super) struct ExpectedPage(
    pub usize,
    pub String,
    pub Vec<String>,
    pub String,
    pub bool,
    pub Option<usize>,
    pub [usize; 2],
    pub [usize; 2],
    pub usize,
    pub [usize; 2],
);

/// Editorial source readback: page, role, revision, facets/anchors, open gates.
#[derive(Debug, Deserialize)]
pub(super) struct ExpectedDisposition(
    pub usize,
    pub String,
    pub String,
    pub Vec<(String, usize, usize)>,
    pub Vec<String>,
);
