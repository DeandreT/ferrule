//! Independent pinned catalog and release exceptions, not a self-check of totals.
//! No vendor binary, network access, COM activation or local reference tree required.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{load, source_only, unique_ids};

const LIBRARY_SHA: &str = "785c484fbaa882cae724a3568b04650300eb18941084ac23eb2d041dfb4a2789";
const CATALOG_SHA: &str = "d9a2fd8a4f8935598fdd2590f837fd6205931ba43c6218dc6b32a29fbe35c16a";
const DUMP_SHA: &str = "672097183d6a6b0d80f3aa5f92d02172bb3a588609cb743bb82b73c342192bb4";

// Bounded raw readback anchors from the hash-pinned official-release TLB.
// The existing name alignment independently checks these owner/type identities.
const OWNERS: [(&str, u64, u64); 20] = [
    ("ENUMDOMType", 2, 35292),
    ("ENUMLibType", 3, 35392),
    ("ENUMProjectType", 4, 35460),
    ("ENUMWrapperClassesVersion", 5, 35848),
    ("ENUMApacheAxisVersion", 6, 35948),
    ("ENUMComponentType", 12, 42020),
    ("ENUMComponentSubType", 13, 42376),
    ("ENUMComponentUsageKind", 17, 44168),
    ("ENUMComponentDatapointSide", 18, 44364),
    ("ENUMSearchDatapointFlags", 20, 44640),
    ("ENUMConnectionType", 22, 44920),
    ("ENUMProgrammingLanguage", 23, 45020),
    ("ENUMCodeGenErrorLevel", 26, 45936),
    ("ENUMOutputGenerationResultCode", 29, 46736),
    ("ENUMCodeGenerationResultCode", 31, 47296),
    ("ENUMProjectItemType", 36, 51232),
    ("ENUMApplicationStatus", 38, 52104),
    ("ENUMAppOutputLine_Severity", 39, 52268),
    ("ENUMAppOutputLine_TextDecoration", 40, 53136),
    ("ENUMViewMode", 47, 53556),
];

const EXTRAS: [(&str, &str, i32); 5] = [
    ("ENUMProjectType", "eVisualStudio2026Project", 16),
    ("ENUMProjectType", "eDotNet10_0", 17),
    (
        "ENUMWrapperClassesVersion",
        "eWrapperClassesVersion_2005r3",
        0,
    ),
    (
        "ENUMWrapperClassesVersion",
        "eWrapperClassesVersion_2006",
        1,
    ),
    ("ENUMProjectItemType", "eProjectItemType_Count", 11),
];

const SENTINELS: [(&str, &str); 3] = [
    ("ENUMProgrammingLanguage", "eUndefinedLanguage"),
    ("ENUMProjectItemType", "eProjectItemType_Invalid"),
    ("ENUMAppOutputLine_Severity", "eSeverity_Undefined"),
];

struct Sources {
    catalog: Value,
    alignment: Value,
    artifacts: Value,
}

impl Sources {
    fn load() -> Self {
        Self {
            catalog: load("com-api.json"),
            alignment: load("com-name-alignment.json"),
            artifacts: load("artifact-observations.json"),
        }
    }
}

fn named<'a>(rows: &'a Value, field: &str, name: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row[field] == name)
        .unwrap()
}

fn check_sources(inventory: &Value, sources: &Sources) {
    assert_eq!(
        inventory["format"],
        "mapforce-enterprise-2026r2-com-enum-values"
    );
    assert_eq!(inventory["format_version"], 1);
    assert_eq!(inventory["inventory_complete"], false);
    assert_eq!(inventory["runtime_proof"], false);
    assert_eq!(inventory["status"], "unassessed");
    assert_eq!(inventory["applicability"], "unknown");
    assert_eq!(inventory["execution_evidence"], json!([]));
    assert_eq!(
        inventory["type_library_source"],
        json!({
            "observation":"artifact-observations.json", "member":"MapForce.tlb",
            "sha256":LIBRARY_SHA, "library":"MapForceLib", "version":"4.1",
            "platform":"SYS_WIN64", "dump_sha256":DUMP_SHA,
            "cab_member":"MapForce.tlb_1193", "bytes":54108, "release_binding":"2026r2"
        })
    );
    let artifact = named(
        &sources.artifacts["matched_members"],
        "name",
        "MapForce.tlb",
    );
    for field in ["sha256", "cab_member", "bytes"] {
        assert_eq!(inventory["type_library_source"][field], artifact[field]);
    }
    assert_eq!(sources.artifacts["release"]["release"], "2026r2");
    assert_eq!(
        inventory["catalog_source"],
        json!({"file":"com-api.json", "release_binding":"unestablished", "sha256":CATALOG_SHA})
    );
    assert_eq!(sources.catalog["release_binding"], "unestablished");
    assert_eq!(
        inventory["catalog_source"],
        sources.alignment["catalog_source"]
    );
    for field in ["sha256", "dump_sha256", "library", "version", "platform"] {
        assert_eq!(
            inventory["type_library_source"][field],
            sources.alignment["type_library_source"][field]
        );
    }
    assert_eq!(inventory["remaining"].as_array().unwrap().len(), 4);
    source_only(inventory);
}

