//! Finite record membership is distinct from declaration adoption and support.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::super::load;
use super::cohorts;
use super::oracle;
use super::schema::*;

fn fragment(name: &str) -> &'static Value {
    static FRAGMENTS: OnceLock<BTreeMap<&str, Value>> = OnceLock::new();
    &FRAGMENTS.get_or_init(|| {
        [
            "com-generation-signatures.json",
            "com-result-getters.json",
            "com-collection-getters.json",
        ]
        .into_iter()
        .chain(cohorts::DECLARED.iter().map(|c| c.file))
        .map(|file| (file, load(file)))
        .collect()
    })[name]
}

fn registered_cohorts(value: &Coverage) {
    // Legacy schemas remain independent; all shared-schema cohorts use the same
    // descriptors as exact declaration validation, not a second hand-built list.
    let legacy = [
        (
            "generation",
            "adopted",
            Some("com-generation-signatures.json"),
            Some("methods"),
            4,
        ),
        (
            "results",
            "adopted",
            Some("com-result-getters.json"),
            Some("getters"),
            14,
        ),
        (
            "collections",
            "adopted",
            Some("com-collection-getters.json"),
            Some("getters"),
            10,
        ),
    ];
    let expected: Vec<_> = legacy
        .into_iter()
        .chain(
            cohorts::DECLARED
                .iter()
                .map(|c| (c.id, "adopted", Some(c.file), Some("records"), c.records)),
        )
        .collect();
    assert_eq!(value.cohorts.len(), expected.len());
    for (c, (id, state, file, field, count)) in value.cohorts.iter().zip(expected) {
        assert_eq!(
            (
                c.id.as_str(),
                c.state.as_str(),
                c.fragment.as_deref(),
                c.record_field.as_deref(),
                c.record_count
            ),
            (id, state, file, field, count)
        );
    }
}

pub fn check(value: &Value) -> Coverage {
    check_using(value, fragment)
}

