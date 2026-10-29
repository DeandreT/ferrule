use std::path::{Path, PathBuf};

use protobuf_wire_fuzz::{Outcome, Root, check, roots, seeds};

/// Expected outcome meaning the root decoded successfully. Any other value is
/// `"<path fragment>: <message fragment>"` of the expected rejection.
const DECODED: &str = "decoded";

/// The outcome each seed was written to reach, for its primary root. Other
/// roots still run through the shared invariants.
const EXPECTED: &[(&str, &str, &str)] = {
    const ROOT: &str = "fuzz.wire.Root";
    const SCALARS: &str = "fuzz.wire.Scalars";
    const PACKED: &str = "fuzz.wire.Packed";
    const NODE: &str = "fuzz.wire.Node";
    const LEGACY: &str = "fuzz.legacy.Legacy";
    &[
        ("empty", ROOT, DECODED),
        ("empty", LEGACY, "Legacy.id: required field is absent"),
        ("scalars_all", ROOT, DECODED),
        ("scalars_direct", SCALARS, DECODED),
        ("scalars_merge", ROOT, DECODED),
        ("varint_ten_bytes", SCALARS, DECODED),
        ("varint_overflow", SCALARS, "Scalars.i64: overflows u64"),
        ("varint_eleven_bytes", SCALARS, "Scalars.i64: overflows u64"),
        ("varint_truncated", SCALARS, "Scalars.i64: truncated int64"),
        ("key_truncated", ROOT, "Root: truncated field key"),
        ("uint32_overflow", SCALARS, "Scalars.u32: uint32 range"),
        (
            "uint64_out_of_range",
            SCALARS,
            "Scalars.u64: signed integer range",
        ),
        ("sint32_overflow", SCALARS, "Scalars.s32: sint32 range"),
        (
            "fixed64_out_of_range",
            SCALARS,
            "Scalars.f64: signed integer range",
        ),
        ("fixed32_truncated", SCALARS, "Scalars.f32: truncated"),
        ("double_nan", SCALARS, "Scalars.real64: finite"),
        ("float_infinity", SCALARS, "Scalars.real32: finite"),
        ("string_invalid_utf8", SCALARS, "Scalars.text: UTF-8"),
        (
            "wire_type_mismatch",
            SCALARS,
            "Scalars.i32: expected wire type 0",
        ),
        ("packed_all", ROOT, DECODED),
        ("packed_direct", PACKED, DECODED),
        ("packed_truncated_fixed", PACKED, "Packed.f32[0]: truncated"),
        (
            "packed_truncated_varint",
            PACKED,
            "Packed.i32[0]: truncated int32",
        ),
        (
            "packed_bad_enum",
            PACKED,
            "Packed.colors[0]: no declared value",
        ),
        ("texts_empty_string", PACKED, DECODED),
        ("length_exact", ROOT, DECODED),
        ("length_short_by_one", ROOT, "Root.name: truncated"),
        (
            "length_truncated_message",
            ROOT,
            "Root.leaves[0]: truncated",
        ),
        (
            "length_u64_max",
            ROOT,
            "Root.name: overflows the input offset",
        ),
        ("length_i64_max", ROOT, "Root.name: truncated"),
        ("length_u32_max", ROOT, "Root.name: truncated"),
        ("length_missing", ROOT, "Root.name: truncated length"),
        ("leaves_repeated", ROOT, DECODED),
        ("oneof_last_wins", ROOT, DECODED),
        ("oneof_back_to_name", ROOT, DECODED),
        ("map_entries", ROOT, DECODED),
        (
            "map_bad_enum",
            ROOT,
            "Root.toggles[0].value: no declared value",
        ),
        ("map_truncated_entry", ROOT, "Root.counts[0]: truncated"),
        ("unknown_fields", ROOT, DECODED),
        ("unknown_group_depth_ok", ROOT, DECODED),
        (
            "unknown_group_too_deep",
            ROOT,
            "Root: group nesting exceeds",
        ),
        ("unknown_group_mismatched_end", ROOT, "Root: does not match"),
        ("unknown_group_unterminated", ROOT, "Root: truncated group"),
        ("stray_end_group", ROOT, "Root: unexpected end-group"),
        ("wire_type_six", ROOT, "Root: unsupported wire type 6"),
        ("wire_type_seven", ROOT, "Root: unsupported wire type 7"),
        ("field_number_zero", ROOT, "Root: field number 0"),
        ("field_number_max", ROOT, DECODED),
        (
            "field_number_too_large",
            ROOT,
            "Root: field number 536870912",
        ),
        (
            "field_key_beyond_u32",
            ROOT,
            "Root: outside the protobuf range",
        ),
        ("tree_depth_limit", ROOT, DECODED),
        (
            "tree_depth_exceeded",
            ROOT,
            "Root.tree.child: nesting exceeds",
        ),
        ("node_depth_limit", NODE, DECODED),
        ("node_depth_exceeded", NODE, "Node.child: nesting exceeds"),
        ("node_children", NODE, DECODED),
        ("legacy_minimal", LEGACY, DECODED),
        ("legacy_full", LEGACY, DECODED),
        ("legacy_bad_enum", LEGACY, "Legacy.kind: no declared value"),
        (
            "legacy_nested_missing_required",
            LEGACY,
            "Legacy.next.id: required",
        ),
        ("legacy_bytes_invalid_utf8", LEGACY, "Legacy.raw: UTF-8"),
    ]
};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn outcome_for<'a>(roots: &[Root], outcomes: &'a [Outcome], root: &str) -> &'a Outcome {
    let index = roots
        .iter()
        .position(|candidate| candidate.name == root)
        .unwrap_or_else(|| panic!("unknown root {root}"));
    &outcomes[index]
}