fn check_readback(inventory: &Value) {
    let readback = &inventory["value_readback"];
    assert_eq!(readback["source"], "type_library_source");
    for (field, value) in [
        ("type_info_table_offset", 624),
        ("type_info_record_bytes", 100),
        ("type_info_records", 75),
        ("name_table_offset", 10804),
        ("name_table_bytes", 13724),
        ("constant_segment_offset", 31100),
        ("constant_segment_bytes", 100),
        ("variable_record_bytes", 20),
        ("variable_kind", 2),
    ] {
        assert_eq!(readback[field], value);
    }
    assert_eq!(readback["variable_datatype"], "80030016");
    assert_eq!(
        readback["referenced_values"],
        json!([{"segment_relative_offset":84, "file_offset":31184, "vartype":3, "signed_value":-1}])
    );
    assert_eq!(
        readback["dump_boundary"],
        "ENUMViewMode has no rendered variable records in the pinned dump; its three values come from bounded raw declaration records, not inferred dump text."
    );
}

fn release_order<'a>(owner: &str, documented: &'a Value) -> Vec<&'a str> {
    let mut names: Vec<_> = documented["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["name"].as_str().unwrap())
        .collect();
    // Other tables have exactly the pinned catalog order. These three need
    // explicit release positions, not sorting by value or guessing aliases.
    match owner {
        "ENUMProjectType" => {
            assert_eq!(names.pop(), Some("eProjectTypeCount"));
            names.extend([
                "eVisualStudio2026Project",
                "eDotNet10_0",
                "eProjectTypeCount",
            ]);
        }
        "ENUMWrapperClassesVersion" => {
            names.splice(
                0..0,
                [
                    "eWrapperClassesVersion_2005r3",
                    "eWrapperClassesVersion_2006",
                ],
            );
        }
        "ENUMProjectItemType" => {
            names.retain(|name| *name != "eProjectItemType_Invalid");
            names.extend(["eProjectItemType_Count", "eProjectItemType_Invalid"]);
        }
        _ => {}
    }
    names
}

