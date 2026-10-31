//! Reconstruct graph edges from source units and independently anchored workflow
//! records, not from the relation arrays being checked. Containment/member edges
//! are unordered graphs; duplicate edges still reject. Source-page links and
//! command-use observations retain their explicit source order.

use super::{BTreeMap, BTreeSet, Value, index, json, rows, text, units};

const COMMAND_USES: [(usize, &str, u64, &str, &str); 8] = [
    (
        36,
        "application-level-host",
        63222,
        "ICActiveXIntegrationOnApplicationLevel",
        "literal_symbol_use_not_new_enum_or_runtime_mode_proof",
    ),
    (
        37,
        "document-level-control-composition",
        63286,
        "ICActiveXIntegrationOnDocumentLevel",
        "literal_symbol_and_value1_use_not_new_enum",
    ),
    (
        38,
        "CSharp-sample-prepare-and-use",
        63438,
        "ICActiveXIntegrationOnDocumentLevel",
        "physically_wrapped_literal_symbol_use_only",
    ),
    (
        42,
        "Java-wrapper-and-control-creation",
        63906,
        "ICActiveXIntegrationOnDocumentLevel",
        "literal_symbol_getValue_call_not_new_enum_or_constructor_signature",
    ),
    (
        42,
        "Java-wrapper-and-control-creation",
        63914,
        "MapForceXProjectWindow",
        "literal_project_window_symbol_getValue_call_only",
    ),
    (
        43,
        "Java-project-placeholder-data-load-example",
        63930,
        "MapForceXProjectWindow",
        "literal_project_window_symbol_use_only",
    ),
    (
        45,
        "Java-menu-action-close-routing",
        64074,
        "ID_WINDOW_CLOSE",
        "same_Close_caption_but_example57602_differs_from_table32453_no_ID_equality",
    ),
    (
        45,
        "Java-menu-action-close-routing",
        64075,
        "ID_WINDOW_CLOSEALL",
        "same_Close_All_caption_but_example34050_differs_from_table32454_no_ID_equality",
    ),
];

const PAGE_EDGES: [(usize, usize, u64, u64, &str); 16] = [
    (
        35,
        34,
        63164,
        63168,
        "missing_OCX_recovery_references_installed_Integration_Package_prerequisites",
    ),
    (
        35,
        36,
        63187,
        63189,
        "application_level_control_selection_reference",
    ),
    (
        35,
        37,
        63187,
        63189,
        "document_level_control_selection_reference",
    ),
    (
        36,
        34,
        63217,
        63217,
        "check_prerequisites_before_one_form_recipe",
    ),
    (36, 35, 63219, 63220, "toolbox_step_reference"),
    (
        37,
        36,
        63270,
        63272,
        "contrast_application_vs_document_control_roles",
    ),
    (37, 35, 63278, 63280, "Visual_Studio_toolbox_reference"),
    (
        38,
        34,
        63421,
        63423,
        "sample_prerequisite_reference_with_VS_version_ambiguity",
    ),
    (
        41,
        34,
        63844,
        63844,
        "check_prerequisites_before_sample_launch",
    ),
    (
        41,
        42,
        63802,
        63805,
        "example_creation_section_pointer_overview_not_extra_activation_proof",
    ),
    (41, 43, 63806, 63808, "example_loading_section_pointer"),
    (
        41,
        44,
        63809,
        63811,
        "overview_pointer_with_unresolved_view_claim_listing_mismatch",
    ),
    (
        41,
        45,
        63812,
        63814,
        "overview_pointer_with_unresolved_validation_out_parameter_claim_listing_mismatch",
    ),
    (41, 46, 63815, 63816, "UI_update_example_pointer"),
    (
        41,
        47,
        63817,
        63819,
        "modal_mapping_table_pointer_with_heading_variant",
    ),
    (48, 34, 64293, 64295, "VBNet_sample_prerequisite_reference"),
];

fn page_id(file: &Value) -> String {
    format!(
        "enterprise2026r2.doc.{}",
        text(file).strip_suffix(".html").unwrap()
    )
}

