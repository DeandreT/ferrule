use std::error::Error;
use std::path::Path;

pub const ENFORCE_ENV: &str = "FERRULE_MFD_SURVEY_ENFORCE_BASELINE";

pub fn enforce_enabled() -> Result<bool, Box<dyn Error>> {
    let Some(value) = std::env::var_os(ENFORCE_ENV) else {
        return Ok(false);
    };
    let value = value
        .to_str()
        .ok_or_else(|| format!("{ENFORCE_ENV} must be valid UTF-8"))?;
    match value {
        "1" | "true" => Ok(true),
        "0" | "false" | "" => Ok(false),
        _ => Err(format!("{ENFORCE_ENV} must be one of 1, true, 0, false, or empty").into()),
    }
}

/// The ignored surveys need the gitignored private compatibility corpus. When
/// it is missing, an explicit run fails rather than reporting a pass.
pub fn require_corpus(samples_dir: &Path) -> Result<(), Box<dyn Error>> {
    if samples_dir.is_dir() {
        return Ok(());
    }
    Err(format!(
        "private compatibility corpus not present at {}; the survey needs the \
         local gitignored samples/ReferenceSamples corpus",
        samples_dir.display()
    )
    .into())
}

pub fn enforce_exact(survey: &str, metrics: &[(&str, usize, usize)]) -> Result<(), Box<dyn Error>> {
    check_exact(enforce_enabled()?, survey, metrics)
}

fn check_exact(
    enforce: bool,
    survey: &str,
    metrics: &[(&str, usize, usize)],
) -> Result<(), Box<dyn Error>> {
    if !enforce {
        return Ok(());
    }
    if metrics.is_empty() {
        return Err(format!("{survey} baseline gate has no metrics").into());
    }
    let mismatches = metrics
        .iter()
        .filter(|(_, actual, expected)| actual != expected)
        .map(|(name, actual, expected)| format!("{name}: expected {expected}, got {actual}"))
        .collect::<Vec<_>>();
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{survey} no longer matches its reviewed private-corpus baseline:\n{}",
            mismatches.join("\n")
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_gate_does_not_constrain_exploratory_surveys() {
        check_exact(false, "survey", &[("total", 1, 2)]).unwrap();
    }

    #[test]
    fn enabled_gate_rejects_empty_or_changed_baselines() {
        let empty = check_exact(true, "survey", &[]).unwrap_err();
        assert!(empty.to_string().contains("has no metrics"));

        let changed =
            check_exact(true, "survey", &[("total", 186, 187), ("warnings", 1, 0)]).unwrap_err();
        let changed = changed.to_string();
        assert!(changed.contains("total: expected 187, got 186"));
        assert!(changed.contains("warnings: expected 0, got 1"));
    }

    #[test]
    fn enabled_gate_accepts_reviewed_metrics() {
        check_exact(true, "survey", &[("total", 187, 187), ("warnings", 0, 0)]).unwrap();
    }

    #[test]
    fn missing_corpus_fails_with_its_path() {
        let missing = Path::new("definitely-not-a-real-ferrule-corpus");
        let error = require_corpus(missing).unwrap_err().to_string();
        assert!(error.contains("corpus not present at definitely-not-a-real-ferrule-corpus"));
    }
}
