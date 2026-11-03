//! Whole frozen page accounting is not product parity or semantic completeness.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

#[path = "documentation_pages/editorial.rs"]
mod editorial;
#[path = "documentation_pages/joins.rs"]
mod joins;
#[path = "documentation_pages/regressions.rs"]
mod regressions;
#[path = "documentation_pages/types.rs"]
mod types;

use joins::{Compared, REGISTRIES};
use types::*;

const BASE: &str = "https://www.altova.com/manual/mapforce/mapforceenterprise/";
const TOC_SHA: &str = "2caa398befc17a69849b7a74b6a32ecbd170029747616aa1dad6542400d7b39b";
const PDF_SHA: &str = "c6b981b664331329a4ab778c7dce6179ae162d27759c58dc9906d0056d815d68";
const TEXT_SHA: &str = "d2fb199b0d69294b525ef7826e1634f9a4249a64f30ce55cf142737c5846e90a";
type Check<T = ()> = Result<T, String>;

fn require(ok: bool, message: &str) -> Check {
    if ok { Ok(()) } else { Err(message.to_owned()) }
}

fn load() -> Value {
    super::load("documentation-pages.json")
}

fn oracle<T: serde::de::DeserializeOwned>(text: &str) -> Check<Vec<T>> {
    text.lines()
        .enumerate()
        .map(|(i, line)| serde_json::from_str(line).map_err(|e| format!("oracle row {i}: {e}")))
        .collect()
}

fn validate(value: Value, compared: &Compared) -> Check {
    let inventory: Inventory = serde_json::from_value(value).map_err(|e| e.to_string())?;
    require(
        inventory.format == "mapforce-enterprise-2026r2-documentation-pages"
            && inventory.format_version == 1,
        "page format",
    )?;
    require(
        !inventory.inventory_complete
            && inventory.page_accounting_complete
            && !inventory.semantic_itemization_complete
            && !inventory.page_units_are_capabilities,
        "unsupported completeness/capability promotion",
    )?;
    require(
        inventory.purpose.contains("metadata, not capabilities")
            && inventory
                .purpose
                .contains("not an additive parity denominator"),
        "page-unit purpose",
    )?;
    let boundary = &inventory.assessment_boundary;
    require(
        boundary.status == "unassessed"
            && boundary.applicability == "unknown"
            && !boundary.runtime_proof
            && boundary.execution_evidence.is_empty()
            && boundary.execution_receipts.is_empty(),
        "unsupported assessment promotion",
    )?;
    require(
        inventory.limitations.len() == 8 && inventory.limitations.iter().all(|s| !s.is_empty()),
        "explicit scope limitations",
    )?;
    require(
        inventory.compared_registries
            == REGISTRIES
                .into_iter()
                .map(|(key, path)| (key, path.to_owned()))
                .collect(),
        "compared registry paths",
    )?;
    sources(&inventory.sources, compared)?;
    editorial::validate(&inventory)?;
    let expected: Vec<ExpectedPage> = oracle(include_str!(
        "../../../../conformance/mfd-2026r2-source-inventory/oracles/documentation-pages.jsonl"
    ))?;
    let bodies: Vec<ExpectedDisposition> = oracle(include_str!(
        "../../../../conformance/mfd-2026r2-source-inventory/oracles/documentation-dispositions.jsonl"
    ))?;
    require(
        expected.len() == 1132 && bodies.len() == 24,
        "finite oracle extents",
    )?;
    let body_map: BTreeMap<_, _> = bodies.iter().map(|body| (body.0, body)).collect();
    require(body_map.len() == bodies.len(), "duplicate editorial page")?;
    require(
        inventory.pages.len() == expected.len(),
        "whole frozen page extent",
    )?;
    let mut seen = BTreeSet::new();
    let mut referenced_files = BTreeSet::new();
    let mut counts = Counts::default();
    for (i, (page, source)) in inventory.pages.iter().zip(&expected).enumerate() {
        let label = format!("page {i} ({})", source.1);
        require(
            seen.insert(&page.source_file),
            &format!("{label}: duplicate page identity"),
        )?;
        page_identity(page, source).map_err(|e| format!("{label}: {e}"))?;
        let expected_joins = compared
            .by_file
            .get(&page.source_file)
            .map(Vec::as_slice)
            .unwrap_or_default();
        require(
            page.joins == expected_joins,
            &format!("{label}: compared-record membership/order"),
        )?;
        require(
            page.joins.iter().collect::<BTreeSet<_>>().len() == page.joins.len(),
            &format!("{label}: duplicate compared join"),
        )?;
        if !page.joins.is_empty() {
            referenced_files.insert(page.source_file.clone());
        }
        for join in &page.joins {
            let record = compared.resolve(join)?;
            context_identity(page, join, record).map_err(|e| format!("{label}: {e}"))?;
            compared_source_range(page, join, record).map_err(|e| format!("{label}: {e}"))?;
        }
        review_level(page).map_err(|e| format!("{label}: {e}"))?;
        disposition(page, body_map.get(&i).copied()).map_err(|e| format!("{label}: {e}"))?;
        count(page, &mut counts);
    }
    require(
        referenced_files == compared.by_file.keys().cloned().collect(),
        "unresolved compared page identities",
    )?;
    require(
        counts == inventory.counts,
        "page counts must be derived, not asserted",
    )?;
    Ok(())
}

