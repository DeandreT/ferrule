use std::path::{Path, PathBuf};

use flextext_layout_fuzz::program::{
    Encoder, MAX_PROGRAM_STRING_BYTES, MAX_WIDE_ARMS, MAX_WIDE_REGEX_ARMS, Program, Str, op, wide,
};
use flextext_layout_fuzz::{MAX_LAYOUT_JSON_DEPTH, MAX_SWITCH_REGEXES, Outcome, check, seeds};
use mapping::{FlexCommand, FlexTextLayoutError};

/// The layout validated.
const VALID: &str = "valid";
/// The input read successfully.
const PARSED: &str = "parsed";
/// The parsed instance wrote back successfully.
const WRITTEN: &str = "written";
/// The stage did not run because an earlier stage rejected the input.
const NONE: &str = "";

/// `(seed, layout, input, render)`: each stage's expected outcome, or a
/// fragment of its expected error message.
const EXPECTED: &[(&str, &str, &str, &str)] = &[
    ("empty", VALID, PARSED, WRITTEN),
    ("store_trim_unicode", VALID, PARSED, WRITTEN),
    ("store_trim_left", VALID, PARSED, WRITTEN),
    ("store_int", VALID, PARSED, WRITTEN),
    ("store_int_invalid", VALID, "`4 2` is not a valid Int", NONE),
    ("store_float_nan", VALID, "`NaN` is not a valid Float", NONE),
    ("store_float", VALID, PARSED, WRITTEN),
    ("store_bool", VALID, PARSED, WRITTEN),
    ("store_bom_crlf", VALID, PARSED, WRITTEN),
    (
        "store_carriage_return_before_line_feed",
        VALID,
        PARSED,
        WRITTEN,
    ),
    (
        "split_once_carriage_return_delimiter",
        VALID,
        PARSED,
        WRITTEN,
    ),
    ("split_once_delimiter_unicode", VALID, PARSED, WRITTEN),
    ("split_once_columns_unicode", VALID, PARSED, WRITTEN),
    ("split_once_columns_short", VALID, PARSED, WRITTEN),
    ("split_once_lines_crlf", VALID, PARSED, WRITTEN),
    ("split_once_line_starting", VALID, PARSED, WRITTEN),
    ("split_once_line_containing", VALID, PARSED, WRITTEN),
    ("split_many_delimiter_trailing", VALID, PARSED, WRITTEN),
    (
        "split_many_lines",
        VALID,
        PARSED,
        "split item expected 2 line(s), got 1",
    ),
    ("split_many_lines_starting", VALID, PARSED, WRITTEN),
    ("split_many_bom_delimiter", VALID, PARSED, WRITTEN),
    ("split_many_empty_input", VALID, PARSED, WRITTEN),
    ("fixed_width_delimited", VALID, PARSED, WRITTEN),
    ("fixed_width_undelimited", VALID, PARSED, WRITTEN),
    (
        "fixed_width_not_multiple",
        VALID,
        "not a multiple of record width 4",
        NONE,
    ),
    ("fixed_width_short_final_string", VALID, PARSED, WRITTEN),
    (
        "fixed_width_empty_int_present",
        VALID,
        "`` is not a valid Int",
        NONE,
    ),
    ("fixed_width_unicode_fill", VALID, PARSED, WRITTEN),
    (
        "fixed_width_huge_field",
        VALID,
        PARSED,
        "output exceeds the 8388608-byte limit",
    ),
    (
        "fixed_columns_huge",
        VALID,
        PARSED,
        "output exceeds the 8388608-byte limit",
    ),
    ("delimited_quoted", VALID, PARSED, WRITTEN),
    ("delimited_backslash_escape", VALID, PARSED, WRITTEN),
    (
        "delimited_trailing_escape",
        VALID,
        "unterminated quoted field",
        NONE,
    ),
    (
        "delimited_unterminated",
        VALID,
        "unterminated quoted field",
        NONE,
    ),
    (
        "delimited_field_count",
        VALID,
        "expected 2 field(s), got 1",
        NONE,
    ),
    ("delimited_multichar_separators", VALID, PARSED, WRITTEN),
    ("switch_first_match", VALID, PARSED, WRITTEN),
    ("switch_all_possible", VALID, PARSED, WRITTEN),
    ("switch_default", VALID, PARSED, WRITTEN),
    ("switch_matched_ignore", VALID, PARSED, "missing field `a`"),
    ("switch_regex_unicode", VALID, PARSED, WRITTEN),
    ("nested_records", VALID, PARSED, WRITTEN),
    ("chain_split_once_depth_limit", VALID, PARSED, WRITTEN),
    (
        "chain_split_once_too_deep",
        "command nesting exceeds",
        NONE,
        NONE,
    ),
    (
        "chain_split_many_deep",
        VALID,
        PARSED,
        "instance nesting exceeds",
    ),
    ("chain_switch_default_depth_limit", VALID, PARSED, WRITTEN),
    ("chain_switch_arm_json_limit", VALID, PARSED, WRITTEN),
    ("chain_switch_arm_at_json_limit", VALID, PARSED, WRITTEN),
    (
        "chain_switch_arm_past_json_limit",
        "layout JSON nesting exceeds the limit of 96",
        NONE,
        NONE,
    ),
    (
        "chain_switch_arm_depth_limit",
        "layout JSON nesting exceeds the limit of 96",
        NONE,
        NONE,
    ),
    (
        "chain_switch_arm_too_deep",
        "command nesting exceeds",
        NONE,
        NONE,
    ),
    ("wide_switch_node_limit", VALID, PARSED, WRITTEN),
    (
        "wide_switch_too_many_nodes",
        "command tree exceeds the limit of 4096 nodes",
        NONE,
        NONE,
    ),
    ("wide_regex_limit", VALID, PARSED, WRITTEN),
    (
        "wide_regex_too_many",
        "limit of 64 distinct switch regular expressions",
        NONE,
        NONE,
    ),
    (
        "too_many_nodes",
        VALID,
        "instance exceeds the limit of 1000000 nodes",
        NONE,
    ),
    (
        "too_many_records",
        VALID,
        "record count exceeds the limit of 1000000",
        NONE,
    ),
    ("empty_root_name", "root name must not be empty", NONE, NONE),
    (
        "empty_command_name",
        "command name must not be empty",
        NONE,
        NONE,
    ),
    (
        "empty_once_delimiter",
        "delimiter must not be empty",
        NONE,
        NONE,
    ),
    (
        "empty_many_delimiter",
        "record delimiter must not be empty",
        NONE,
        NONE,
    ),
    (
        "duplicate_split_names",
        "duplicate sibling name `b`",
        NONE,
        NONE,
    ),
    (
        "duplicate_switch_prefix",
        "duplicate switch condition `#`",
        NONE,
        NONE,
    ),
    (
        "empty_switch",
        "switch must contain an arm or a default",
        NONE,
        NONE,
    ),
    ("switch_without_output", "no viable output", NONE, NONE),
    ("split_once_without_output", "no viable output", NONE, NONE),
    ("split_many_without_output", "no viable output", NONE, NONE),
    (
        "empty_record",
        "record must contain at least one field",
        NONE,
        NONE,
    ),
    (
        "duplicate_record_fields",
        "duplicate sibling name `b`",
        NONE,
        NONE,
    ),
    (
        "dialect_line_break_quote",
        "must be non-NUL non-line characters",
        NONE,
        NONE,
    ),
    (
        "dialect_quote_in_separator",
        "field separator must differ from the quote",
        NONE,
        NONE,
    ),
    (
        "dialect_empty_record_separator",
        "record separator must not be empty",
        NONE,
        NONE,
    ),
    (
        "fill_line_break",
        "fill character must not be a line break",
        NONE,
        NONE,
    ),
    (
        "duplicate_trim_character",
        "trim characters must form a set",
        NONE,
        NONE,
    ),
    (
        "empty_trim_characters",
        "trim characters must not be empty",
        NONE,
        NONE,
    ),
    (
        "invalid_regex",
        "switch regular expression is invalid",
        NONE,
        NONE,
    ),
    (
        "regex_too_large",
        "switch regular expression is invalid",
        NONE,
        NONE,
    ),
    (
        "regex_empty",
        "switch regular expression must not be empty",
        NONE,
        NONE,
    ),
    (
        "regex_pattern_over_limit",
        "switch regular expression exceeds",
        NONE,
        NONE,
    ),
    ("name_over_limit", "command exceeds", NONE, NONE),
    (
        "name_at_limit",
        "layout strings exceed the 1048576-byte total limit",
        NONE,
        NONE,
    ),
    (
        "string_budget_fallback",
        "layout strings exceed the 1048576-byte total limit",
        NONE,
        NONE,
    ),
    (
        "delimiter_at_limit",
        "layout strings exceed the 1048576-byte total limit",
        NONE,
        NONE,
    ),
    ("json_store", VALID, PARSED, WRITTEN),
    ("json_records", VALID, PARSED, WRITTEN),
    (
        "json_empty_delimiter",
        "delimiter must not be empty",
        NONE,
        NONE,
    ),
    ("json_zero_width", "expected a nonzero u32", NONE, NONE),
    ("json_unknown_kind", "unknown variant `loop`", NONE, NONE),
    ("json_truncated", "EOF while parsing", NONE, NONE),
    ("json_deep_nesting", "recursion limit exceeded", NONE, NONE),
];

