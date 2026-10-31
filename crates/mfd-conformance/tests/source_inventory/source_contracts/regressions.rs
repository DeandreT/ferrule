use super::{Value, check_inventory, command_values, json, ledger_ids, load, reject, rows, text};
use std::collections::BTreeSet;

fn reject_edits(inventory: &mut Value, ledger: &BTreeSet<String>, edits: Vec<(String, Value)>) {
    let saved: Vec<_> = edits
        .into_iter()
        .map(|(pointer, value)| {
            let old = std::mem::replace(inventory.pointer_mut(&pointer).unwrap(), value);
            (pointer, old)
        })
        .collect();
    let rejected = std::panic::catch_unwind(|| check_inventory(inventory, ledger)).is_err();
    for (pointer, old) in saved {
        *inventory.pointer_mut(&pointer).unwrap() = old;
    }
    assert!(rejected, "accepted coherent multi-field mutation");
}

#[test]
fn source_contracts_reject_unresolved_and_misplaced_family_scopes() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    for wrong in ["missing.page", "enterprise2026r2.doc.enumplaceholderwindmf"] {
        reject(
            &mut inventory,
            &ledger,
            "/source_units/API/0/data/existing_family_scope",
            json!(wrong),
        );
        reject(
            &mut inventory,
            &ledger,
            "/source_units/appendix/0/data/existing_family_scope",
            json!(wrong),
        );
    }
    let repeated: Vec<_> = rows(&inventory["source_units"]["API"])
        .iter()
        .enumerate()
        .filter_map(|(i, u)| {
            u["data"]["existing_family_scopes"]
                .as_array()
                .filter(|a| a.len() == 2)
                .map(|_| i)
        })
        .collect();
    assert_eq!(repeated.len(), 2);
    for i in repeated {
        let pointer = format!("/source_units/API/{i}/data/existing_family_scopes");
        let original = inventory.pointer(&pointer).unwrap().clone();
        reject(&mut inventory, &ledger, &pointer, json!([original[0]]));
        reject(
            &mut inventory,
            &ledger,
            &pointer,
            json!([original[0], original[0]]),
        );
        reject(
            &mut inventory,
            &ledger,
            &pointer,
            json!([original[1], original[0]]),
        );
        reject(
            &mut inventory,
            &ledger,
            &format!("{pointer}/1"),
            json!("enterprise2026r2.doc.enumplaceholderwindmf"),
        );
    }
    let workflow = rows(&inventory["source_units"]["API"])
        .iter()
        .position(|u| u["data"].get("outcome_key").is_some())
        .unwrap();
    reject(
        &mut inventory,
        &ledger,
        &format!("/source_units/API/{workflow}/data/existing_parent_id"),
        json!("enterprise2026r2.doc.enumplaceholderwindmf"),
    );
    check_inventory(&inventory, &ledger);
}

#[test]
fn source_contracts_reject_missing_duplicate_and_misconnected_relation_edges() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    for field in [
        "broad_research_source_containment",
        "COM_member_use_or_conflict",
        "COM_command_source_use_or_conflict",
        "COM_page_source_relations",
    ] {
        let pointer = format!("/semantic_relations/{field}");
        reject(&mut inventory, &ledger, &pointer, json!([]));
        let mut missing = rows(inventory.pointer(&pointer).unwrap()).to_vec();
        missing.pop();
        reject(&mut inventory, &ledger, &pointer, json!(missing));
        let mut duplicate = rows(inventory.pointer(&pointer).unwrap()).to_vec();
        duplicate[1] = duplicate[0].clone();
        reject(&mut inventory, &ledger, &pointer, json!(duplicate));
        let mut swapped = rows(inventory.pointer(&pointer).unwrap()).to_vec();
        swapped.swap(0, 1);
        if field == "broad_research_source_containment" || field == "COM_member_use_or_conflict" {
            // Only graph membership matters for these two unordered edge sets.
            let original =
                std::mem::replace(inventory.pointer_mut(&pointer).unwrap(), json!(swapped));
            check_inventory(&inventory, &ledger);
            *inventory.pointer_mut(&pointer).unwrap() = original;
        } else {
            reject(&mut inventory, &ledger, &pointer, json!(swapped));
        }
    }
    for (pointer, wrong) in [
        (
            "/semantic_relations/broad_research_source_containment/0/parent_research_id",
            json!("enterprise2026r2.doc.enumplaceholderwindmf"),
        ),
        (
            "/semantic_relations/broad_research_source_containment/0/canonical_broad_family",
            json!("deployment.host-api"),
        ),
        (
            "/semantic_relations/COM_member_use_or_conflict/0/record/existing_id",
            json!("enterprise2026r2.doc.appcontrol_appearance"),
        ),
        (
            "/semantic_relations/COM_member_use_or_conflict/0/workflow_id",
            json!("enterprise2026r2.doc.ax_run_cs_sample"),
        ),
        (
            "/semantic_relations/COM_command_source_use_or_conflict/0/workflow_id",
            json!("enterprise2026r2.doc.ax_run_cs_sample"),
        ),
        (
            "/semantic_relations/COM_command_source_use_or_conflict/0/selected_source_line",
            json!(63438),
        ),
        (
            "/semantic_relations/COM_page_source_relations/0/source_outcome_ids/0",
            json!("enterprise2026r2.doc.ax_run_cs_sample"),
        ),
    ] {
        reject(&mut inventory, &ledger, pointer, wrong);
    }
    reject_edits(
        &mut inventory,
        &ledger,
        vec![
            (
                "/semantic_relations/COM_page_source_relations/0/to".into(),
                json!(36),
            ),
            (
                "/semantic_relations/COM_page_source_relations/0/to_existing_page_id".into(),
                json!("enterprise2026r2.doc.integrationapplevel"),
            ),
        ],
    );
    check_inventory(&inventory, &ledger);
}

