//! Coherent edits must not turn source accounting into semantic qualification.

use serde_json::{Value, json};

use super::{Compared, Counts, Page, count, load, validate};

fn refresh_counts(value: &mut Value) {
    let mut counts = Counts::default();
    for row in value["pages"].as_array().unwrap() {
        let page: Page = serde_json::from_value(row.clone()).unwrap();
        count(&page, &mut counts);
    }
    value["counts"] = serde_json::to_value(counts).unwrap();
}

fn rejects(edit: impl FnOnce(&mut Value)) {
    let compared = Compared::load().unwrap();
    let mut value = load();
    edit(&mut value);
    assert!(
        validate(value, &compared).is_err(),
        "accepted unsupported page-accounting mutation"
    );
}

#[test]
fn omitted_page_with_refreshed_counts_and_indexes_is_rejected() {
    rejects(|value| {
        value["pages"].as_array_mut().unwrap().remove(3);
        for (i, row) in value["pages"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            row["index"] = json!(i);
        }
        refresh_counts(value);
    });
}

#[test]
fn duplicate_and_reordered_source_identities_are_rejected() {
    rejects(|value| {
        value["pages"][4]["source_file"] = value["pages"][3]["source_file"].clone();
    });
    rejects(|value| {
        value["pages"].as_array_mut().unwrap().swap(3, 4);
        value["pages"][3]["index"] = json!(3);
        value["pages"][4]["index"] = json!(4);
        refresh_counts(value);
    });
}

#[test]
fn coherent_source_range_and_source_header_changes_are_rejected() {
    rejects(|value| {
        value["pages"][3]["heading_lines"] = json!([913, 913]);
        value["pages"][3]["body_lines"][0] = json!(913);
    });
    rejects(|value| {
        value["pages"][3]["toc_position"][1] = json!(123);
    });
    rejects(|value| {
        value["sources"]["layout"]["sha256"] = json!("0".repeat(64));
    });
    rejects(|value| {
        value["sources"]["toc"]["companion_pointer"] = json!("/source_documents/manual_pdf");
    });
    rejects(|value| {
        value["sources"]["pdf"]["bytes"] = json!(1);
    });
    rejects(|value| {
        value["pages"][1131]["body_lines"][1] = json!(76786);
    });
}

#[test]
fn existing_pointer_to_wrong_page_is_not_a_valid_join() {
    rejects(|value| {
        // A real pointer and real body-review kind do not establish membership
        // for the target page. Refresh the asserted level and counts as well.
        value["pages"][3]["joins"] = json!([{
            "registry":"nonleaf", "pointer":"/coverage/0", "kind":"nonleaf_body_review"
        }]);
        value["pages"][3]["prior_representation"] = json!("represented");
        value["pages"][3]["review_level"] = json!("compared_body_or_decomposition");
        value["pages"][3]["review_question"] = json!("semantic_exhaustiveness_not_established");
        refresh_counts(value);
    });
    rejects(|value| {
        value["pages"][0]["joins"][0]["pointer"] =
            json!("/source_documents/release_and_scope_references/5");
    });
    rejects(|value| {
        value["pages"][17]["joins"][0]["pointer"] = json!("/research_base_contexts/999999");
    });
}

#[test]
fn context_only_review_questions_cannot_be_promoted_coherently() {
    rejects(|value| {
        let mut changed = 0;
        for row in value["pages"].as_array_mut().unwrap() {
            if row["review_level"] == "compared_context_or_control_only" {
                row["review_level"] = json!("compared_body_or_decomposition");
                row["review_question"] = json!("semantic_exhaustiveness_not_established");
                changed += 1;
            }
        }
        assert_eq!(changed, 462);
        refresh_counts(value);
    });
    rejects(|value| {
        value["pages"][17]["review_question"] = json!("semantic_itemization_complete");
    });
    rejects(|value| {
        value["semantic_itemization_complete"] = json!(true);
    });
}

#[test]
fn history_cannot_be_relabelled_current_and_packaging_is_not_legal_only() {
    for index in [3, 4, 5, 6] {
        rejects(|value| {
            value["pages"][index]["disposition"]["temporal_scope"] = json!("pinned_release");
        });
    }
    rejects(|value| {
        value["pages"][2]["disposition"]["observations"]
            .as_array_mut()
            .unwrap()
            .remove(1);
    });
    rejects(|value| {
        value["pages"][1131]["disposition"]["kind"] = json!("legal_context");
        value["pages"][1131]["disposition"]["open_questions"] = json!(["legal_scope_not_decided"]);
    });
    rejects(|value| {
        value["pages"][12]["disposition"]["kind"] = json!("provenance_overview");
    });
    rejects(|value| {
        value["pages"][3]["disposition"]["observations"][0]["authored_claim"] = json!(
            "All historical additions establish current backend eligibility and complete runtime support."
        );
    });
}