/// Rejected program layouts whose JSON form nests past serde_json's limit, so
/// serde refuses it before the constructor runs. Every other rejected
/// program is rejected through JSON with the constructor's message.
const REFUSED_BY_SERDE: &[&str] = &["chain_switch_arm_depth_limit", "chain_switch_arm_too_deep"];

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn stage<T>(result: Option<&Result<T, String>>, success: &str) -> String {
    match result {
        None => NONE.to_string(),
        Some(Ok(_)) => success.to_string(),
        Some(Err(message)) => message.clone(),
    }
}

fn matches(actual: &str, expected: &str) -> bool {
    if [VALID, PARSED, WRITTEN, NONE].contains(&expected) {
        actual == expected
    } else {
        ![VALID, PARSED, WRITTEN, NONE].contains(&actual) && actual.contains(expected)
    }
}

fn describe(outcome: &Outcome) -> [String; 3] {
    let layout = match &outcome.layout {
        Ok(()) => VALID.to_string(),
        Err(message) => message.clone(),
    };
    [
        layout,
        stage(outcome.input.as_ref(), PARSED),
        stage(outcome.render.as_ref(), WRITTEN),
    ]
}

#[test]
fn every_seed_reaches_its_intended_outcome() {
    let seeds = seeds::all();
    for (name, ..) in EXPECTED {
        assert!(
            seeds.iter().any(|(seed, _)| seed == name),
            "expectation for unknown seed {name}"
        );
    }
    for (name, bytes) in &seeds {
        let Some((_, layout, input, render)) = EXPECTED.iter().find(|(seed, ..)| seed == name)
        else {
            panic!("seed {name} has no expected outcome");
        };
        let outcome = check(bytes);
        let actual = describe(&outcome);
        let expected = [*layout, *input, *render];
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| matches(actual, expected)),
            "{name}: expected {expected:?}, got {actual:?}"
        );
        assert_eq!(
            outcome.json_refused_by_serde,
            REFUSED_BY_SERDE.contains(name),
            "{name}: serde_json depth refusal"
        );
    }
}

