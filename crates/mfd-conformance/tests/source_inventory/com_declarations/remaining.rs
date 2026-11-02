//! Shared cohort checks and the distinct graph/diagnostic/event edge cases.

use super::*;

#[test]
fn all_declared_cohorts_match_independent_oracles_and_disjoint_coverage() {
    let coverage = coverage::check(&load("com-callable-coverage.json"));
    for cohort in &cohorts::DECLARED {
        let inventory = validate::declarations(&load(cohort.file), cohort);
        coverage::declaration_extents(&inventory, &coverage);
        assert_eq!(inventory.records.len(), cohort.records as usize);
        assert_eq!(
            inventory
                .records
                .iter()
                .map(|r| r.parameters.len())
                .sum::<usize>(),
            cohort.parameters
        );
    }
    assert_eq!(coverage.counts.adopted_records, 325);
    assert_eq!(coverage.counts.remaining_records, 0);
    assert_eq!(coverage.counts.ordinary_records, 317);
    assert_eq!(coverage.counts.event_records, 8);
}

#[test]
fn remaining_cohorts_reject_all_scalar_and_unknown_field_mutations() {
    for cohort in cohorts::DECLARED.iter().filter(|c| c.id != "core") {
        let value = load(cohort.file);
        assert!(mutation_walk(&value, &value, "", declared) > cohort.records as usize * 30);
    }
}

#[test]
fn each_cohort_rejects_missing_duplicate_reordered_and_wrong_family_records() {
    for cohort in &cohorts::DECLARED {
        coherent_cases(
            cohort.file,
            declared,
            &[
                |v| {
                    v["records"].as_array_mut().unwrap().remove(0);
                },
                |v| {
                    v["records"][1] = v["records"][0].clone();
                },
                |v| {
                    v["records"].as_array_mut().unwrap().swap(0, 1);
                },
                |v| {
                    let event = v["cohort"] == "events";
                    let ty = &mut v["records"][0]["return_type"];
                    ty["encoded"] = json!(if event { "80190019" } else { "80000018" });
                    ty["leaf"]["name"] = json!(if event { "HRESULT" } else { "VOID" });
                },
                |v| {
                    v["records"][0]["source_anchor"]["function_dump_lines"] = Value::Null;
                },
                |v| {
                    let r = v["records"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|r| !r["parameters"].as_array().unwrap().is_empty())
                        .unwrap();
                    r["parameters"][0]["default_value"] = json!(false);
                },
                |v| {
                    v["sources"]["catalog_sha256"] = json!("0".repeat(64));
                },
                |v| {
                    v["assessment"]["inventory_complete"] = json!(true);
                },
            ],
        );
    }
}

fn record<'a>(value: &'a mut Value, owner: &str, index: u32) -> &'a mut Value {
    value["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["owner"] == owner && r["record_index"] == index)
        .unwrap()
}

