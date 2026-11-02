//! Literal oracle facts plus explicit bounded ownership/reachability checks.
//!
//! The expected tuples were transcribed from dump text independently of the
//! candidate binary reader. Deterministic anchors/joins omitted from the compact
//! oracle are checked here; no vendor artifact is opened by any test.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::super::{load, source_only};
use super::metadata;

fn number(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

fn word(v: &Value) -> u32 {
    let s = v.as_str().unwrap();
    assert_eq!(s.len(), 8);
    assert!(
        s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    u32::from_str_radix(s, 16).unwrap()
}

fn row(v: &Value, length: usize) -> &[Value] {
    let r = v.as_array().unwrap();
    assert_eq!(r.len(), length);
    r
}

fn expected() -> &'static Vec<Value> {
    static ROWS: OnceLock<Vec<Value>> = OnceLock::new();
    ROWS.get_or_init(|| {
        include_str!("../oracles/com-type-relationships.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    })
}

pub fn oracle(v: &Value) {
    let mut tuples = Vec::new();
    for t in v["types"].as_array().unwrap() {
        let t = row(t, 14);
        let mut r = vec![json!("T")];
        r.extend_from_slice(&t[..5]);
        r.extend_from_slice(&t[6..13]);
        r.push(row(&t[13], 6)[3].clone());
        tuples.push(Value::Array(r));
    }
    for g in v["guids"].as_array().unwrap() {
        let mut r = vec![json!("G")];
        r.extend_from_slice(&row(g, 6)[..4]);
        tuples.push(Value::Array(r));
    }
    for r in v["references"].as_array().unwrap() {
        row(r, 9);
        tuples.push(json!(["R", r[0], r[3], r[5], r[6], r[7]]));
    }
    assert!(
        tuples == *expected(),
        "type/global-table source facts changed"
    );
}

pub fn structure(v: &Value) {
    let types = v["types"].as_array().unwrap();
    let guids = v["guids"].as_array().unwrap();
    let refs = v["references"].as_array().unwrap();
    assert_eq!((types.len(), guids.len(), refs.len()), (75, 62, 32));
    let mut names = BTreeSet::new();
    let mut used_guids = BTreeSet::new();
    let mut categories = [0; 5]; // ordinary, event, dummy, coclass, enum
    let (mut zero, mut ordinary, mut events, mut variables) = (0, 0, 0, 0);
    for (i, t) in types.iter().enumerate() {
        row(t, 14);
        assert_eq!(t[0], i);
        let name = t[1].as_str().unwrap();
        assert!(names.insert(name));
        let a = row(&t[13], 6);
        let ni = number(&a[3]);
        assert!(ni < 453);
        assert_eq!(
            t[13],
            json!([
                624 + 100 * i,
                194 + 28 * i,
                221 + 28 * i,
                ni,
                2693 + 6 * ni,
                2698 + 6 * ni
            ])
        );
        assert_eq!(number(&t[3]) % 4, 0);
        assert!(number(&t[3]) + 12 + name.len() as u64 <= 13724);
        let counts = row(&t[11], 2);
        let (functions, vars) = (number(&counts[0]), number(&counts[1]));
        zero += usize::from(functions == 0 && vars == 0);
        variables += vars;
        word(&t[7]);
        word(&t[10]);
        word(&row(&t[12], 4)[3]);
        let category = match t[2].as_str().unwrap() {
            "TKIND_ENUM" => {
                assert_eq!(t[4], "ffffffff");
                assert!(t[5].is_null());
                assert_eq!(t[8], 0);
                assert_eq!(t[9], "ffffffff");
                assert_eq!(functions, 0);
                4
            }
            "TKIND_COCLASS" => {
                assert_eq!((functions, vars), (0, 0));
                assert!((1..=2).contains(&number(&t[8])));
                3
            }
            "TKIND_DISPATCH" => {
                assert_eq!(vars, 0);
                assert_eq!(t[8], 1);
                if name.ends_with("Events") {
                    assert!(name.starts_with("_I"));
                    assert_eq!(t[9], "ffffffff");
                    events += functions;
                    1
                } else {
                    assert_eq!(t[9], "00000001");
                    if name == "_IDummy" {
                        assert_eq!(functions, 0);
                        2
                    } else {
                        assert!(name.starts_with('I'));
                        ordinary += functions;
                        0
                    }
                }
            }
            _ => panic!("unknown type kind"),
        };
        categories[category] += 1;
        if category != 4 {
            let offset = word(&t[4]);
            assert!(offset < 1488 && offset.is_multiple_of(24));
            assert!(used_guids.insert(offset / 24));
            let g = &guids[offset as usize / 24];
            assert_eq!(t[5], g[1]);
            assert_eq!(word(&g[2]), i as u32 * 100);
            assert_eq!(g[4], "local_type_identity");
        }
    }
    assert_eq!(categories, [22, 9, 1, 23, 20]);
    assert_eq!((zero, ordinary, events, variables), (29, 317, 8, 130));
    assert_eq!(used_guids.len(), 55);
    let mut unique_guids = BTreeSet::new();
    for (i, g) in guids.iter().enumerate() {
        row(g, 6);
        assert_eq!(g[0], i);
        let s = g[1].as_str().unwrap();
        assert_eq!(s.len(), 36);
        assert!(unique_guids.insert(s));
        for (position, b) in s.bytes().enumerate() {
            if [8, 13, 18, 23].contains(&position) {
                assert_eq!(b, b'-');
            } else {
                assert!(b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            }
        }
        word(&g[2]);
        let next = word(&g[3]);
        assert!(next == u32::MAX || (next < 1488 && next.is_multiple_of(24)));
        let role = match i {
            0 => "library_identity",
            4 => "import_library_identity",
            5 => "import_interface_identity",
            1..=3 | 60 => "unclassified_metadata_identity",
            _ => "local_type_identity",
        };
        assert_eq!(g[4], role);
        assert_eq!(g[5], json!([8252 + 24 * i, 2304 + 5 * i, 2308 + 5 * i]));
        assert_eq!(
            used_guids.contains(&(i as u32)),
            role == "local_type_identity"
        );
    }
    assert_eq!(v["library"]["guid"], guids[0][1]);
    assert_eq!(v["import"]["library_guid"], guids[4][1]);
    assert_eq!(v["import"]["interface_guid"], guids[5][1]);
    reference_lists(types, refs);
}

fn reference_lists(types: &[Value], refs: &[Value]) {
    for (i, r) in refs.iter().enumerate() {
        row(r, 9);
        assert_eq!(r[0], 16 * i);
        assert_eq!(r[8], json!([9740 + 16 * i, 2615 + i]));
        row(&r[4], 3);
        let index = number(&r[4][0]) as usize;
        let t = &types[index];
        assert_eq!(word(&r[3]), index as u32 * 100);
        assert_eq!(r[4], json!([t[0], t[1], t[5]]));
        assert_eq!(t[2], "TKIND_DISPATCH");
        assert_eq!(r[6], "ffffffff");
    }
    let mut reached = BTreeSet::new();
    let mut source_targets = BTreeSet::new();
    let (mut defaults, mut sources, mut single) = (0, 0, 0);
    for owner in types.iter().filter(|t| t[2] == "TKIND_COCLASS") {
        let mut cursor = word(&owner[9]);
        let count = number(&owner[8]);
        single += usize::from(count == 1);
        for ordinal in 0..count {
            assert!(cursor < 512 && cursor.is_multiple_of(16));
            assert!(reached.insert(cursor), "shared or cyclic reference list");
            let r = &refs[cursor as usize / 16];
            assert_eq!(r[1], owner[0]);
            assert_eq!(r[2], ordinal);
            if ordinal == 0 {
                assert_eq!(r[5], 1); // IMPLTYPEFLAG_FDEFAULT
                let expected_name = if owner[1] == "_Dummy" {
                    "_IDummy".to_string()
                } else {
                    format!("I{}", owner[1].as_str().unwrap())
                };
                assert_eq!(r[4][1], expected_name);
                defaults += 1;
            } else {
                assert_eq!(r[5], 3); // FDEFAULT | FSOURCE, not event inheritance.
                assert_eq!(r[4][1], format!("_I{}Events", owner[1].as_str().unwrap()));
                assert!(source_targets.insert(number(&r[4][0])));
                sources += 1;
            }
            cursor = word(&r[7]);
        }
        assert_eq!(cursor, u32::MAX, "list did not terminate after cImplTypes");
    }
    assert_eq!((defaults, sources, single), (23, 9, 14));
    assert!(reached.iter().copied().eq((0..512).step_by(16)));
    assert_eq!(
        source_targets,
        BTreeSet::from([41, 43, 45, 48, 52, 55, 57, 59, 61])
    );
}

pub fn joins(v: &Value, coverage: &Value, enums: &Value) {
    let types = v["types"].as_array().unwrap();
    let declarations = coverage["declarations"].as_array().unwrap();
    assert_eq!(declarations.len(), 75);
    for (t, d) in types.iter().zip(declarations) {
        assert_eq!(t[0], d["type_info_index"]);
        assert_eq!(t[1], d["name"]);
        assert_eq!(t[2], d["kind"]);
        assert_eq!(t[11][0], d["function_records"]);
        assert_eq!(t[11][1], d["variable_records"]);
    }
    for (field, expected) in [
        ("type_infos", 75),
        ("ordinary_records", 317),
        ("event_records", 8),
        ("adopted_records", 325),
        ("remaining_records", 0),
        ("enum_types", 20),
        ("enum_variables", 130),
        ("zero_callable_types", 29),
    ] {
        assert_eq!(coverage["counts"][field], expected);
    }
    let mut seen = BTreeSet::new();
    let tables = enums["enumerations"].as_array().unwrap();
    assert_eq!(tables.len(), 20);
    for e in tables {
        let index = number(&e["type_info_index"]) as usize;
        assert!(seen.insert(index));
        let t = &types[index];
        assert_eq!(t[2], "TKIND_ENUM");
        assert_eq!(t[1], e["name"]);
        assert_eq!(t[11][1], e["members"].as_array().unwrap().len());
    }
}

pub fn check(v: &Value) {
    metadata::check(v);
    oracle(v);
    structure(v);
    static JOINED: OnceLock<(Value, Value)> = OnceLock::new();
    let (coverage, enums) = JOINED.get_or_init(|| {
        (
            load("com-callable-coverage.json"),
            load("com-enum-values.json"),
        )
    });
    joins(v, coverage, enums);
    source_only(v);
}