/// Every prefix of every short seed exercises truncation of the layout
/// program, its operands and the input text at each byte boundary.
#[test]
fn every_short_seed_prefix_satisfies_the_invariants() {
    for (_, bytes) in seeds::all() {
        if bytes.len() > 512 {
            continue;
        }
        for end in 0..bytes.len() {
            check(&bytes[..end]);
        }
    }
}

/// The harness's copy of the distinct-regex limit must match the limit the
/// validator reports.
#[test]
fn switch_regex_limit_matches_the_validator() {
    let message = FlexTextLayoutError::TooManySwitchRegexes.to_string();
    assert!(
        message.contains(&format!("limit of {MAX_SWITCH_REGEXES} distinct")),
        "validator reports `{message}`"
    );
}

/// The harness's copy of the serialized-depth limit must match the limit the
/// validator reports.
#[test]
fn layout_json_depth_limit_matches_the_validator() {
    let message = FlexTextLayoutError::SerializedLayoutTooDeep.to_string();
    assert!(
        message.contains(&format!("limit of {MAX_LAYOUT_JSON_DEPTH}")),
        "validator reports `{message}`"
    );
}

/// A chain of `SplitOnce` commands on one column whose first children are
/// wide switches; the last second child is a store.
fn wide_switches(count: usize, arms: u16, kind: u8) -> Program {
    let mut encoder = Encoder::new(false, false, "a");
    for _ in 0..count {
        encoder.op(op::SPLIT_ONCE).name("a").byte(1).count(1);
        encoder
            .op(op::WIDE_SWITCH)
            .name("b")
            .byte(0)
            .u16(arms)
            .byte(kind);
    }
    encoder.op(op::STORE).name("c").byte(0).byte(0);
    Program::decode(&encoder.finish(""))
}

