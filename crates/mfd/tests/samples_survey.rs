//! Compatibility survey over the local gitignored ReferenceSamples corpus.
//!
//! Run with `cargo test -p mfd --test samples_survey -- --ignored --nocapture`.
//! Set `FERRULE_SURVEY_JSON=/path/to/report.json` for a versioned machine-
//! readable report and `FERRULE_SURVEY_DETAILS=1` for per-file diagnostics.
//! The survey never executes a sample or writes inside the sample tree.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

const SAMPLES_DIR: &str = "../../samples/ReferenceSamples";
const JSON_REPORT_ENV: &str = "FERRULE_SURVEY_JSON";
const REPORT_SCHEMA_VERSION: u32 = 1;
const MAX_SAMPLE_DEPTH: usize = 32;
const MAX_SAMPLE_FILES: usize = 10_000;

/// Collapses a diagnostic to a stable category so the histogram groups
/// per-component messages together.
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
        } else if !in_quote {
            output.push(character);
        }
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StageStatus {
    Passed,
    Failed,
    Skipped,
}

impl StageStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug)]
struct StageOutcome {
    status: StageStatus,
    diagnostics: Vec<String>,
}

impl StageOutcome {
    fn passed(diagnostics: Vec<String>) -> Self {
        Self {
            status: StageStatus::Passed,
            diagnostics,
        }
    }

    fn failed(diagnostics: Vec<String>) -> Self {
        Self {
            status: StageStatus::Failed,
            diagnostics,
        }
    }

    fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: StageStatus::Skipped,
            diagnostics: vec![reason.into()],
        }
    }

    fn is_passed(&self) -> bool {
        self.status == StageStatus::Passed
    }

    fn is_clean(&self) -> bool {
        self.is_passed() && self.diagnostics.is_empty()
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status.label(),
            "diagnostics": self.diagnostics,
        })
    }
}

#[derive(Debug)]
struct SampleOutcome {
    file: String,
    import: StageOutcome,
    validation: StageOutcome,
    export: StageOutcome,
    reimport: StageOutcome,
    roundtrip_validation: StageOutcome,
    execution: StageOutcome,
    reference_match: StageOutcome,
}

impl SampleOutcome {
    fn pending(file: String) -> Self {
        let blocked = StageOutcome::skipped("an earlier compatibility stage did not complete");
        Self {
            file,
            import: blocked.clone(),
            validation: blocked.clone(),
            export: blocked.clone(),
            reimport: blocked.clone(),
            roundtrip_validation: blocked,
            execution: StageOutcome::skipped(
                "execution is not attempted by the read-only import/export survey",
            ),
            reference_match: StageOutcome::skipped(
                "reference comparison requires a safely redirected execution result",
            ),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "file": self.file,
            "stages": {
                "import": self.import.to_json(),
                "validation": self.validation.to_json(),
                "export": self.export.to_json(),
                "reimport": self.reimport.to_json(),
                "roundtrip_validation": self.roundtrip_validation.to_json(),
                "execution": self.execution.to_json(),
                "reference_match": self.reference_match.to_json(),
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SurveySummary {
    total: usize,
    imported: usize,
    import_clean: usize,
    valid: usize,
    exported: usize,
    export_clean: usize,
    reimported: usize,
    reimport_clean: usize,
    roundtrip_valid: usize,
    execution_attempted: usize,
    execution_passed: usize,
    reference_matched: usize,
}

impl SurveySummary {
    fn from_outcomes(outcomes: &[SampleOutcome]) -> Self {
        Self {
            total: outcomes.len(),
            imported: count(outcomes, |outcome| outcome.import.is_passed()),
            import_clean: count(outcomes, |outcome| outcome.import.is_clean()),
            valid: count(outcomes, |outcome| outcome.validation.is_passed()),
            exported: count(outcomes, |outcome| outcome.export.is_passed()),
            export_clean: count(outcomes, |outcome| outcome.export.is_clean()),
            reimported: count(outcomes, |outcome| outcome.reimport.is_passed()),
            reimport_clean: count(outcomes, |outcome| outcome.reimport.is_clean()),
            roundtrip_valid: count(outcomes, |outcome| outcome.roundtrip_validation.is_passed()),
            execution_attempted: count(outcomes, |outcome| {
                outcome.execution.status != StageStatus::Skipped
            }),
            execution_passed: count(outcomes, |outcome| outcome.execution.is_passed()),
            reference_matched: count(outcomes, |outcome| outcome.reference_match.is_passed()),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total": self.total,
            "imported": self.imported,
            "import_clean": self.import_clean,
            "valid": self.valid,
            "exported": self.exported,
            "export_clean": self.export_clean,
            "reimported": self.reimported,
            "reimport_clean": self.reimport_clean,
            "roundtrip_valid": self.roundtrip_valid,
            "execution_attempted": self.execution_attempted,
            "execution_passed": self.execution_passed,
            "reference_matched": self.reference_matched,
        })
    }
}

fn count(outcomes: &[SampleOutcome], predicate: impl Fn(&SampleOutcome) -> bool) -> usize {
    outcomes.iter().filter(|outcome| predicate(outcome)).count()
}

struct SurveyWorkspace(PathBuf);

impl SurveyWorkspace {
    fn new() -> io::Result<Self> {
        let base = std::env::temp_dir();
        for attempt in 0..1_024 {
            let path = base.join(format!(
                "ferrule-mfd-survey-{}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique survey workspace",
        ))
    }

    fn export_path(&self, index: usize) -> PathBuf {
        self.0.join(format!("sample-{index}.mfd"))
    }
}

impl Drop for SurveyWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn validation_outcome(project: &mapping::Project) -> StageOutcome {
    let diagnostics = engine::validate(project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        StageOutcome::passed(Vec::new())
    } else {
        StageOutcome::failed(diagnostics)
    }
}

fn discover_sample_paths(samples_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut directories = vec![(samples_dir.to_path_buf(), 0usize)];
    let mut sample_paths = Vec::new();
    while let Some((directory, depth)) = directories.pop() {
        let mut entries = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if depth >= MAX_SAMPLE_DEPTH {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "sample directory nesting exceeds {MAX_SAMPLE_DEPTH} levels at {}",
                            path.display()
                        ),
                    ));
                }
                directories.push((path, depth + 1));
                continue;
            }
            if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
            {
                sample_paths.push(path);
                if sample_paths.len() > MAX_SAMPLE_FILES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("sample corpus exceeds {MAX_SAMPLE_FILES} mapping files"),
                    ));
                }
            }
        }
    }
    sample_paths.sort();
    Ok(sample_paths)
}