#[test]
fn matrix_restrictions_and_unknown_eligibility_cannot_be_dropped() {
    for index in [1085, 1086] {
        rejects(|value| {
            value["pages"][index]["disposition"]["observations"]
                .as_array_mut()
                .unwrap()
                .pop();
        });
        rejects(|value| {
            value["pages"][index]["disposition"]["open_questions"] = json!([]);
        });
        rejects(|value| {
            value["pages"][index]["disposition"]["open_questions"] =
                json!(["all_future_backends_excluded"]);
        });
        rejects(|value| {
            value["pages"][index]["review_level"] = json!("compared_body_or_decomposition");
            refresh_counts(value);
        });
    }
    rejects(|value| {
        value["pages"][1085]["disposition"]["observations"][2]["authored_claim"] =
            json!("Every backend is inapplicable for all XML operations and all future profiles.");
    });
    rejects(|value| {
        value["limitations"][4] =
            json!("Online COM is release-bound and all schema wrapper APIs are complete.");
    });
}

#[test]
fn capability_runtime_completeness_and_blanket_na_promotions_are_rejected() {
    for field in [
        "inventory_complete",
        "semantic_itemization_complete",
        "page_units_are_capabilities",
    ] {
        rejects(|value| {
            value[field] = json!(true);
        });
    }
    rejects(|value| {
        value["assessment_boundary"]["runtime_proof"] = json!(true);
    });
    rejects(|value| {
        value["assessment_boundary"]["status"] = json!("supported");
    });
    rejects(|value| {
        value["assessment_boundary"]["applicability"] = json!("not_applicable");
    });
    rejects(|value| {
        value["assessment_boundary"]["execution_evidence"] = json!(["documentation"]);
    });
    rejects(|value| {
        value["assessment_boundary"]["execution_receipts"] = json!(["invented"]);
    });
    rejects(|value| {
        value["pages"][0]["canonical_id"] = json!("invented.capability");
    });
    rejects(|value| {
        value["pages"][1085]["disposition"]["backend_applicability"] = json!("not_applicable");
    });
}

#[test]
fn unknown_fields_missing_nulls_and_incorrect_editorial_shape_are_rejected() {
    rejects(|value| {
        value["extra"] = json!(false);
    });
    rejects(|value| {
        value["pages"][0]
            .as_object_mut()
            .unwrap()
            .remove("parent_index");
    });
    rejects(|value| {
        value["pages"][17]
            .as_object_mut()
            .unwrap()
            .remove("disposition");
    });
    rejects(|value| {
        value["pages"][17]["disposition"] = value["pages"][0]["disposition"].clone();
    });
    rejects(|value| {
        value["pages"][0]["disposition"] = Value::Null;
    });
    rejects(|value| {
        value["pages"][0]["disposition"]["authored_reason"] = json!("");
    });
    rejects(|value| {
        value["pages"][1085]["disposition"]["observations"][0]["text_lines"] = json!([1, 2]);
    });
    rejects(|value| {
        value["pages"][1085]["disposition"]["observations"][0]["facet"] =
            json!("all_cells_eligible");
    });
}

#[test]
fn stale_compared_body_anchors_and_hashes_are_rejected() {
    for (registry, pointer, replacement) in [
        (
            super::Registry::Nonleaf,
            "/coverage/0/source/text_lines",
            json!([1678, 1761]),
        ),
        (
            super::Registry::Nonleaf,
            "/coverage/0/source/pdf_sha256",
            json!("0".repeat(64)),
        ),
        (
            super::Registry::Functions,
            "/source_units/0/source/text_lines",
            json!([34262, 34301]),
        ),
        (
            super::Registry::Constructors,
            "/source_units/xs_constructors/0/source/membership_evidence/text_lines",
            json!([1, 2]),
        ),
        (
            super::Registry::Supporting,
            "/coverage/0/text_lines",
            json!([1, 2]),
        ),
    ] {
        let mut compared = Compared::load().unwrap();
        *compared
            .documents
            .get_mut(&registry)
            .unwrap()
            .pointer_mut(pointer)
            .unwrap() = replacement;
        assert!(
            validate(load(), &compared).is_err(),
            "accepted stale compared source metadata"
        );
    }
}
