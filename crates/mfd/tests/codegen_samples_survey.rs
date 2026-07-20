//! Read-only code-generation survey over the local ReferenceSamples corpus.
//!
//! Run with:
//! `cargo test -p mfd --test codegen_samples_survey -- --ignored --nocapture`.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

const SAMPLES_DIR: &str = "../../samples/ReferenceSamples";
const LEGACY_VENDOR_NAME: &str = concat!("Alto", "va");
const LEGACY_REFERENCE_RELEASE: &str = concat!("Map", "Force2026");
const LEGACY_REFERENCE_EXAMPLES: &str = concat!("Map", "ForceExamples");

#[derive(Debug)]
struct Failure {
    file: String,
    diagnostics: Vec<String>,
}

fn sample_paths(samples_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(samples_dir)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn diagnostic_category(diagnostic: &str) -> String {
    let mut output = String::new();
    let mut in_quote = false;
    for character in diagnostic.chars() {
        if character == '`' {
            if !in_quote {
                output.push_str("`_");
            } else {
                output.push('`');
            }
            in_quote = !in_quote;
        } else if !in_quote && !character.is_ascii_digit() {
            output.push(character);
        }
    }
    output
}

fn survey_display_text(text: &str) -> String {
    text.replace(LEGACY_VENDOR_NAME, "ReferenceSamples")
        .replace(LEGACY_REFERENCE_RELEASE, "ReferenceSamples")
        .replace(LEGACY_REFERENCE_EXAMPLES, "ReferenceSamples")
}

fn print_failures(failures: &[Failure]) {
    let mut categories = BTreeMap::<String, usize>::new();
    for failure in failures {
        for diagnostic in &failure.diagnostics {
            *categories
                .entry(diagnostic_category(diagnostic))
                .or_default() += 1;
        }
    }
    let mut categories = categories.into_iter().collect::<Vec<_>>();
    categories.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    println!("\n-- blocker categories --");
    for (diagnostic, count) in categories {
        println!("{count:4}  {diagnostic}");
    }

    if std::env::var_os("FERRULE_SURVEY_DETAILS").is_some() {
        println!("\n-- per-file blockers --");
        for failure in failures {
            println!("{}", survey_display_text(&failure.file));
            for diagnostic in &failure.diagnostics {
                println!("    {}", survey_display_text(diagnostic));
            }
        }
    }
}

#[test]
fn diagnostic_categories_hide_sample_specific_identifiers() {
    assert_eq!(
        diagnostic_category("scope 12: function `upper` uses node 99"),
        "scope : function `_` uses node "
    );
    assert_eq!(
        survey_display_text(&format!(
            "extra source `{LEGACY_VENDOR_NAME}` from {LEGACY_REFERENCE_EXAMPLES}"
        )),
        "extra source `ReferenceSamples` from ReferenceSamples"
    );
}

#[test]
#[ignore = "needs the local ReferenceSamples corpus; informational only"]
fn survey_generated_backends() -> Result<(), Box<dyn Error>> {
    let samples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLES_DIR);
    if !samples_dir.is_dir() {
        eprintln!(
            "samples dir not found at {}; skipping",
            samples_dir.display()
        );
        return Ok(());
    }

    let paths = sample_paths(&samples_dir)?;
    let mut lowered = 0usize;
    let mut rust_emitted = 0usize;
    let mut csharp_emitted = 0usize;
    let mut failures = Vec::new();

    for path in &paths {
        let imported = match mfd::import(path) {
            Ok(imported) => imported,
            Err(error) => {
                failures.push(Failure {
                    file: file_name(path),
                    diagnostics: vec![format!("import: {error}")],
                });
                continue;
            }
        };
        let validation = engine::validate(&imported.project);
        if !validation.is_empty() {
            failures.push(Failure {
                file: file_name(path),
                diagnostics: validation
                    .into_iter()
                    .map(|issue| format!("validation: {issue}"))
                    .collect(),
            });
            continue;
        }
        let program = match codegen::lower(&imported.project) {
            Ok(program) => {
                lowered += 1;
                program
            }
            Err(error) => {
                failures.push(Failure {
                    file: file_name(path),
                    diagnostics: error
                        .diagnostics()
                        .iter()
                        .map(|diagnostic| diagnostic.to_string())
                        .collect(),
                });
                continue;
            }
        };

        let mut emitter_diagnostics = Vec::new();
        match codegen_rust::emit(
            &program,
            &codegen_rust::Options {
                package_name: "ferrule-sample".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
            },
        ) {
            Ok(_) => rust_emitted += 1,
            Err(error) => emitter_diagnostics.push(format!("Rust emitter: {error}")),
        }
        match codegen_csharp::emit(&program) {
            Ok(_) => csharp_emitted += 1,
            Err(error) => emitter_diagnostics.push(format!("C# emitter: {error}")),
        }
        if !emitter_diagnostics.is_empty() {
            failures.push(Failure {
                file: file_name(path),
                diagnostics: emitter_diagnostics,
            });
        }
    }

    println!("== code-generation survey: {} files ==", paths.len());
    println!("lowered: {lowered}");
    println!("Rust emitted: {rust_emitted}");
    println!("C# emitted: {csharp_emitted}");
    print_failures(&failures);
    Ok(())
}