fn sample_name(samples_dir: &Path, path: &Path) -> String {
    path.strip_prefix(samples_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn survey_file(samples_dir: &Path, path: &Path, export_path: &Path) -> SampleOutcome {
    let file = sample_name(samples_dir, path);
    let mut outcome = SampleOutcome::pending(file);
    let imported = match mfd::import(path) {
        Ok(imported) => imported,
        Err(error) => {
            outcome.import = StageOutcome::failed(vec![error.to_string()]);
            return outcome;
        }
    };
    outcome.import = StageOutcome::passed(imported.warnings);
    outcome.validation = validation_outcome(&imported.project);

    let export_warnings = match mfd::export(&imported.project, export_path) {
        Ok(warnings) => warnings,
        Err(error) => {
            outcome.export = StageOutcome::failed(vec![error.to_string()]);
            return outcome;
        }
    };
    outcome.export = StageOutcome::passed(export_warnings);

    let roundtripped = match mfd::import(export_path) {
        Ok(imported) => imported,
        Err(error) => {
            outcome.reimport = StageOutcome::failed(vec![error.to_string()]);
            return outcome;
        }
    };
    outcome.reimport = StageOutcome::passed(roundtripped.warnings);
    outcome.roundtrip_validation = validation_outcome(&roundtripped.project);
    outcome
}

fn print_categories<'a>(title: &str, diagnostics: impl Iterator<Item = &'a String>) {
    let mut categories = BTreeMap::new();
    for diagnostic in diagnostics {
        *categories
            .entry(diagnostic_category(diagnostic))
            .or_insert(0usize) += 1;
    }
    println!("\n-- {title} --");
    let mut categories = categories.into_iter().collect::<Vec<_>>();
    categories.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (diagnostic, count) in categories {
        println!("{count:4}  {diagnostic}");
    }
}

fn stage_diagnostics(
    outcomes: &[SampleOutcome],
    stage: impl Fn(&SampleOutcome) -> &StageOutcome,
    status: StageStatus,
) -> impl Iterator<Item = &String> {
    outcomes
        .iter()
        .map(stage)
        .filter(move |outcome| outcome.status == status)
        .flat_map(|outcome| outcome.diagnostics.iter())
}

fn write_json_report(
    path: &Path,
    samples_dir: &Path,
    summary: &SurveySummary,
    outcomes: &[SampleOutcome],
) -> Result<(), Box<dyn Error>> {
    let report = serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "kind": "ferrule.mfd_sample_compatibility",
        "samples_dir": samples_dir,
        "summary": summary.to_json(),
        "samples": outcomes.iter().map(SampleOutcome::to_json).collect::<Vec<_>>(),
    });
    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn print_details(outcomes: &[SampleOutcome]) {
    println!("\n-- per-file non-clean stages --");
    for outcome in outcomes {
        let stages = [
            ("import", &outcome.import),
            ("validation", &outcome.validation),
            ("export", &outcome.export),
            ("reimport", &outcome.reimport),
            ("roundtrip validation", &outcome.roundtrip_validation),
        ];
        if stages
            .iter()
            .all(|(_, stage)| stage.is_clean() || stage.status == StageStatus::Skipped)
        {
            continue;
        }
        println!("{}", outcome.file);
        for (name, stage) in stages {
            if stage.is_clean() || stage.status == StageStatus::Skipped {
                continue;
            }
            println!("    {name}: {}", stage.status.label());
            for diagnostic in &stage.diagnostics {
                println!("        {diagnostic}");
            }
        }
    }
}