fn sources(s: &Sources, compared: &Compared) -> Check {
    require(
        s.toc.official_url == format!("{BASE}index.html")
            && s.toc.sha256 == TOC_SHA
            && s.toc.bytes == 376978
            && s.toc.companion_pointer == "/source_documents/release_and_scope_references/2",
        "pinned TOC header",
    )?;
    require(
        s.pdf.official_url == "https://www.altova.com/documents/MapForceEnterprise.pdf"
            && s.pdf.sha256 == PDF_SHA
            && s.pdf.bytes == 23745188
            && s.pdf.physical_pages == 1751
            && s.pdf.companion_pointer == "/source_documents/manual_pdf",
        "pinned public PDF header",
    )?;
    require(
        s.layout.sha256 == TEXT_SHA
            && s.layout.bytes == 3467690
            && s.layout.lf_count == 76785
            && s.layout.companion_pointer == "/source_documents/manual_text",
        "pinned layout header",
    )?;
    let doc = &compared.documents[&Registry::Contracts];
    for (pointer, field, hash) in [
        (&s.toc.companion_pointer, "uncompressed_sha256", TOC_SHA),
        (&s.pdf.companion_pointer, "sha256", PDF_SHA),
        (&s.layout.companion_pointer, "sha256", TEXT_SHA),
    ] {
        let record = doc
            .pointer(pointer)
            .ok_or_else(|| "source-header pointer".to_owned())?;
        require(record[field] == hash, "source-header hash join")?;
    }
    require(
        doc.pointer(&s.toc.companion_pointer).unwrap()["official_url"] == s.toc.official_url,
        "TOC URL join",
    )?;
    require(
        doc.pointer(&s.pdf.companion_pointer).unwrap()["official_url"] == s.pdf.official_url,
        "PDF URL join",
    )?;
    Ok(())
}

fn page_identity(page: &Page, source: &ExpectedPage) -> Check {
    require(
        page.index == source.0
            && page.source_file == source.1
            && page.manual_path == source.2
            && page.section == source.3
            && page.leaf == source.4
            && page.parent_index == source.5
            && page.toc_position == source.6
            && page.heading_lines == source.7
            && page.physical_pdf_page == source.8
            && page.body_lines == source.9,
        "frozen identity/order/anchor mismatch",
    )?;
    require(
        page.heading_lines[0] == page.body_lines[0]
            && page.heading_lines[1] <= page.body_lines[1]
            && page.body_lines[1] <= 75076
            && page.physical_pdf_page > 0
            && page.physical_pdf_page <= 1751,
        "source range bounds",
    )
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn context_identity(page: &Page, join: &Join, record: &Value) -> Check {
    if join.kind == JoinKind::ResearchContext
        && record["research_id"]
            .as_str()
            .is_some_and(|s| s.starts_with("enterprise2026r2.doc."))
    {
        let path = record["manual_path"]
            .as_array()
            .ok_or_else(|| "context path".to_owned())?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(normalize)
                    .ok_or_else(|| "context path label".to_owned())
            })
            .collect::<Check<Vec<_>>>()?;
        require(path == page.manual_path, "context manual path")?;
        require(
            record["title"].as_str().map(normalize).as_ref() == page.manual_path.last(),
            "context title",
        )?;
        require(
            record["official_url"] == format!("{BASE}{}", page.source_file),
            "context URL",
        )?;
    }
    Ok(())
}