fn containing_pages(data: &Value) -> Vec<String> {
    let source = &data["source"];
    if let Some(occurrences) = source.get("occurrences") {
        rows(occurrences)
            .iter()
            .map(|row| page_id(&row["source_file"]))
            .collect()
    } else {
        vec![page_id(
            &source.get("page").unwrap_or(source)["source_file"],
        )]
    }
}

fn check_containment(inventory: &Value, ledger: &BTreeSet<String>) {
    let contexts = index(&inventory["research_base_contexts"], "research_id");
    let mut expected = BTreeMap::new();
    for unit in rows(&inventory["source_units"]["API"]) {
        for parent in containing_pages(&unit["data"]) {
            let broad = contexts[parent.as_str()]["broad_canonical_context"].clone();
            assert!(ledger.contains(text(&broad)));
            assert!(
                expected
                    .insert((text(&unit["research_id"]).to_owned(), parent), broad)
                    .is_none()
            );
        }
    }
    let mut actual = BTreeMap::new();
    for row in rows(&inventory["semantic_relations"]["broad_research_source_containment"]) {
        assert_eq!(
            row["relation"],
            "broad_existing_source_page_container_not_exact_API_identity"
        );
        assert!(
            actual
                .insert(
                    (
                        text(&row["source_id"]).to_owned(),
                        text(&row["parent_research_id"]).to_owned()
                    ),
                    row["canonical_broad_family"].clone()
                )
                .is_none()
        );
    }
    assert_eq!(expected.len(), 578);
    assert_eq!(actual, expected);
}

fn workflows(inventory: &Value) -> BTreeMap<(usize, &str), &Value> {
    let pages = &inventory["bounded_source_pages"]["API_pages"];
    let mut result = BTreeMap::new();
    for group in ["API", "existing_ID_enrichments"] {
        for unit in rows(&inventory["source_units"][group]) {
            let data = &unit["data"];
            if let Some(outcome) = data.get("outcome_key") {
                let page = data["master_index"].as_u64().unwrap() as usize;
                assert_eq!(
                    data["existing_parent_id"],
                    pages[page]["research_page_ids"][0]
                );
                assert!(result.insert((page, text(outcome)), unit).is_none());
            }
        }
    }
    assert_eq!(result.len(), 23);
    result
}

fn check_members(inventory: &Value, workflow_units: &BTreeMap<(usize, &str), &Value>) {
    let pages = &inventory["bounded_source_pages"]["API_pages"];
    let adopted = index(
        &inventory["source_units"]["existing_ID_enrichments"],
        "research_id",
    );
    let mut expected = BTreeMap::new();
    let mut seen_outcomes = BTreeSet::new();
    for page in rows(&inventory["supporting_source"]["com_workflows"]["pages"]) {
        let page_index = page["index"].as_u64().unwrap() as usize;
        for outcome in rows(&page["outcomes"]) {
            let key = (page_index, text(&outcome["key"]));
            assert!(seen_outcomes.insert(key));
            let unit = workflow_units[&key];
            let accepted = &unit["data"]["accepted_member_relations"];
            let from_source: Vec<_> = rows(&outcome["member_refs"])
                .iter()
                .map(|i| &pages[i.as_u64().unwrap() as usize]["research_page_ids"][0])
                .collect();
            let from_unit: Vec<_> = rows(accepted).iter().map(|r| &r["existing_id"]).collect();
            assert_eq!(from_unit, from_source);
            // Agreement between source views does not adopt a context-only
            // container as a COM member endpoint.
            for member in from_source {
                assert!(
                    adopted.contains_key(text(member)),
                    "unadopted COM member {member}"
                );
            }
            for record in rows(accepted) {
                let edge = (text(&unit["research_id"]), text(&record["existing_id"]));
                assert!(
                    adopted.contains_key(edge.1),
                    "unadopted COM member {}",
                    edge.1
                );
                assert!(expected.insert(edge, record).is_none());
            }
        }
    }
    assert_eq!(seen_outcomes, workflow_units.keys().copied().collect());
    let mut actual = BTreeMap::new();
    for row in rows(&inventory["semantic_relations"]["COM_member_use_or_conflict"]) {
        let edge = (
            text(&row["workflow_id"]),
            text(&row["record"]["existing_id"]),
        );
        assert!(
            adopted.contains_key(edge.1),
            "unadopted COM member {}",
            edge.1
        );
        assert!(actual.insert(edge, &row["record"]).is_none());
    }
    assert_eq!(expected.len(), 52);
    assert_eq!(actual, expected);
}