#[test]
fn diagnostic_categories_replace_quoted_values_once() {
    assert_eq!(
        diagnostic_category("binding for `Person/Name` comes from `source`"),
        "binding for `_` comes from `_`"
    );
}

#[test]
fn summary_distinguishes_success_from_clean_success() {
    let mut clean = SampleOutcome::pending("clean.mfd".into());
    clean.import = StageOutcome::passed(Vec::new());
    clean.validation = StageOutcome::passed(Vec::new());
    clean.export = StageOutcome::passed(Vec::new());
    clean.reimport = StageOutcome::passed(Vec::new());
    clean.roundtrip_validation = StageOutcome::passed(Vec::new());

    let mut warned = SampleOutcome::pending("warned.mfd".into());
    warned.import = StageOutcome::passed(vec!["import warning".into()]);
    warned.validation = StageOutcome::passed(Vec::new());
    warned.export = StageOutcome::passed(vec!["export warning".into()]);
    warned.reimport = StageOutcome::failed(vec!["reimport failure".into()]);

    assert_eq!(
        SurveySummary::from_outcomes(&[clean, warned]),
        SurveySummary {
            total: 2,
            imported: 2,
            import_clean: 1,
            valid: 2,
            exported: 2,
            export_clean: 1,
            reimported: 1,
            reimport_clean: 1,
            roundtrip_valid: 1,
            execution_attempted: 0,
            execution_passed: 0,
            reference_matched: 0,
        }
    );
}

#[test]
fn sample_discovery_recurses_and_preserves_relative_identity() -> Result<(), Box<dyn Error>> {
    let workspace = SurveyWorkspace::new()?;
    let nested = workspace.0.join("tutorial/part-1");
    std::fs::create_dir_all(&nested)?;
    std::fs::write(workspace.0.join("root.mfd"), "")?;
    std::fs::write(nested.join("root.mfd"), "")?;
    std::fs::write(nested.join("ignored.txt"), "")?;

    let paths = discover_sample_paths(&workspace.0)?;
    let names = paths
        .iter()
        .map(|path| sample_name(&workspace.0, path))
        .collect::<Vec<_>>();
    assert_eq!(names, ["root.mfd", "tutorial/part-1/root.mfd"]);
    Ok(())
}

#[test]
#[ignore = "needs the local ReferenceSamples corpus; informational only"]
fn survey_samples() -> Result<(), Box<dyn Error>> {
    let samples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLES_DIR);
    if !samples_dir.is_dir() {
        eprintln!(
            "samples dir not found at {}; skipping",
            samples_dir.display()
        );
        return Ok(());
    }

    let sample_paths = discover_sample_paths(&samples_dir)?;

    let workspace = SurveyWorkspace::new()?;
    let outcomes = sample_paths
        .iter()
        .enumerate()
        .map(|(index, path)| survey_file(&samples_dir, path, &workspace.export_path(index)))
        .collect::<Vec<_>>();
    let summary = SurveySummary::from_outcomes(&outcomes);

    println!("== mfd compatibility survey: {} files ==", summary.total);
    println!(
        "imported: {} ({} with zero warnings)",
        summary.imported, summary.import_clean
    );
    println!("engine-valid: {}", summary.valid);
    println!(
        "exported: {} ({} with zero warnings)",
        summary.exported, summary.export_clean
    );
    println!(
        "re-imported: {} ({} with zero warnings)",
        summary.reimported, summary.reimport_clean
    );
    println!("post-export engine-valid: {}", summary.roundtrip_valid);
    println!("execution/reference comparison: not measured (read-only survey)");

    print_categories(
        "import failures",
        stage_diagnostics(&outcomes, |outcome| &outcome.import, StageStatus::Failed),
    );
    print_categories(
        "validation failures",
        stage_diagnostics(
            &outcomes,
            |outcome| &outcome.validation,
            StageStatus::Failed,
        ),
    );
    print_categories(
        "export failures",
        stage_diagnostics(&outcomes, |outcome| &outcome.export, StageStatus::Failed),
    );
    print_categories(
        "export warnings",
        stage_diagnostics(&outcomes, |outcome| &outcome.export, StageStatus::Passed),
    );
    print_categories(
        "re-import failures",
        stage_diagnostics(&outcomes, |outcome| &outcome.reimport, StageStatus::Failed),
    );
    print_categories(
        "re-import warnings",
        stage_diagnostics(&outcomes, |outcome| &outcome.reimport, StageStatus::Passed),
    );
    print_categories(
        "post-export validation failures",
        stage_diagnostics(
            &outcomes,
            |outcome| &outcome.roundtrip_validation,
            StageStatus::Failed,
        ),
    );

    if std::env::var_os("FERRULE_SURVEY_DETAILS").is_some() {
        print_details(&outcomes);
    }
    if let Some(report_path) = std::env::var_os(JSON_REPORT_ENV) {
        if report_path.is_empty() {
            return Err(format!("{JSON_REPORT_ENV} must name an output file").into());
        }
        let report_path = PathBuf::from(report_path);
        write_json_report(&report_path, &samples_dir, &summary, &outcomes)?;
        println!("json report: {}", report_path.display());
    }

    Ok(())
}
