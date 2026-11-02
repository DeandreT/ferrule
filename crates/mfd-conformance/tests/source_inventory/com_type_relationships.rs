//! Release-bound declaration wiring, never activation, ABI or runtime evidence.

#[path = "com_type_relationships/metadata.rs"]
mod metadata;
#[path = "com_type_relationships/mutations.rs"]
mod mutations;
#[path = "com_type_relationships/validate.rs"]
mod validate;

use serde_json::{Value, json};

use super::load;

const FILE: &str = "com-type-relationships.json";

#[test]
fn type_relationships_match_independent_global_dump_oracle() {
    validate::check(&load(FILE));
}

#[test]
fn all_nine_sources_include_empty_headers_and_preserve_dummy_exception() {
    let v = load(FILE);
    validate::check(&v);
    let mut empty_sources = 0;
    for (owner, source, functions) in [
        (42, 41, 0),
        (44, 43, 0),
        (46, 45, 0),
        (49, 48, 3),
        (53, 52, 2),
        (56, 55, 1),
        (58, 57, 2),
        (60, 59, 0),
        (62, 61, 0),
    ] {
        let edges: Vec<_> = v["references"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r[1] == owner)
            .collect();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0][5], 1); // FDEFAULT, not a runtime default.
        assert_eq!(edges[1][5], 3); // FDEFAULT | FSOURCE.
        assert_eq!(edges[1][4][0], source);
        let t = &v["types"][source];
        assert_eq!(t[8], 1);
        assert_eq!(t[9], "ffffffff"); // Do not invent an event-base edge.
        assert_eq!(t[11], json!([functions, 0]));
        empty_sources += usize::from(functions == 0);
    }
    assert_eq!(empty_sources, 5);
    assert_eq!(v["types"][73][1], "_IDummy");
    assert_eq!(v["types"][73][9], "00000001");
    assert_eq!(v["types"][73][11], json!([0, 0]));
    assert_eq!(v["types"][74][1], "_Dummy");
    assert_eq!(v["references"][31][4][0], 73);
    assert_eq!(v["references"][31][5], 1);
}

fn rejects(v: &Value, label: &str) {
    assert!(
        std::panic::catch_unwind(|| validate::check(v)).is_err(),
        "accepted mutation: {label}"
    );
}