fn compared_source_range(page: &Page, join: &Join, record: &Value) -> Check {
    let (value, whole_body) = match join.kind {
        JoinKind::NonleafBodyReview | JoinKind::FunctionBodyReview => {
            (&record["source"]["text_lines"], true)
        }
        JoinKind::ConstructorReview => (
            &record["source"]["membership_evidence"]["text_lines"],
            false,
        ),
        JoinKind::SupportingScopedReview => (&record["text_lines"], false),
        JoinKind::ApiDecomposition => (&record["text_lines"], true),
        JoinKind::AppendixDecomposition => {
            require(
                record["start"] == page.body_lines[0] && record["end"] == page.body_lines[1],
                "appendix source range",
            )?;
            return Ok(());
        }
        _ => return Ok(()),
    };
    let range: [usize; 2] = serde_json::from_value(value.clone())
        .map_err(|_| "compared source range shape".to_owned())?;
    require(
        range[0] >= page.body_lines[0] && range[0] <= range[1] && range[1] <= page.body_lines[1],
        "compared source anchor outside page body",
    )?;
    if whole_body {
        require(range == page.body_lines, "compared own-body range differs")?;
    }
    if join.kind == JoinKind::NonleafBodyReview {
        require(
            record["source"]["pdf_sha256"] == PDF_SHA
                && record["source"]["text_sha256"] == TEXT_SHA,
            "compared parent source hashes",
        )?;
    }
    Ok(())
}

fn review_level(page: &Page) -> Check {
    let represented = page.joins.iter().any(|join| !join.kind.is_reference());
    let prior = if represented {
        Representation::Represented
    } else if page.joins.is_empty() {
        Representation::Absent
    } else {
        Representation::ReferenceOnly
    };
    require(
        page.prior_representation == prior,
        "prior representation disagrees with actual joins",
    )?;
    let (level, question) = if page
        .joins
        .iter()
        .any(|join| join.kind.is_body_or_decomposition())
    {
        (
            ReviewLevel::ComparedBodyOrDecomposition,
            ReviewQuestion::SemanticExhaustivenessNotEstablished,
        )
    } else if represented {
        (
            ReviewLevel::ComparedContextOrControlOnly,
            ReviewQuestion::ComparedRegistryReviewLevelUnresolved,
        )
    } else {
        (
            ReviewLevel::AuthoredScopedDisposition,
            ReviewQuestion::ScopedDispositionNotContractItemization,
        )
    };
    require(
        page.review_level == level && page.review_question == question,
        "unsupported review-level promotion",
    )
}

fn disposition(page: &Page, expected: Option<&ExpectedDisposition>) -> Check {
    match (&page.disposition, expected) {
        (None, None) => Ok(()),
        (Some(body), Some(source)) => {
            require(
                body.kind == source.1 && body.temporal_scope == source.2,
                "editorial role/revision mismatch",
            )?;
            require(
                body.authored_reason.len() >= 30,
                "explicit authored disposition reason",
            )?;
            require(
                body.open_questions == source.4 && !body.open_questions.is_empty(),
                "unresolved gates changed",
            )?;
            require(
                body.observations.len() == source.3.len(),
                "source restriction/observation extent",
            )?;
            for (observed, (facet, start, end)) in body.observations.iter().zip(&source.3) {
                require(
                    observed.facet == *facet && observed.text_lines == [*start, *end],
                    "frozen editorial facet/anchor",
                )?;
                require(
                    observed.authored_claim.len() >= 30
                        && *start >= page.body_lines[0]
                        && *start <= *end
                        && *end <= page.body_lines[1],
                    "authored claim/range",
                )?;
            }
            Ok(())
        }
        _ => Err("missing or invented page disposition".to_owned()),
    }
}

fn count(page: &Page, counts: &mut Counts) {
    counts.pages += 1;
    if page.leaf {
        counts.leaf_pages += 1;
    } else {
        counts.parent_pages += 1;
    }
    counts.chapters += usize::from(page.parent_index.is_none());
    match page.prior_representation {
        Representation::Represented => counts.represented += 1,
        Representation::ReferenceOnly => counts.reference_only += 1,
        Representation::Absent => counts.absent += 1,
    }
    match page.review_level {
        ReviewLevel::ComparedBodyOrDecomposition => counts.compared_body_or_decomposition += 1,
        ReviewLevel::ComparedContextOrControlOnly => counts.compared_context_or_control_only += 1,
        ReviewLevel::AuthoredScopedDisposition => counts.authored_scoped_disposition += 1,
    }
}

#[test]
fn frozen_whole_page_accounting_preserves_review_and_assessment_gates() {
    validate(load(), &Compared::load().unwrap()).unwrap();
}
