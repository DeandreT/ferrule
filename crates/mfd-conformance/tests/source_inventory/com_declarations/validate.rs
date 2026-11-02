//! Reusable bounded metadata and catalog-join checks for declaration cohorts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::super::load;
use super::cohorts::DeclaredCohort;
use super::oracle;
use super::schema::*;

pub fn catalog() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/mfd-2026r2-source-inventory/com-api.json");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "d9a2fd8a4f8935598fdd2590f837fd6205931ba43c6218dc6b32a29fbe35c16a"
        );
        serde_json::from_slice(&bytes).unwrap()
    })
}

fn sources(s: &Sources) {
    let adopted = load("com-collection-getters.json");
    let t = &adopted["type_library_source"];
    assert_eq!(
        json!([
            s.observation,
            s.member,
            s.cab_member,
            s.bytes,
            s.sha256,
            s.library,
            s.version,
            s.platform,
            s.dump_sha256,
            s.release_binding
        ]),
        json!([
            t["observation"],
            t["member"],
            t["cab_member"],
            t["bytes"],
            t["sha256"],
            t["library"],
            t["version"],
            t["platform"],
            t["dump_sha256"],
            t["release_binding"]
        ])
    );
    assert_eq!(
        json!([s.catalog_file, s.catalog_sha256, s.catalog_release_binding]),
        json!([
            "com-api.json",
            "d9a2fd8a4f8935598fdd2590f837fd6205931ba43c6218dc6b32a29fbe35c16a",
            "unestablished"
        ])
    );
    assert_eq!(
        [
            s.type_info_table_offset,
            s.type_info_record_bytes,
            s.type_info_records,
            s.name_table_offset,
            s.name_table_bytes,
            s.descriptor_table_offset,
            s.descriptor_table_bytes,
            s.descriptor_record_bytes
        ],
        [624, 100, 75, 10804, 13724, 30276, 824, 8]
    );
    assert!(s.type_info_table_offset + s.type_info_records * s.type_info_record_bytes <= s.bytes);
    assert!(s.name_table_offset + s.name_table_bytes <= s.bytes);
    assert!(s.descriptor_table_offset + s.descriptor_table_bytes <= s.bytes);
}

pub fn hex(value: &str) -> u32 {
    assert_eq!(value.len(), 8);
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    u32::from_str_radix(value, 16).unwrap()
}

fn name_bounds(name: &str, offset: u32, source: &Sources) {
    assert!(name.is_ascii() && !name.is_empty());
    assert_eq!(offset % 4, 0);
    assert!(offset + 12 + name.len() as u32 <= source.name_table_bytes);
}

fn datatype(ty: &DataType, inventory: &Declarations, used: &mut BTreeSet<u32>) {
    let mut encoded = hex(&ty.encoded);
    let mut chain = Vec::new();
    let mut depth = 0;
    let leaf = loop {
        if encoded & 0x80000000 != 0 {
            break Leaf::Primitive {
                name: Primitive::from_tag(encoded & 65535),
            };
        }
        assert_eq!(encoded % inventory.sources.descriptor_record_bytes, 0);
        assert!(encoded + 8 <= inventory.sources.descriptor_table_bytes);
        assert!(!chain.contains(&encoded), "cyclic descriptor");
        chain.push(encoded);
        used.insert(encoded);
        let d = inventory
            .descriptors
            .iter()
            .find(|d| d.descriptor_offset == encoded)
            .unwrap();
        let target = hex(&d.target_word);
        match hex(&d.descriptor_word) & 65535 {
            26 => {
                depth += 1;
                encoded = target;
            }
            29 => {
                assert_eq!(target % inventory.sources.type_info_record_bytes, 0);
                let index = target / inventory.sources.type_info_record_bytes;
                let t = inventory
                    .types
                    .iter()
                    .find(|t| t.type_info_index == index)
                    .unwrap();
                assert!(matches!(t.kind, TypeKind::Dispatch | TypeKind::Enum));
                break Leaf::UserDefined {
                    name: t.name.clone(),
                    type_info_index: index,
                    type_kind: t.kind,
                };
            }
            tag => panic!("unreviewed descriptor tag: {tag}"),
        }
    };
    assert_eq!(ty.descriptor_chain, chain);
    assert_eq!(ty.pointer_depth, depth);
    assert_eq!(ty.leaf, leaf);
}

fn catalog_join(record: &Record, online: &Value, cohort: &DeclaredCohort) {
    let matches: Vec<_> = online["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| {
            m["member_identity"]["owner"] == cohort.catalog_owner(record.type_info_index)
                && m["member_identity"]["name"] == record.name
        })
        .collect();
    match &record.catalog_id {
        None => assert!(
            matches.is_empty(),
            "unmatched means exact-name absence, not an alias"
        ),
        Some(id) => {
            assert_eq!(matches.len(), 1);
            let member = matches[0];
            assert_eq!(member["id"], *id);
            assert_eq!(member["source"]["release_binding"], "unestablished");
            let expected_kind = if cohort.events {
                "event"
            } else if record.invocation_kind == Invocation::Function {
                "method"
            } else {
                "property"
            };
            assert_eq!(member["member_identity"]["kind"], expected_kind);
            for p in member["contract"]["parameters"].as_array().unwrap() {
                assert_eq!(
                    p["default_or_optionality"],
                    "no_default_or_omission_contract_stated"
                );
                assert!(p["default_value"].is_null());
            }
        }
    }
}

