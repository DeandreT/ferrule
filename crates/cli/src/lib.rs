//! Headless runner: loads a mapping project and runs it against a CSV input
//! to produce a CSV output. Split out from `main.rs` so it's testable
//! without shelling out to the built binary.

use std::path::Path;

use anyhow::Context;

/// Loads the project at `project_path`, runs it over every row of
/// `input_path`, and writes the results to `output_path`. Returns the number
/// of rows written.
pub fn run_project(
    project_path: &Path,
    input_path: &Path,
    output_path: &Path,
) -> anyhow::Result<usize> {
    let project_json = std::fs::read_to_string(project_path)
        .with_context(|| format!("reading project file {}", project_path.display()))?;
    let project: mapping::Project = serde_json::from_str(&project_json)
        .with_context(|| format!("parsing project file {}", project_path.display()))?;

    let sources = format_csv::read(input_path, &project.source)
        .with_context(|| format!("reading input {}", input_path.display()))?;

    let mut targets = Vec::with_capacity(sources.len());
    for source in &sources {
        targets.push(engine::run(&project, source)?);
    }

    format_csv::write(output_path, &project.target, &targets)
        .with_context(|| format!("writing output {}", output_path.display()))?;

    Ok(targets.len())
}
