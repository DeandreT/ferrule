use super::tables::{
    CASE_IGNORABLE, CASED, LOWER_EXPANSIONS, LOWER_RUNS, UPPER_EXPANSIONS, UPPER_RUNS,
};
use super::{UNICODE_VERSION, to_lower, to_upper};

// Independently authored expectations, mirrored by the C# smoke tests.
const UPPER: &[(&str, &str)] = &[
    ("", ""),
    ("ascii 1-z", "ASCII 1-Z"),
    ("straße", "STRASSE"),
    ("ẞ", "ẞ"),
    ("ﬁ ﬀ ﬃ", "FI FF FFI"),
    ("ŉ", "\u{2BC}N"),
    ("ǰ", "J\u{30C}"),
    ("ΐ", "\u{399}\u{308}\u{301}"),
    ("ᾳ ᾼ", "ΑΙ ΑΙ"),
    ("ǅ ǆ Ǆ", "Ǆ Ǆ Ǆ"),
    ("ı i İ", "I I İ"),
    ("ſ µ ÿ \u{345}", "S Μ Ÿ Ι"),
    ("σς", "ΣΣ"),
    ("𐐨𞤢𐓘", "𐐀𞤀𐒰"),
    ("\u{16EBB}\u{A7CF}", "\u{16EA0}\u{A7CE}"),
    ("漢🙂\u{301}", "漢🙂\u{301}"),
];

const LOWER: &[(&str, &str)] = &[
    ("", ""),
    ("ASCII 1-Z", "ascii 1-z"),
    ("I İ", "i i\u{307}"),
    ("ẞ SS", "ß ss"),
    ("ǅ Ǆ", "ǆ ǆ"),
    ("\u{212A}\u{2126}\u{212B}", "kωå"),
    ("𐐀𞤀𐒰", "𐐨𞤢𐓘"),
    ("\u{16EA0}\u{A7CE}", "\u{16EBB}\u{A7CF}"),
    ("Σ", "σ"),
    ("ΣΑ", "σα"),
    ("ΟΔΟΣ", "οδος"),
    ("ΟΔΟΣ ΟΔΟΣ.", "οδος οδος."),
    ("ΑΣΣ", "ασς"),
    ("AΣB", "aσb"),
    ("AΣ'", "aς'"),
    ("A'Σ", "a'ς"),
    ("AΣ'B", "aσ'b"),
    ("AΣ\u{301}", "aς\u{301}"),
    ("1Σ", "1σ"),
    ("漢Σ", "漢σ"),
    ("𐐀Σ", "𐐨ς"),
    ("ΑΣ𐐀", "ασ𐐨"),
    ("ʰΣ", "ʰσ"),
    ("AʰΣ", "aʰς"),
    ("ΑΣʰ", "αςʰ"),
    ("漢🙂", "漢🙂"),
];

#[test]
fn literal_vectors_cover_expansions_context_and_supplementary_scalars() {
    for &(input, expected) in UPPER {
        assert_eq!(to_upper(input), expected, "upper({input:?})");
    }
    for &(input, expected) in LOWER {
        assert_eq!(to_lower(input), expected, "lower({input:?})");
    }
}

#[test]
fn builtins_use_the_portable_mapping() {
    use ir::Value;
    assert_eq!(
        crate::call("upper", &[Value::String("straße 𐐨".into())]),
        Ok(Value::String("STRASSE 𐐀".into()))
    );
    assert_eq!(
        crate::call("lower", &[Value::String("ΟΔΟΣ İ".into())]),
        Ok(Value::String("οδος i\u{307}".into()))
    );
}

// The tables, not the toolchain, define the contract. When the toolchain's
// Unicode version equals the pin, its `str` casing is an independent check of
// every scalar and of the Final_Sigma context classes.
#[test]
fn every_scalar_matches_rust_str_casing_at_the_pinned_version() {
    if char::UNICODE_VERSION != UNICODE_VERSION {
        eprintln!(
            "skipping std comparison: toolchain Unicode {:?}",
            char::UNICODE_VERSION
        );
        return;
    }
    let mut text = String::new();
    for scalar in (0..=0x10FFFF).filter_map(char::from_u32) {
        for context in [
            format!("{scalar}"),
            format!("AΣ{scalar}"),
            format!("{scalar}Σ"),
            format!("A{scalar}Σ"),
            format!("AΣ{scalar}B"),
        ] {
            assert_eq!(to_lower(&context), context.to_lowercase(), "{context:?}");
        }
        text.clear();
        text.push(scalar);
        assert_eq!(to_upper(&text), text.to_uppercase(), "{text:?}");
    }
}

#[test]
fn csharp_leaf_retains_the_rust_rows() {
    let rust = include_str!("tables.rs");
    let csharp =
        include_str!("../../../../runtime/csharp/Ferrule.Runtime/FerruleUnicodeCasing.Tables.cs");
    let rows = |source: &'static str, prefix: &'static str| {
        source
            .lines()
            .filter_map(move |line| line.trim().strip_prefix(prefix))
            .filter(|row| row.starts_with("(0x"))
            .collect::<Vec<_>>()
    };
    let header = |source: &'static str| source.lines().nth(1);
    assert_eq!(header(rust), header(csharp));
    assert!(header(rust).is_some_and(|line| line.starts_with("// Unicode 17.0.0;")));
    let rust_rows = rows(rust, "");
    assert_eq!(rust_rows, rows(csharp, "new"));
    let total = UPPER_RUNS.len()
        + UPPER_EXPANSIONS.len()
        + LOWER_RUNS.len()
        + LOWER_EXPANSIONS.len()
        + CASED.len()
        + CASE_IGNORABLE.len();
    assert_eq!(rust_rows.len(), total);
}

#[test]
fn rust_leaf_matches_the_canonical_records() {
    let mut upper_runs = Vec::new();
    let mut lower_runs = Vec::new();
    let mut upper_expansions = Vec::new();
    let mut lower_expansions = Vec::new();
    let mut cased = Vec::new();
    let mut ignorable = Vec::new();
    let hex = |text: &str| u32::from_str_radix(text, 16).expect("canonical hex scalar");
    for line in include_str!("casing_tables.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields = line.split(' ').collect::<Vec<_>>();
        if let [kind, range] = fields[..] {
            let (first, last) = range.split_once("..").expect("canonical class range");
            let table = if kind == "cased" {
                &mut cased
            } else {
                &mut ignorable
            };
            table.push((hex(first), hex(last)));
        } else if let [direction, range, stride, delta] = fields[..]
            && let Some((first, last)) = range.split_once("..")
        {
            let run = (
                hex(first),
                hex(last),
                stride.parse().expect("canonical stride"),
                delta.parse().expect("canonical delta"),
            );
            let table = if direction == "upper" {
                &mut upper_runs
            } else {
                &mut lower_runs
            };
            table.push(run);
        } else {
            let mut padded = fields[2..].iter().map(|field| hex(field)).chain([0; 2]);
            let mut next = || padded.next().expect("padded expansion");
            let expansion = (hex(fields[1]), next(), next(), next());
            let table = if fields[0] == "upper" {
                &mut upper_expansions
            } else {
                &mut lower_expansions
            };
            table.push(expansion);
        }
    }
    assert_eq!(upper_runs, UPPER_RUNS);
    assert_eq!(upper_expansions, UPPER_EXPANSIONS);
    assert_eq!(lower_runs, LOWER_RUNS);
    assert_eq!(lower_expansions, LOWER_EXPANSIONS);
    assert_eq!(cased, CASED);
    assert_eq!(ignorable, CASE_IGNORABLE);
}
