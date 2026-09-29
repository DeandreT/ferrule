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

pub fn corpus_available(samples_dir: &Path) -> Result<bool, Box<dyn Error>> {
    check_corpus_available(enforce_enabled()?, samples_dir)
}

fn check_corpus_available(enforce: bool, samples_dir: &Path) -> Result<bool, Box<dyn Error>> {
    if samples_dir.is_dir() {
        return Ok(true);
    }
    if enforce {
        return Err(format!(
            "{ENFORCE_ENV}=1 requires the private compatibility corpus at {}",
            samples_dir.display()
        )
        .into());
    }
    eprintln!(
        "samples dir not found at {}; skipping",
        samples_dir.display()
    );
    Ok(false)
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
    fn enforced_corpus_must_exist() {
        let missing = Path::new("definitely-not-a-real-ferrule-corpus");
        assert!(!check_corpus_available(false, missing).unwrap());
        let error = check_corpus_available(true, missing).unwrap_err();
        assert!(error.to_string().contains(ENFORCE_ENV));
    }
}