pub fn declarations(value: &Value, cohort: &DeclaredCohort) -> Declarations {
    let inventory: Declarations = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        inventory.format,
        "mapforce-enterprise-2026r2-com-declarations"
    );
    assert_eq!(inventory.format_version, 1);
    assert_eq!(inventory.cohort, cohort.id);
    inventory.assessment.check();
    assert!(
        oracle::declaration_tuples(&inventory) == cohort.expected(),
        "declaration tuples changed"
    );
    sources(&inventory.sources);
    assert_eq!(inventory.records.len(), cohort.records as usize);
    assert_eq!(
        inventory
            .records
            .iter()
            .map(|r| r.parameters.len())
            .sum::<usize>(),
        cohort.parameters
    );
    let mut used = BTreeSet::new();
    for t in &inventory.types {
        assert!(t.type_info_index < inventory.sources.type_info_records);
        name_bounds(&t.name, t.name_offset, &inventory.sources);
        assert!(t.header_dump_lines[0] > 0 && t.header_dump_lines[0] <= t.header_dump_lines[1]);
    }
    let owners: BTreeMap<_, _> = inventory
        .owners
        .iter()
        .map(|o| (o.type_info_index, o))
        .collect();
    assert_eq!(owners.len(), inventory.owners.len());
    assert!(owners.keys().copied().eq(cohort.owners.iter().map(|o| o.0)));
    for o in &inventory.owners {
        let t = inventory
            .types
            .iter()
            .find(|t| t.type_info_index == o.type_info_index)
            .unwrap();
        assert_eq!(o.name, t.name);
        assert_eq!(t.kind, TypeKind::Dispatch);
        let public_owner = cohort.catalog_owner(o.type_info_index);
        assert_eq!(
            o.name,
            if cohort.events {
                format!("_I{public_owner}Events")
            } else {
                format!("I{public_owner}")
            }
        );
        assert_eq!(o.type_flags, if cohort.events { 0x1000 } else { 0x1140 });
        assert_eq!(
            o.member_block_offset + 4 + o.member_data_bytes,
            o.member_id_table_offset
        );
        assert_eq!(
            o.member_id_table_offset + 4 * o.function_records,
            o.member_name_table_offset
        );
        assert_eq!(
            o.member_name_table_offset + 4 * o.function_records,
            o.member_record_table_offset
        );
        assert!(o.member_record_table_offset + 4 * o.function_records <= inventory.sources.bytes);
    }
    let mut identities = BTreeSet::new();
    for r in &inventory.records {
        assert!(identities.insert((r.type_info_index, r.record_index, r.invocation_kind_value)));
        let o = owners[&r.type_info_index];
        assert_eq!(r.owner, o.name);
        assert!(r.record_index < o.function_records);
        assert!(r.record_offset >= o.member_block_offset + 4);
        assert!(r.record_offset + r.record_bytes <= o.member_id_table_offset);
        assert_eq!(r.invocation_kind_value, r.invocation_kind.value());
        assert_eq!(hex(&r.raw_member_id) as i32, r.dispatch_member_id);
        assert_eq!(r.optional_argument_count, 0); // observed bits, not an omission contract
        name_bounds(&r.name, r.name_offset, &inventory.sources);
        datatype(&r.return_type, &inventory, &mut used);
        assert_eq!(
            r.return_type.leaf,
            Leaf::Primitive {
                name: cohort.return_family()
            }
        );
        assert_eq!(r.return_type.pointer_depth, 0);
        if cohort.events {
            assert_eq!(r.invocation_kind, Invocation::Function);
        }
        assert!(r.record_bytes >= 24 + 12 * r.parameters.len() as u32);
        for (i, p) in r.parameters.iter().enumerate() {
            assert_eq!(p.index as usize, i);
            assert_eq!(
                p.record_offset,
                r.record_offset + r.record_bytes - 12 * (r.parameters.len() - i) as u32
            );
            match &p.name {
                Some(name) => name_bounds(name, p.name_offset, &inventory.sources),
                None => assert_eq!(p.name_offset, u32::MAX),
            }
            let directions: Vec<_> = [
                (1, Direction::In),
                (2, Direction::Out),
                (8, Direction::Retval),
            ]
            .into_iter()
            .filter_map(|(bit, d)| (p.parameter_flags & bit != 0).then_some(d))
            .collect();
            assert_eq!(p.direction, directions);
            assert_eq!(p.parameter_flags & !11, 0); // no optional/default flag observed
            datatype(&p.datatype, &inventory, &mut used);
        }
        let a = &r.source_anchor;
        assert_eq!(
            a.function_dump_lines.is_some(),
            cohort.has_function_dump(r.type_info_index, r.record_index)
        );
        if let Some(lines) = a.function_dump_lines {
            assert!(lines[0] > 0 && lines[0] < lines[1]);
            assert!(lines[1] < a.member_id_dump_line);
        }
        assert!(a.member_id_dump_line > 0);
        assert!(a.member_id_dump_line < a.member_name_dump_line);
        assert!(a.member_name_dump_line < a.member_offset_dump_line);
        catalog_join(r, catalog(), cohort);
    }
    assert_eq!(
        used,
        inventory
            .descriptors
            .iter()
            .map(|d| d.descriptor_offset)
            .collect()
    );
    inventory
}
