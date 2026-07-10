use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn ferrule(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(args)
        .output()
        .unwrap()
}

fn json_lines(output: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn temporary_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ferrule_cli_{name}_{}.json", std::process::id()))
}

#[test]
fn validation_issues_are_json_lines_with_a_failure_exit_code() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project.json");
    let text = std::fs::read_to_string(fixture).unwrap();
    let mut project: mapping::Project = serde_json::from_str(&text).unwrap();
    project.graph.nodes.insert(
        999,
        mapping::Node::Call {
            function: "not_a_builtin".into(),
            args: vec![12345],
        },
    );
    let invalid = temporary_path("diagnostics_invalid");
    std::fs::write(&invalid, serde_json::to_string(&project).unwrap()).unwrap();

    let output = ferrule(&[
        "--diagnostics",
        "json",
        "validate",
        "--project",
        invalid.to_str().unwrap(),
    ]);
    std::fs::remove_file(invalid).unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostics = json_lines(&output.stderr);
    assert!(diagnostics.len() >= 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["schema_version"] == 1
            && diagnostic["command"] == "validate"
            && diagnostic["severity"] == "error"
            && diagnostic["location"].is_string()
            && diagnostic["message"].is_string()
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown function"))
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing node 12345"))
    }));
}

#[test]
fn import_warnings_use_json_without_changing_stdout_or_the_artifact() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/noschema-json.mfd");
    let imported = temporary_path("diagnostics_imported");
    let output = ferrule(&[
        "--diagnostics",
        "json",
        "import-mfd",
        "--mfd",
        fixture.to_str().unwrap(),
        "--out",
        imported.to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(imported.exists());
    std::fs::remove_file(imported).unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("wrote "));
    let diagnostics = json_lines(&output.stderr);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["schema_version"], 1);
    assert_eq!(diagnostics[0]["command"], "import-mfd");
    assert_eq!(diagnostics[0]["severity"], "warning");
    assert!(diagnostics[0].get("location").is_none());
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no schema reference"))
    );
}

#[test]
fn import_failures_are_a_single_json_diagnostic() {
    let missing = temporary_path("diagnostics_missing_xsd").with_extension("xsd");
    let output = ferrule(&[
        "--diagnostics",
        "json",
        "import-xsd",
        "--xsd",
        missing.to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostics = json_lines(&output.stderr);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["schema_version"], 1);
    assert_eq!(diagnostics[0]["command"], "import-xsd");
    assert_eq!(diagnostics[0]["severity"], "error");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("importing xsd"))
    );
}

#[test]
fn invalid_usage_requested_as_json_is_one_diagnostic() {
    let output = ferrule(&["--diagnostics", "json", "validate"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostics = json_lines(&output.stderr);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["schema_version"], 1);
    assert_eq!(diagnostics[0]["command"], "validate");
    assert_eq!(diagnostics[0]["severity"], "error");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("--project"))
    );
}

#[test]
fn invalid_usage_uses_the_first_subcommand_not_an_option_value() {
    let output = ferrule(&[
        "--diagnostics",
        "json",
        "validate",
        "--project",
        "run",
        "--bogus",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostics = json_lines(&output.stderr);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["command"], "validate");
}