fn check_command_uses(inventory: &Value, workflow_units: &BTreeMap<(usize, &str), &Value>) {
    let all_units = units(inventory);
    let commands: BTreeMap<_, _> = all_units
        .values()
        .filter(|u| u["data"]["source"].get("occurrences").is_some())
        .map(|u| (text(&u["data"]["member_identity"]["name"]), u))
        .collect();
    let expected: Vec<_> = COMMAND_USES.into_iter().map(|(page, outcome, line, command, relation)| {
        let workflow = workflow_units[&(page, outcome)];
        let target = commands[command];
        json!({
            "workflow_id":workflow["research_id"], "outcome_key":outcome,
            "selected_source_line":line, "identity":target["data"]["member_identity"],
            "relation":relation, "api_identity_equal":false, "new_parity_row":false,
            "runtime_evidence_transferred":false, "command_source_proposal_id":target["research_id"]
        })
    }).collect();
    assert_eq!(
        inventory["semantic_relations"]["COM_command_source_use_or_conflict"],
        json!(expected)
    );
}

fn check_page_links(inventory: &Value, workflow_units: &BTreeMap<(usize, &str), &Value>) {
    let pages = &inventory["bounded_source_pages"]["API_pages"];
    let sources: BTreeMap<_, _> = rows(&inventory["supporting_source"]["com_source_pages"])
        .iter()
        .map(|p| (p["master_index"].as_u64().unwrap() as usize, p))
        .collect();
    let expected: Vec<_> = PAGE_EDGES
        .into_iter()
        .map(|(from, to, start, end, relation)| {
            let source = &sources[&from]["source"]["fragment"];
            let offset = source["lines"][0].as_u64().unwrap();
            let fragment =
                &rows(&source["raw_lines"])[(start - offset) as usize..=(end - offset) as usize];
            // Keep source-outcome order rather than sorting lexically by its key.
            let outcomes: Vec<_> = rows(&inventory["supporting_source"]["com_workflows"]["pages"])
                .iter()
                .find(|p| p["index"] == from)
                .unwrap()["outcomes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| &workflow_units[&(from, text(&o["key"]))]["research_id"])
                .collect();
            json!({
                "from":from, "to":to, "lines":[start,end], "relation":relation,
                "from_existing_page_id":pages[from]["research_page_ids"][0],
                "to_existing_page_id":pages[to]["research_page_ids"][0],
                "source_fragment":{"lines":[start,end], "raw_lines":fragment},
                "relation_is_runtime_proof":false, "source_outcome_ids":outcomes
            })
        })
        .collect();
    assert_eq!(
        inventory["semantic_relations"]["COM_page_source_relations"],
        json!(expected)
    );
}

pub(super) fn check(inventory: &Value, ledger: &BTreeSet<String>) {
    let all_units = units(inventory);
    let enrichments = index(
        &inventory["semantic_relations"]["exact_research_ID_enrichments"],
        "research_id",
    );
    let enriched = index(
        &inventory["source_units"]["existing_ID_enrichments"],
        "research_id",
    );
    assert_eq!(
        enrichments.keys().collect::<Vec<_>>(),
        enriched.keys().collect::<Vec<_>>()
    );
    for (id, row) in enrichments {
        assert_eq!(row["canonical_id"], all_units[id]["canonical_id"]);
    }
    check_containment(inventory, ledger);
    let workflow_units = workflows(inventory);
    check_members(inventory, &workflow_units);
    check_command_uses(inventory, &workflow_units);
    check_page_links(inventory, &workflow_units);
}