fn arm_count(command: &FlexCommand) -> usize {
    match command {
        FlexCommand::SplitOnce { first, second, .. } => arm_count(first) + arm_count(second),
        FlexCommand::SplitMany { child, .. } => arm_count(child),
        FlexCommand::Switch { arms, default, .. } => {
            arms.len()
                + arms
                    .iter()
                    .map(|arm| arm_count(arm.command()))
                    .sum::<usize>()
                + default.as_deref().map_or(0, arm_count)
        }
        _ => 0,
    }
}

/// Generated wide-switch arms are capped per program, not per switch.
#[test]
fn wide_switch_arms_are_capped_per_program() {
    for (kind, cap) in [
        (wide::LITERAL, MAX_WIDE_ARMS),
        (wide::REGEX, MAX_WIDE_REGEX_ARMS),
    ] {
        let program = wide_switches(4, 4_000, kind);
        let Ok(command) = &program.command else {
            panic!("wide switches decode: {:?}", program.command);
        };
        assert_eq!(arm_count(command), cap, "wide switch kind {kind}");
    }
}

/// Decoded string operands stay within the per-program budget: once an
/// at- or over-limit operand is spent, further ones fall back to the pool.
#[test]
fn string_operands_stay_within_the_program_budget() {
    let mut encoder = Encoder::new(false, false, "a");
    for _ in 0..8 {
        encoder
            .op(op::SPLIT_ONCE)
            .name_str(Str::OverLimit)
            .byte(2)
            .text_str(Str::AtLimit);
        encoder.op(op::IGNORE);
    }
    encoder.op(op::STORE).name_str(Str::AtLimit).byte(0).byte(0);
    let program = Program::decode(&encoder.finish(""));
    let Ok(command) = &program.command else {
        panic!("program decodes: {:?}", program.command);
    };
    let bytes = program.root_name.len() + string_bytes(command);
    assert!(
        bytes <= MAX_PROGRAM_STRING_BYTES,
        "{bytes} decoded string bytes exceed {MAX_PROGRAM_STRING_BYTES}"
    );
}

fn string_bytes(command: &FlexCommand) -> usize {
    match command {
        FlexCommand::SplitOnce {
            name,
            splitter,
            first,
            second,
        } => {
            let splitter = match splitter {
                mapping::OnceSplitter::Delimiter(value)
                | mapping::OnceSplitter::LineStartingWith(value)
                | mapping::OnceSplitter::LineContaining(value) => value.len(),
                _ => 0,
            };
            name.len() + splitter + string_bytes(first) + string_bytes(second)
        }
        FlexCommand::Store { name, .. } => name.len(),
        _ => 0,
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
    for path in read_dir(&manifest_dir().join("regressions")) {
        let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        check(&bytes);
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
