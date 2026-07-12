//! Support-matrix survey over the local (gitignored) MapForce sample set.
//!
//! Run with `cargo test -p mfd --test samples_survey -- --ignored --nocapture`.
//! Skips silently when the samples directory isn't present, so CI is
//! unaffected; the numbers it prints are for judging import-coverage deltas
//! while developing, not for asserting.

use std::collections::BTreeMap;
use std::path::Path;

const SAMPLES_DIR: &str = "../../samples/ReferenceSamples";

/// Collapses a warning to a stable category so the histogram groups
/// per-component messages together.
fn warning_category(w: &str) -> String {
    // Replace `quoted` spans with `_` so messages differing only in the
    // component/path/function name land in one bucket.
    let mut out = String::new();
    let mut in_quote = false;
    for c in w.chars() {
        if c == '`' {
            if !in_quote {
                out.push_str("`_");
            } else {
                out.push('`');
            }
            in_quote = !in_quote;
        } else if !in_quote {
            out.push(c);
        }
    }
    out
}

#[test]
fn warning_categories_replace_quoted_values_once() {
    assert_eq!(
        warning_category("binding for `Person/Name` comes from `source`"),
        "binding for `_` comes from `_`"
    );
}

#[test]
#[ignore = "needs the local MapForce sample set; informational only"]
fn survey_samples() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLES_DIR);
    if !dir.is_dir() {
        eprintln!("samples dir not found at {}; skipping", dir.display());
        return;
    }

    let mut mfds: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mfd")))
        .collect();
    mfds.sort();

    let mut ok = 0usize;
    let mut ok_clean = 0usize;
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut warning_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut warnings_by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in &mfds {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match mfd::import(path) {
            Ok(imported) => {
                ok += 1;
                if imported.warnings.is_empty() {
                    ok_clean += 1;
                }
                for w in &imported.warnings {
                    *warning_counts.entry(warning_category(w)).or_default() += 1;
                    warnings_by_file
                        .entry(name.clone())
                        .or_default()
                        .push(w.clone());
                }
            }
            Err(err) => {
                errors
                    .entry(warning_category(&err.to_string()))
                    .or_default()
                    .push(name);
            }
        }
    }

    println!("== mfd import survey: {} files ==", mfds.len());
    println!("imported: {ok} ({ok_clean} with zero warnings)");
    println!("rejected: {}", mfds.len() - ok);
    println!("\n-- rejection reasons --");
    for (reason, files) in &errors {
        println!("{:4}  {reason}", files.len());
        for f in files.iter().take(3) {
            println!("        e.g. {f}");
        }
    }
    println!("\n-- warning categories (imported files) --");
    let mut warnings: Vec<_> = warning_counts.into_iter().collect();
    warnings.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (cat, n) in warnings {
        println!("{n:4}  {cat}");
    }
    if std::env::var_os("FERRULE_SURVEY_DETAILS").is_some() {
        println!("\n-- warnings by file --");
        for (file, warnings) in warnings_by_file {
            println!("{file}");
            for warning in warnings {
                println!("    {warning}");
            }
        }
    }
}
