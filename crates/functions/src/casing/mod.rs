//! Portable full Unicode casing for the `upper` and `lower` builtins.
//!
//! Mappings are pinned to the Unicode 17.0.0 data in `tables.rs`, which the C#
//! runtime shares row by row, so results never follow the host toolchain's
//! Unicode version. The contract is documented in `docs/unicode-casing.md`.

mod tables;
#[cfg(test)]
mod tests;

use tables::{CASE_IGNORABLE, CASED, LOWER_EXPANSIONS, LOWER_RUNS, UPPER_EXPANSIONS, UPPER_RUNS};

#[cfg(test)]
const UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0);
const CAPITAL_SIGMA: char = '\u{03A3}';
const FINAL_SIGMA: char = '\u{03C2}';

type Run = (u32, u32, u32, i32);
type Expansion = (u32, u32, u32, u32);

/// Language-neutral full uppercase mapping of every scalar.
pub(crate) fn to_upper(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for scalar in value.chars() {
        if scalar.is_ascii() {
            output.push(scalar.to_ascii_uppercase());
        } else {
            push_mapping(&mut output, scalar, UPPER_RUNS, UPPER_EXPANSIONS);
        }
    }
    output
}

/// Language-neutral full lowercase mapping with the Final_Sigma context.
pub(crate) fn to_lower(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for (index, scalar) in value.char_indices() {
        if scalar.is_ascii() {
            output.push(scalar.to_ascii_lowercase());
        } else if scalar == CAPITAL_SIGMA && is_final_sigma(value, index) {
            output.push(FINAL_SIGMA);
        } else {
            push_mapping(&mut output, scalar, LOWER_RUNS, LOWER_EXPANSIONS);
        }
    }
    output
}

// Unicode Final_Sigma: a cased scalar precedes the sigma and none follows it,
// skipping case-ignorable scalars in the original text on both sides. A scalar
// that is both cased and case-ignorable is skipped, as in Rust's `str` casing.
fn is_final_sigma(value: &str, index: usize) -> bool {
    let after = index + CAPITAL_SIGMA.len_utf8();
    cased_after_ignorable(value[..index].chars().rev())
        && !cased_after_ignorable(value[after..].chars())
}

fn cased_after_ignorable(mut scalars: impl Iterator<Item = char>) -> bool {
    scalars
        .find(|&scalar| !in_ranges(scalar, CASE_IGNORABLE))
        .is_some_and(|scalar| in_ranges(scalar, CASED))
}

fn in_ranges(scalar: char, ranges: &[(u32, u32)]) -> bool {
    let code = u32::from(scalar);
    let index = ranges.partition_point(|&(first, _)| first <= code);
    index
        .checked_sub(1)
        .is_some_and(|index| code <= ranges[index].1)
}

fn push_mapping(output: &mut String, scalar: char, runs: &[Run], expansions: &[Expansion]) {
    let code = u32::from(scalar);
    if let Ok(index) = expansions.binary_search_by_key(&code, |&(source, ..)| source) {
        let (_, first, second, third) = expansions[index];
        // Zero pads shorter expansions; U+0000 never occurs in a mapping.
        output.extend(
            [first, second, third]
                .into_iter()
                .filter(|&mapped| mapped != 0)
                .filter_map(char::from_u32),
        );
        return;
    }
    output.push(run_mapping(code, runs).unwrap_or(scalar));
}

fn run_mapping(code: u32, runs: &[Run]) -> Option<char> {
    let index = runs
        .partition_point(|&(first, ..)| first <= code)
        .checked_sub(1)?;
    let (first, last, stride, delta) = runs[index];
    if code > last || !(code - first).is_multiple_of(stride) {
        return None;
    }
    char::from_u32(code.checked_add_signed(delta)?)
}