fn expected_member(owner: &str, name: &str, documented: &Value, index: usize, block: u64) -> Value {
    let catalog_member = documented["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == name);
    let (value, class, literal) = if let Some((_, _, value)) = EXTRAS
        .iter()
        .find(|(extra_owner, extra_name, _)| *extra_owner == owner && *extra_name == name)
    {
        assert!(catalog_member.is_none());
        (*value, "type-library-only", Value::Null)
    } else {
        let literal = &catalog_member.unwrap()["value_literal"];
        let documented_value = literal.as_str().unwrap().parse::<i32>().unwrap();
        if (owner, name) == ("ENUMProjectType", "eProjectTypeCount") {
            assert_eq!(documented_value, 16);
            (18, "different", literal.clone())
        } else {
            (documented_value, "equal", literal.clone())
        }
    };
    let (encoded, storage) = if SENTINELS.contains(&(owner, name)) {
        assert_eq!(value, -1);
        ("00000054".to_owned(), "referenced-i4")
    } else {
        assert!((0..1 << 25).contains(&value));
        (format!("{:08x}", 0x8c00_0000 | value as u32), "inline-i4")
    };
    json!({
        "name":name, "value":value, "record_index":index,
        "record_offset":block + 4 + 20 * index as u64,
        "encoded_value":encoded, "value_storage":storage,
        "catalog":{"classification":class, "value_literal":literal}
    })
}

fn check_inventory(inventory: &Value, sources: &Sources) {
    check_sources(inventory, sources);
    check_readback(inventory);
    let enums = inventory["enumerations"].as_array().unwrap();
    assert_eq!(enums.len(), OWNERS.len());
    unique_ids(enums, "name");
    unique_ids(enums, "catalog_id");
    let mut seen_types = BTreeSet::new();
    let mut constants = 0;
    let mut equal = 0;
    let mut different = 0;
    let mut extra = 0;
    let mut referenced = 0;
    for (owner, type_index, block) in OWNERS {
        let row = named(&inventory["enumerations"], "name", owner);
        let documented = named(&sources.catalog["enumerations"], "name", owner);
        let aligned = named(&sources.alignment["enumerations"], "catalog_name", owner);
        assert_eq!(row["type_info_index"], type_index);
        assert!(seen_types.insert(row["type_info_index"].as_u64().unwrap()));
        assert_eq!(row["type_info_index"], aligned["type_info_index"]);
        assert_eq!(row["member_block_offset"], block);
        assert_eq!(row["catalog_id"], documented["id"]);
        assert_eq!(row["catalog_source"], documented["source"]);
        let expected_order = release_order(owner, documented);
        let members = row["members"].as_array().unwrap();
        unique_ids(members, "name");
        assert_eq!(members.len(), expected_order.len());
        assert_eq!(aligned["record_count"], members.len());
        let aligned_extras: BTreeSet<_> = aligned["type_library_only"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        let expected_extras: BTreeSet<_> = EXTRAS
            .iter()
            .filter(|(extra_owner, _, _)| *extra_owner == owner)
            .map(|(_, name, _)| *name)
            .collect();
        assert_eq!(aligned_extras, expected_extras);
        for (index, (member, name)) in members.iter().zip(expected_order).enumerate() {
            assert_eq!(
                member,
                &expected_member(owner, name, documented, index, block),
                "{owner}.{name}"
            );
            match member["catalog"]["classification"].as_str().unwrap() {
                "equal" => equal += 1,
                "different" => different += 1,
                "type-library-only" => extra += 1,
                other => panic!("unexpected classification {other}"),
            }
            if member["value_storage"] == "referenced-i4" {
                referenced += 1;
            }
        }
        constants += members.len();
    }
    assert_eq!(
        inventory["counts"],
        json!({
            "enumerations":20, "constants":130, "catalog_constants":125,
            "equal_values":124, "different_values":1, "type_library_only":5,
            "catalog_only":0, "referenced_signed_values":3
        })
    );
    assert_eq!(
        (constants, equal, different, extra, referenced),
        (130, 124, 1, 5, 3)
    );
    assert_eq!(sources.catalog["counts"]["enum_members"], equal + different);
    let original = named(
        &sources.artifacts["installed_declarations"],
        "id",
        "installed.com.project-type-values",
    );
    let project = named(&inventory["enumerations"], "name", "ENUMProjectType");
    for old_member in original["members"].as_array().unwrap() {
        let member = named(
            &project["members"],
            "name",
            old_member["name"].as_str().unwrap(),
        );
        for field in ["value", "encoded_value"] {
            assert_eq!(old_member[field], member[field]);
        }
    }
}

fn rejects(inventory: &Value, sources: &Sources, label: &str) {
    assert!(
        std::panic::catch_unwind(|| check_inventory(inventory, sources)).is_err(),
        "accepted mutation: {label}"
    );
}

#[test]
fn com_enum_values_cover_exact_release_declarations_without_execution_claims() {
    check_inventory(&load("com-enum-values.json"), &Sources::load());
    let canonical: Value =
        serde_json::from_str(include_str!("../../../../conformance/mfd-2026r2.json")).unwrap();
    assert_eq!(canonical["inventory_complete"], false);
}

#[test]
fn com_enum_values_reject_missing_duplicate_or_misowned_declarations() {
    let sources = Sources::load();
    let original = load("com-enum-values.json");
    for (i, row) in original["enumerations"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let mut missing = original.clone();
        missing["enumerations"].as_array_mut().unwrap().remove(i);
        rejects(&missing, &sources, "missing owner");
        let mut duplicate = original.clone();
        duplicate["enumerations"][(i + 1) % OWNERS.len()] = row.clone();
        rejects(&duplicate, &sources, "same-count duplicate owner");
        let members = row["members"].as_array().unwrap();
        for (j, member) in members.iter().enumerate() {
            let mut missing = original.clone();
            missing["enumerations"][i]["members"]
                .as_array_mut()
                .unwrap()
                .remove(j);
            rejects(&missing, &sources, "missing constant");
            let mut duplicate = original.clone();
            duplicate["enumerations"][i]["members"][(j + 1) % members.len()] = member.clone();
            rejects(&duplicate, &sources, "same-count duplicate constant");
            let mut renamed = original.clone();
            renamed["enumerations"][i]["members"][j]["name"] = json!("wrongOwnerOrSpelling");
            rejects(&renamed, &sources, "changed constant identity");
        }
    }
}

#[test]
fn com_enum_values_reject_coherent_value_and_classification_mutations() {
    let sources = Sources::load();
    let original = load("com-enum-values.json");
    for (i, row) in original["enumerations"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for (j, member) in row["members"].as_array().unwrap().iter().enumerate() {
            let mut changed = original.clone();
            let wrong = member["value"].as_i64().unwrap() + 1;
            let target = &mut changed["enumerations"][i]["members"][j];
            // Change redundant fields together: internal consistency must not
            // substitute for the separately pinned catalog/release observations.
            target["value"] = json!(wrong);
            target["encoded_value"] = json!(format!("{:08x}", 0x8c00_0000 | wrong as u32));
            target["value_storage"] = json!("inline-i4");
            if !target["catalog"]["value_literal"].is_null() {
                target["catalog"]["value_literal"] = json!(wrong.to_string());
            }
            rejects(&changed, &sources, "coherent changed value");
            let mut changed = original.clone();
            changed["enumerations"][i]["members"][j]["catalog"]["classification"] =
                json!(if member["catalog"]["classification"] == "equal" {
                    "different"
                } else {
                    "equal"
                });
            rejects(&changed, &sources, "wrong comparison classification");
        }
    }
    for (owner, name) in SENTINELS {
        for wrong in [1, 84, 4_294_967_295_i64] {
            let mut changed = original.clone();
            let row = changed["enumerations"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["name"] == owner)
                .unwrap();
            let member = row["members"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|member| member["name"] == name)
                .unwrap();
            member["value"] = json!(wrong);
            rejects(
                &changed,
                &sources,
                "lost negative sign or decoded offset as value",
            );
        }
    }
}

#[test]
fn com_enum_values_reject_changed_source_records_and_hashes() {
    let sources = Sources::load();
    let original = load("com-enum-values.json");
    for (pointer, value) in [
        ("/type_library_source/sha256", json!("0".repeat(64))),
        ("/type_library_source/dump_sha256", json!("0".repeat(64))),
        ("/type_library_source/cab_member", json!("other.tlb")),
        ("/catalog_source/sha256", json!("0".repeat(64))),
        ("/value_readback/referenced_values/0/signed_value", json!(1)),
        ("/value_readback/referenced_values/0/vartype", json!(19)),
        (
            "/value_readback/referenced_values/0/segment_relative_offset",
            json!(0),
        ),
        ("/value_readback/referenced_values/0/file_offset", json!(84)),
        (
            "/value_readback/dump_boundary",
            json!("all tables rendered"),
        ),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        rejects(&changed, &sources, pointer);
    }
    for (i, row) in original["enumerations"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for (field, wrong) in [
            ("type_info_index", json!(0)),
            ("member_block_offset", json!(0)),
            ("catalog_id", json!("provisional.com.WrongOwner")),
        ] {
            let mut changed = original.clone();
            changed["enumerations"][i][field] = wrong;
            rejects(&changed, &sources, field);
        }
        for field in ["raw_sha256", "source_file", "official_url"] {
            let mut changed = original.clone();
            changed["enumerations"][i]["catalog_source"][field] = json!("0".repeat(64));
            rejects(&changed, &sources, "wrong catalog page source");
        }
        for (j, _) in row["members"].as_array().unwrap().iter().enumerate() {
            for (field, wrong) in [
                ("record_index", json!(999)),
                ("record_offset", json!(0)),
                ("encoded_value", json!("ffffffff")),
                ("value_storage", json!("unsigned")),
            ] {
                let mut changed = original.clone();
                changed["enumerations"][i]["members"][j][field] = wrong;
                rejects(&changed, &sources, field);
            }
        }
    }
}

#[test]
fn com_enum_values_reject_completeness_release_and_runtime_promotions() {
    let sources = Sources::load();
    let original = load("com-enum-values.json");
    for (pointer, value) in [
        ("/inventory_complete", json!(true)),
        ("/runtime_proof", json!(true)),
        ("/status", json!("verified")),
        ("/applicability", json!("applicable")),
        (
            "/execution_evidence",
            json!(["declaration is not execution"]),
        ),
        ("/catalog_source/release_binding", json!("2026r2")),
        (
            "/enumerations/0/catalog_source/release_binding",
            json!("2026r2"),
        ),
        ("/counts/equal_values", json!(125)),
        ("/counts/type_library_only", json!(0)),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        rejects(&changed, &sources, pointer);
    }
}