#[test]
fn every_seed_reaches_its_intended_outcome() {
    let roots = roots();
    let seeds = seeds::all();
    for (name, _, _) in EXPECTED {
        assert!(
            seeds.iter().any(|(seed, _)| seed == name),
            "expectation for unknown seed {name}"
        );
    }
    for (name, bytes) in &seeds {
        let outcomes = check(&roots, bytes);
        let mut covered = false;
        for (_, root, expect) in EXPECTED.iter().filter(|(seed, _, _)| seed == name) {
            covered = true;
            let outcome = outcome_for(&roots, &outcomes, root);
            let matches = match outcome {
                Outcome::Decoded { .. } => *expect == DECODED,
                Outcome::Rejected { path, message } => {
                    expect
                        .split_once(": ")
                        .is_some_and(|(want_path, want_message)| {
                            path.contains(want_path) && message.contains(want_message)
                        })
                }
            };
            assert!(
                matches,
                "{name} on {root}: expected {expect}, got {outcome:?}"
            );
        }
        assert!(covered, "seed {name} has no expected outcome");
    }
}

/// Every prefix of every seed exercises truncation at each byte boundary.
#[test]
fn every_seed_prefix_satisfies_the_invariants() {
    let roots = roots();
    for (_, bytes) in seeds::all() {
        for end in 0..bytes.len() {
            check(&roots, &bytes[..end]);
        }
    }
}

#[test]
fn checked_in_seeds_match_the_generator() {
    let dir = manifest_dir().join("seeds");
    let mut expected: Vec<(String, Vec<u8>)> = seeds::all()
        .into_iter()
        .map(|(name, bytes)| (format!("{name}.bin"), bytes))
        .collect();
    expected.sort();
    let mut actual: Vec<(String, Vec<u8>)> = read_dir(&dir)
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_default();
            let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
            (name, bytes)
        })
        .collect();
    actual.sort();
    assert!(
        actual == expected,
        "seeds/ is stale; run `cargo run --example write_seeds`"
    );
}

/// Minimized reproductions saved under `regressions/` must keep passing.
#[test]
fn regressions_satisfy_the_invariants() {
    let roots = roots();
    for path in read_dir(&manifest_dir().join("regressions")) {
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        check(&roots, &bytes);
    }
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
}