#[test]
fn graph_discrepancies_stay_literal_at_both_declaration_levels() {
    let graph = validate::declarations(
        &load("com-graph-declarations.json"),
        cohorts::by_id("graph"),
    );
    let mut inout = Vec::new();
    for r in &graph.records {
        for p in &r.parameters {
            if p.parameter_flags == 3 {
                inout.push((r.name.as_str(), p.index));
                assert_eq!(p.name.as_deref(), Some("pbError"));
                assert_eq!(p.direction, [schema::Direction::In, schema::Direction::Out]);
                assert_eq!(p.datatype.leaf.tuple(), "VARIANT");
                assert_eq!(p.datatype.pointer_depth, 1);
            }
        }
    }
    assert_eq!(inout, [("GenerateOutput", 0), ("Test1", 1)]);
    let flagged: Vec<_> = graph
        .records
        .iter()
        .filter(|r| r.function_flags != 0)
        .map(|r| (r.owner.as_str(), r.name.as_str(), r.function_flags))
        .collect();
    assert_eq!(flagged, [("IComponent", "Test1", 0x40)]);
    let catalog = validate::catalog();
    let online = catalog["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "provisional.com.Component.GenerateOutput")
        .unwrap();
    assert_eq!(online["contract"]["parameters"][0]["direction"], "out");
    assert_eq!(
        online["contract"]["parameters"][0]["declared_type"],
        "Boolean"
    );
    let id = graph
        .records
        .iter()
        .find(|r| r.owner == "IComponent" && r.name == "ID")
        .unwrap();
    assert_eq!(id.parameters[0].datatype.leaf.tuple(), "UINT");
    let online_id = catalog["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "provisional.com.Component.ID")
        .unwrap();
    assert_eq!(
        online_id["contract"]["declared_return_or_property_type"],
        "Long"
    );
    assert!(
        graph
            .records
            .iter()
            .filter(|r| r.name == "HighlightMyConnectionsRecursivly")
            .all(|r| r.catalog_id.is_none())
    );
    // Preserve the older partial observation; it is evidence, not record 326.
    let prior = load("artifact-observations.json");
    assert_eq!(
        prior["installed_declarations"][2]["parameter"]["parameter_flags"],
        3
    );
    assert_eq!(
        graph
            .records
            .iter()
            .filter(|r| r.owner == "IComponent" && r.name == "GenerateOutput")
            .count(),
        1
    );
}

#[test]
fn graph_mutations_cannot_erase_inout_test_flags_pointer_targets_or_unmatched_spelling() {
    coherent_cases(
        "com-graph-declarations.json",
        declared,
        &[
            |v| {
                record(v, "IComponent", 30)["function_flags"] = json!(0);
            },
            |v| {
                let p = &mut record(v, "IComponent", 10)["parameters"][0];
                p["parameter_flags"] = json!(2);
                p["direction"] = json!(["out"]);
            },
            |v| {
                let p = &mut record(v, "IComponent", 30)["parameters"][1];
                p["parameter_flags"] = json!(2);
                p["direction"] = json!(["out"]);
            },
            |v| {
                let p = &mut record(v, "IComponent", 2)["parameters"][0];
                p["type"]["encoded"] = json!("00000020");
                p["type"]["leaf"]["name"] = json!("I4");
            },
            |v| {
                let r = record(v, "IMapForceView", 10);
                r["name"] = json!("HighlightMyConnectionsRecursively");
                r["catalog_id"] =
                    json!("provisional.com.MapForceView.HighlightMyConnectionsRecursively");
            },
            |v| {
                record(v, "IMapping", 7)["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            },
            |v| {
                let replacement = record(v, "IMapping", 7)["parameters"][2]["type"].clone();
                record(v, "IMapping", 5)["parameters"][2]["type"] = replacement;
            },
            |v| {
                let p = &mut record(v, "IMapping", 5)["parameters"][2]["type"];
                p["pointer_depth"] = json!(1);
                p["descriptor_chain"].as_array_mut().unwrap().remove(0);
            },
        ],
    );
}

#[test]
fn event_owners_void_returns_and_five_unrendered_bodies_remain_separate() {
    let events = validate::declarations(
        &load("com-events-declarations.json"),
        cohorts::by_id("events"),
    );
    assert_eq!(
        events
            .records
            .iter()
            .filter(|r| r.source_anchor.function_dump_lines.is_none())
            .count(),
        5
    );
    for r in &events.records {
        assert!(r.owner.starts_with("_I") && r.owner.ends_with("Events"));
        assert_eq!(r.return_type.leaf.tuple(), "VOID");
        assert_eq!(r.invocation_kind, schema::Invocation::Function);
        assert_eq!(
            r.source_anchor.function_dump_lines.is_some(),
            r.type_info_index == 48
        );
        assert!(r.catalog_id.is_some());
    }
    coherent_cases(
        "com-events-declarations.json",
        declared,
        &[
            |v| {
                record(v, "_IDocumentEvents", 0)["source_anchor"]["function_dump_lines"] =
                    json!([15148, 15162]);
            },
            |v| {
                record(v, "_IDocumentEvents", 0)["source_anchor"]
                    .as_object_mut()
                    .unwrap()
                    .remove("function_dump_lines");
            },
            |v| {
                record(v, "_IDocumentEvents", 0)["owner"] = json!("IDocument");
            },
            |v| {
                let r = record(v, "_IProjectEvents", 0);
                r["parameters"][0]["type"]["leaf"]["name"] = json!("IDocument");
                r["parameters"][0]["type"]["leaf"]["type_info_index"] = json!(7);
            },
        ],
    );
}

#[test]
fn diagnostics_do_not_promote_prose_enum_or_separator_runtime_contracts() {
    let diagnostics = validate::declarations(
        &load("com-diagnostics-declarations.json"),
        cohorts::by_id("diagnostics"),
    );
    let severity = diagnostics
        .records
        .iter()
        .find(|r| r.name == "GetLineSeverity")
        .unwrap();
    assert_eq!(severity.parameters[0].datatype.leaf.tuple(), "I4");
    let mut value = load("com-diagnostics-declarations.json");
    record(&mut value, "IAppOutputLine", 6)["runtime_null_delimiter_policy"] = json!("accepted");
    rejects(declared, &value, "invented null delimiter behavior");
}

#[test]
fn remaining_finite_counts_preserve_null_names_generic_leaves_and_source_gaps() {
    let inventories: Vec<_> = cohorts::DECLARED
        .iter()
        .filter(|c| c.id != "core")
        .map(|c| validate::declarations(&load(c.file), c))
        .collect();
    let records: Vec<_> = inventories.iter().flat_map(|c| &c.records).collect();
    let parameters: Vec<_> = records.iter().flat_map(|r| &r.parameters).collect();
    assert_eq!((records.len(), parameters.len()), (183, 240));
    assert_eq!(records.iter().filter(|r| r.catalog_id.is_none()).count(), 6);
    assert_eq!(parameters.iter().filter(|p| p.name.is_none()).count(), 25);
    for p in parameters.iter().filter(|p| p.name.is_none()) {
        assert_eq!(p.parameter_flags, 1);
        assert_eq!(p.name_offset, u32::MAX);
    }
    for (flags, count) in [(1, 104), (3, 2), (10, 134)] {
        assert_eq!(
            parameters
                .iter()
                .filter(|p| p.parameter_flags == flags)
                .count(),
            count
        );
    }
    assert_eq!(
        parameters
            .iter()
            .filter(|p| p.datatype.leaf.tuple() == "DISPATCH")
            .count(),
        28
    );
    assert!(
        parameters
            .iter()
            .filter(|p| matches!(
                p.datatype.leaf,
                schema::Leaf::Primitive {
                    name: schema::Primitive::Dispatch | schema::Primitive::Unknown
                }
            ))
            .all(|p| p.datatype.pointer_depth == 1)
    );
}

#[test]
fn actual_fragment_memberships_cannot_lose_duplicate_or_move_records_between_cohorts() {
    let manifest = load("com-callable-coverage.json");
    let original: std::collections::BTreeMap<_, _> = manifest["cohorts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let file = c["fragment"].as_str().unwrap().to_owned();
            let value = load(&file);
            (file, value)
        })
        .collect();
    for cohort in &cohorts::DECLARED {
        for case in 0..3 {
            let mut fragments = original.clone();
            let rows = fragments.get_mut(cohort.file).unwrap()["records"]
                .as_array_mut()
                .unwrap();
            match case {
                0 => {
                    rows.remove(0);
                }
                1 => {
                    rows[1] = rows[0].clone();
                }
                2 => {
                    let other = if cohort.id == "events" {
                        "core"
                    } else {
                        "events"
                    };
                    rows[0] = original[cohorts::by_id(other).file]["records"][0].clone();
                }
                _ => unreachable!(),
            }
            assert!(
                std::panic::catch_unwind(|| {
                    coverage::check_using(&manifest, |file| &fragments[file]);
                })
                .is_err(),
                "accepted {} membership mutation {case}",
                cohort.id
            );
        }
    }
}
