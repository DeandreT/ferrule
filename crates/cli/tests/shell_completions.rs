use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

/// Values of the `--diagnostics`, `generate --language`,
/// `generate --x12-*-envelope-profile` and `export-mfd --profile` value enums.
// `json` and `rust` also occur in command and option names, so their enums are
// checked through their distinctive siblings and the Bash/Zsh value lists.
const OPTION_VALUES: [&str; 7] = [
    "human",
    "csharp",
    "single-transaction",
    "grouped-transactions",
    "mapforce-interchanges",
    "ferrule-extensions",
    "native-mfd",
];

/// Removes a per-test directory on drop, including after a panic.
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_shell_completions_{name}_{}",
            std::process::id()
        ));
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ferrule_in(directory: &Path, args: &[&str]) -> anyhow::Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(args)
        .current_dir(directory)
        .output()?)
}

fn ferrule(args: &[&str]) -> anyhow::Result<Output> {
    ferrule_in(Path::new(env!("CARGO_MANIFEST_DIR")), args)
}

/// Subcommand names listed by `ferrule --help`, so the expectation follows
/// the parser rather than a copied list.
fn listed_subcommands() -> anyhow::Result<Vec<String>> {
    let output = ferrule(&["--help"])?;
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout)?;
    let names = help
        .lines()
        .skip_while(|line| *line != "Commands:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect();
    Ok(names)
}

fn completion_script(shell: &str) -> anyhow::Result<String> {
    let output = ferrule(&["completions", shell])?;
    assert!(output.status.success(), "{shell}: {output:?}");
    assert!(output.stderr.is_empty(), "{shell}: {output:?}");
    let script = String::from_utf8(output.stdout)?;
    assert!(!script.trim().is_empty(), "{shell} script is empty");
    Ok(script)
}

#[test]
fn help_lists_every_current_subcommand() -> anyhow::Result<()> {
    let subcommands = listed_subcommands()?;
    for expected in [
        "bundle-project",
        "run-rest-json",
        "run",
        "run-document",
        "run-pipeline",
        "validate",
        "generate",
        "import-xsd",
        "import-json-schema",
        "import-db",
        "import-mfd",
        "export-mfd",
        "completions",
    ] {
        assert!(
            subcommands.iter().any(|name| name == expected),
            "{expected} missing from {subcommands:?}"
        );
    }
    Ok(())
}

#[test]
fn every_shell_script_completes_every_subcommand() -> anyhow::Result<()> {
    let subcommands = listed_subcommands()?;
    for shell in SHELLS {
        let script = completion_script(shell)?;
        for name in &subcommands {
            assert!(script.contains(name.as_str()), "{shell} lacks {name}");
        }
        // Fish spells long options as `-l NAME`, so match names only.
        for option in ["diagnostics", "rust-runtime-path", "allow-live-rest"] {
            assert!(script.contains(option), "{shell} lacks option {option}");
        }
    }
    Ok(())
}

#[test]
fn shell_scripts_offer_value_enum_values() -> anyhow::Result<()> {
    // clap_complete's PowerShell script completes subcommands and flags only,
    // and its Fish script completes option values but not positional ones.
    for shell in ["bash", "zsh", "fish"] {
        let script = completion_script(shell)?;
        for value in OPTION_VALUES {
            assert!(script.contains(value), "{shell} lacks value {value}");
        }
    }
    for (shell, values) in [
        ("bash", ["\"human json\"", "\"rust csharp\""]),
        ("zsh", ["(human json)", "(rust csharp)"]),
    ] {
        let script = completion_script(shell)?;
        for value in values {
            assert!(script.contains(value), "{shell} lacks values {value}");
        }
    }
    for shell in ["bash", "zsh"] {
        let script = completion_script(shell)?;
        for value in SHELLS {
            assert!(script.contains(value), "{shell} lacks shell {value}");
        }
    }
    Ok(())
}

#[test]
fn generation_is_deterministic_and_writes_no_files() -> anyhow::Result<()> {
    let directory = TestDirectory::new("no_files")?;
    for shell in SHELLS {
        let first = ferrule_in(directory.path(), &["completions", shell])?;
        let second = ferrule_in(directory.path(), &["completions", shell])?;
        assert!(first.status.success() && second.status.success());
        assert_eq!(first.stdout, second.stdout, "{shell}");
    }
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    Ok(())
}

#[test]
fn json_diagnostics_flag_keeps_stdout_script_only() -> anyhow::Result<()> {
    let output = ferrule(&["--diagnostics", "json", "completions", "bash"])?;
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, completion_script("bash")?.into_bytes());
    Ok(())
}

#[test]
fn invalid_or_missing_shells_fail_on_stderr_only() -> anyhow::Result<()> {
    for args in [
        &["completions", "tcsh"][..],
        &["completions", "elvish"],
        &["completions", "Bash"],
        &["completions"],
        &["completions", "bash", "zsh"],
    ] {
        let output = ferrule(args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("error:"), "{args:?}: {stderr}");
    }

    let output = ferrule(&["completions", "tcsh"])?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("invalid value 'tcsh'"), "{stderr}");
    assert!(
        stderr.contains("[possible values: bash, zsh, fish, powershell]"),
        "{stderr}"
    );
    Ok(())
}

#[test]
fn invalid_shell_reports_a_json_diagnostic() -> anyhow::Result<()> {
    let output = ferrule(&["--diagnostics", "json", "completions", "tcsh"])?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr)?;
    let diagnostics = stderr
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(diagnostics.len(), 1, "{stderr}");
    assert_eq!(diagnostics[0]["command"], "completions");
    assert_eq!(diagnostics[0]["severity"], "error");
    Ok(())
}

#[test]
fn completions_help_lists_supported_shells() -> anyhow::Result<()> {
    let output = ferrule(&["completions", "--help"])?;
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let help = String::from_utf8(output.stdout)?;
    assert!(help.contains("Usage: ferrule completions"), "{help}");
    assert!(
        help.contains("[possible values: bash, zsh, fish, powershell]"),
        "{help}"
    );

    let output = ferrule(&["help", "completions"])?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout)?, help);
    Ok(())
}