pub fn check_using<'a>(value: &Value, get_fragment: impl Fn(&str) -> &'a Value) -> Coverage {
    let coverage: Coverage = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        coverage.format,
        "mapforce-enterprise-2026r2-com-callable-coverage"
    );
    assert_eq!(coverage.format_version, 1);
    assert_eq!(coverage.source, "com-core-declarations.json#sources");
    coverage.assessment.check();
    assert!(
        oracle::coverage_tuples(&coverage) == *oracle::coverage_expected(),
        "coverage tuples changed"
    );
    registered_cohorts(&coverage);
    let c = &coverage.counts;
    assert_eq!(
        [
            c.type_infos,
            c.ordinary_records,
            c.event_records,
            c.adopted_records,
            c.remaining_records,
            c.enum_types,
            c.enum_variables,
            c.zero_callable_types
        ],
        [75, 317, 8, 325, 0, 20, 130, 29]
    );
    let declarations: BTreeMap<_, _> = coverage
        .declarations
        .iter()
        .map(|d| (d.type_info_index, d))
        .collect();
    assert_eq!(declarations.len(), c.type_infos as usize);
    assert!(declarations.keys().copied().eq(0..c.type_infos));
    assert_eq!(
        coverage
            .declarations
            .iter()
            .filter(|d| d.kind == TypeKind::Enum)
            .count(),
        c.enum_types as usize
    );
    assert_eq!(
        coverage
            .declarations
            .iter()
            .map(|d| d.variable_records)
            .sum::<u32>(),
        c.enum_variables
    );
    assert_eq!(
        coverage
            .declarations
            .iter()
            .filter(|d| d.function_records == 0 && d.variable_records == 0)
            .count(),
        c.zero_callable_types as usize
    );
    assert_eq!(
        coverage
            .declarations
            .iter()
            .filter(|d| d.kind == TypeKind::Coclass)
            .count(),
        23
    );
    let mut identities = BTreeSet::new();
    let mut membership: BTreeMap<&str, BTreeSet<(u32, u32)>> = BTreeMap::new();
    let mut ordinary = 0;
    let mut events = 0;
    for r in &coverage.records {
        assert!(identities.insert((r.type_info_index, r.record_index)));
        let declaration = declarations[&r.type_info_index];
        assert_eq!(declaration.kind, TypeKind::Dispatch);
        assert_eq!(r.owner, declaration.name);
        assert!(r.record_index < declaration.function_records);
        assert!(coverage.cohorts.iter().any(|c| c.id == r.cohort));
        assert!(
            membership
                .entry(&r.cohort)
                .or_default()
                .insert((r.type_info_index, r.record_index))
        );
        if r.owner.starts_with("_I") {
            events += 1;
            assert_eq!(r.cohort, "events");
        } else {
            ordinary += 1;
            assert_ne!(r.cohort, "events");
        }
    }
    assert_eq!((ordinary, events), (c.ordinary_records, c.event_records));
    assert_eq!(
        coverage.records.len() as u32,
        c.ordinary_records + c.event_records
    );
    for d in &coverage.declarations {
        let records: Vec<_> = coverage
            .records
            .iter()
            .filter(|r| r.type_info_index == d.type_info_index)
            .collect();
        assert_eq!(records.len(), d.function_records as usize);
        assert!(
            records
                .iter()
                .enumerate()
                .all(|(i, r)| r.record_index as usize == i)
        );
        assert!(
            records
                .windows(2)
                .all(|pair| pair[0].record_offset + pair[0].record_bytes == pair[1].record_offset)
        );
    }
    for cohort in &coverage.cohorts {
        assert_eq!(
            membership[cohort.id.as_str()].len(),
            cohort.record_count as usize
        );
        if let (Some(file), Some(field)) = (&cohort.fragment, &cohort.record_field) {
            let adopted = get_fragment(file);
            let rows = adopted[field].as_array().unwrap();
            assert_eq!(rows.len(), cohort.record_count as usize);
            let mut adopted_membership = BTreeSet::new();
            for row in rows {
                // The original generation fragment has a single owner in its
                // readback header; retain that schema rather than migrating it.
                let owner = row
                    .get("owner")
                    .unwrap_or(&adopted["readback"]["owner"]["name"]);
                let matches: Vec<_> = coverage
                    .records
                    .iter()
                    .filter(|r| {
                        r.cohort == cohort.id
                            && *owner == r.owner
                            && row["record_index"] == r.record_index
                    })
                    .collect();
                assert_eq!(matches.len(), 1);
                let r = matches[0];
                assert!(adopted_membership.insert((r.type_info_index, r.record_index)));
                for (key, expected) in [
                    ("name", json!(r.name)),
                    ("invocation_kind", json!(r.invocation_kind)),
                    ("dispatch_member_id", json!(r.dispatch_member_id)),
                    ("record_offset", json!(r.record_offset)),
                    ("record_bytes", json!(r.record_bytes)),
                ] {
                    assert_eq!(row[key], expected);
                }
                assert_eq!(
                    row["source_anchor"]["function_dump_lines"],
                    json!(r.function_dump_lines)
                );
            }
            assert_eq!(adopted_membership, membership[cohort.id.as_str()]);
        }
    }
    assert_eq!(
        coverage
            .cohorts
            .iter()
            .filter(|c| c.state == "adopted")
            .map(|c| c.record_count)
            .sum::<u32>(),
        c.adopted_records
    );
    assert_eq!(
        coverage
            .cohorts
            .iter()
            .filter(|c| c.state == "remaining")
            .map(|c| c.record_count)
            .sum::<u32>(),
        c.remaining_records
    );
    // Name-only alignment and enum values are independent existing inventories,
    // not more complete signatures or additional capabilities.
    let names = load("com-name-alignment.json");
    for owner in names["interfaces"]
        .as_array()
        .unwrap()
        .iter()
        .chain(names["events"].as_array().unwrap())
    {
        let index = owner["type_info_index"].as_u64().unwrap() as u32;
        assert_eq!(owner["type_library_name"], declarations[&index].name);
        assert_eq!(owner["record_count"], declarations[&index].function_records);
    }
    let enums = load("com-enum-values.json");
    for table in enums["enumerations"].as_array().unwrap() {
        let index = table["type_info_index"].as_u64().unwrap() as u32;
        assert_eq!(table["name"], declarations[&index].name);
        assert_eq!(
            table["members"].as_array().unwrap().len(),
            declarations[&index].variable_records as usize
        );
    }
    coverage
}

pub fn declaration_extents(inventory: &Declarations, coverage: &Coverage) {
    for owner in &inventory.owners {
        let rows: Vec<_> = coverage
            .records
            .iter()
            .filter(|r| r.type_info_index == owner.type_info_index)
            .collect();
        assert_eq!(rows.len(), owner.function_records as usize);
        assert_eq!(
            rows.first().unwrap().record_offset,
            owner.member_block_offset + 4
        );
        let last = rows.last().unwrap();
        assert_eq!(
            last.record_offset + last.record_bytes,
            owner.member_id_table_offset
        );
    }
    let adopted = &coverage
        .records
        .iter()
        .filter(|r| r.cohort == inventory.cohort)
        .collect::<Vec<_>>();
    assert_eq!(adopted.len(), inventory.records.len());
    for (r, c) in inventory.records.iter().zip(adopted) {
        assert_eq!(
            (
                r.type_info_index,
                r.record_index,
                r.record_offset,
                r.record_bytes
            ),
            (
                c.type_info_index,
                c.record_index,
                c.record_offset,
                c.record_bytes
            )
        );
    }
}