#[test]
fn source_contracts_reject_context_only_member_even_when_views_agree() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    let workflow = "enterprise2026r2.doc.integrationapplevel";
    let old_member = "enterprise2026r2.doc.appcontrol_application";
    let wrong_member = "enterprise2026r2.doc.cg_classref_cpp_datetime";
    let adopted = rows(&inventory["source_units"]["existing_ID_enrichments"]);
    assert!(
        !adopted
            .iter()
            .any(|unit| unit["research_id"] == wrong_member)
    );
    let unit = adopted
        .iter()
        .position(|unit| unit["research_id"] == workflow)
        .unwrap();
    let context = rows(&inventory["research_base_contexts"])
        .iter()
        .find(|row| row["research_id"] == wrong_member)
        .unwrap();
    assert_eq!(context["canonical_id"], Value::Null);
    let page = rows(&inventory["supporting_source"]["com_workflows"]["pages"])
        .iter()
        .position(|page| page["index"] == 36)
        .unwrap();
    let outcome = rows(&inventory["supporting_source"]["com_workflows"]["pages"][page]["outcomes"])
        .iter()
        .position(|outcome| outcome["key"] == "application-level-host")
        .unwrap();
    assert_eq!(
        inventory["supporting_source"]["com_workflows"]["pages"][page]["outcomes"][outcome]["member_refs"]
            [0],
        73
    );
    let mut record = inventory["source_units"]["existing_ID_enrichments"][unit]["data"]["accepted_member_relations"][0].clone();
    assert_eq!(record["existing_id"], old_member);
    let relation = rows(&inventory["semantic_relations"]["COM_member_use_or_conflict"])
        .iter()
        .position(|row| {
            row["workflow_id"] == workflow && row["record"]["existing_id"] == old_member
        })
        .unwrap();
    record["existing_id"] = json!(wrong_member);
    // All three views agree on page 0, but it is only a class-container context,
    // not an adopted COM member endpoint. Agreement cannot qualify that target.
    reject_edits(
        &mut inventory,
        &ledger,
        vec![
            (
                format!(
                    "/supporting_source/com_workflows/pages/{page}/outcomes/{outcome}/member_refs/0"
                ),
                json!(0),
            ),
            (
                format!(
                    "/source_units/existing_ID_enrichments/{unit}/data/accepted_member_relations/0"
                ),
                record.clone(),
            ),
            (
                format!("/semantic_relations/COM_member_use_or_conflict/{relation}/record"),
                record,
            ),
        ],
    );
    check_inventory(&inventory, &ledger);
}

#[test]
fn source_contracts_reject_coherent_numeric_drift_for_every_command() {
    let mut inventory = load();
    let ledger = ledger_ids();
    check_inventory(&inventory, &ledger);
    for (name, expected) in command_values::EXPECTED {
        let i = rows(&inventory["source_units"]["API"])
            .iter()
            .position(|u| u["data"]["member_identity"]["name"] == name)
            .unwrap();
        let unit = &inventory["source_units"]["API"][i];
        let id = text(&unit["research_id"]);
        let mut data = unit["data"].clone();
        assert_eq!(data["member_identity"]["numeric_value"], expected);
        assert!(
            rows(&data["source"]["raw_signature_table_lines"])
                .iter()
                .any(|line| text(line).contains(&expected.to_string()))
        );
        let wrong = expected + 1;
        data["member_identity"]["numeric_value"] = json!(wrong);
        data["member_identity"]["semantic_key"][2] = json!(wrong);
        data["contract"]["numeric_value"] = json!(wrong);
        for occurrence in data["source"]["occurrences"].as_array_mut().unwrap() {
            occurrence["numeric_value"] = json!(wrong);
            occurrence["identity"][2] = json!(wrong);
        }
        let mut edits = Vec::new();
        for (j, placement) in rows(&inventory["supporting_source"]["command_placements"])
            .iter()
            .enumerate()
        {
            if placement["symbol"] == name {
                let mut changed = placement.clone();
                changed["numeric_value"] = json!(wrong);
                changed["identity"][2] = json!(wrong);
                edits.push((
                    format!("/supporting_source/command_placements/{j}"),
                    changed,
                ));
            }
        }
        for (j, relation) in
            rows(&inventory["semantic_relations"]["COM_command_source_use_or_conflict"])
                .iter()
                .enumerate()
        {
            if relation["command_source_proposal_id"] == id {
                edits.push((
                    format!("/semantic_relations/COM_command_source_use_or_conflict/{j}/identity"),
                    data["member_identity"].clone(),
                ));
            }
        }
        edits.push((format!("/source_units/API/{i}/data"), data));
        reject_edits(&mut inventory, &ledger, edits);
    }
    check_inventory(&inventory, &ledger);
}
