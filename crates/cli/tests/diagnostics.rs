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

/// The CLI imports on its main thread; a schema far past the XSD import
/// depth limit is refused there instead of overflowing the stack.
#[test]
fn schemas_past_the_xsd_depth_limit_fail_with_a_diagnostic() {
    let deep = temporary_path("diagnostics_deep_xsd").with_extension("xsd");
    let mut schema = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input" type="t0"/>"#,
    );
    for index in 0..6_000 {
        schema.push_str(&format!(
            r#"<xs:complexType name="t{index}"><xs:sequence><xs:element name="Value" type="t{}"/></xs:sequence></xs:complexType>"#,
            index + 1
        ));
    }
    schema.push_str(r#"<xs:complexType name="t6000"><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:schema>"#);
    std::fs::write(&deep, schema).unwrap();
    let output = ferrule(&[
        "--diagnostics",
        "json",
        "import-xsd",
        "--xsd",
        deep.to_str().unwrap(),
    ]);
    std::fs::remove_file(&deep).unwrap();

    // A stack overflow would end the process with a signal and no code.
    assert!(
        !output.status.success() && output.status.code().is_some(),
        "{output:?}"
    );
    let diagnostics = json_lines(&output.stderr);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["command"], "import-xsd");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("nesting and reference depth limit")),
        "{diagnostics:?}"
    );
}

/// The CLI imports on its main thread; a schema reached through a long chain
/// of include files imports there instead of overflowing the stack.
#[test]
fn schemas_behind_a_long_include_chain_import() {
    const FILES: usize = 10_000;
    let directory = temporary_path("diagnostics_include_chain").with_extension("");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let xs = r#"xmlns:xs="http://www.w3.org/2001/XMLSchema""#;
    for index in 1..FILES {
        let body = if index + 1 < FILES {
            format!(r#"<xs:include schemaLocation="s{}.xsd"/>"#, index + 1)
        } else {
            r#"<xs:complexType name="Line"><xs:sequence><xs:element name="Sku" type="xs:string"/></xs:sequence></xs:complexType>"#
                .to_string()
        };
        std::fs::write(
            directory.join(format!("s{index}.xsd")),
            format!("<xs:schema {xs}>{body}</xs:schema>"),
        )
        .unwrap();
    }
    let root = directory.join("s0.xsd");
    std::fs::write(
        &root,
        format!(
            r#"<xs:schema {xs}><xs:include schemaLocation="s1.xsd"/><xs:element name="Input" type="Line"/></xs:schema>"#
        ),
    )
    .unwrap();
    let output = ferrule(&[
        "--diagnostics",
        "json",
        "import-xsd",
        "--xsd",
        root.to_str().unwrap(),
    ]);
    std::fs::remove_dir_all(&directory).unwrap();

    assert!(output.status.success(), "{output:?}");
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["name"], "Input");
    assert_eq!(schema["kind"]["children"][0]["name"], "Sku", "{schema}");
}

/// A design whose entry tree nests far past the MFD entry depth limit is
/// refused on the CLI's main thread instead of overflowing the stack.
#[test]
fn designs_past_the_entry_depth_limit_fail_with_a_diagnostic() {
    let deep = temporary_path("diagnostics_deep_entries").with_extension("mfd");
    let typed = r#"<entry name="n" ferrule-kind="group" ferrule-repeating="0">"#;
    std::fs::write(
        &deep,
        format!(
            r#"<mapping version="22"><component name="defaultmap" uid="1"><structure><children><component name="Input" library="xml" uid="2" kind="14"><data><root>{}<entry name="Value" outkey="2" ferrule-kind="scalar" ferrule-repeating="0" datatype="string"/>{}</root></data></component></children></structure></component></mapping>"#,
            typed.repeat(200_000),
            "</entry>".repeat(200_000)
        ),
    )
    .unwrap();
    let out = temporary_path("diagnostics_deep_entries_project");
    let mut outputs = Vec::new();
    for pipeline in [false, true] {
        let mut args = vec![
            "--diagnostics",
            "json",
            "import-mfd",
            "--mfd",
            deep.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ];
        if pipeline {
            args.push("--pipeline");
        }
        outputs.push(ferrule(&args));
    }
    std::fs::remove_file(&deep).unwrap();

    for output in outputs {
        // A stack overflow would end the process with a signal and no code.
        assert!(
            !output.status.success() && output.status.code().is_some(),
            "{output:?}"
        );
        assert!(!out.exists());
        let diagnostics = json_lines(&output.stderr);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0]["command"], "import-mfd");
        assert!(
            diagnostics[0]["message"].as_str().is_some_and(|message| {
                message.contains(&format!(
                    "more than {} levels deep",
                    mfd::MAX_MFD_ENTRY_DEPTH
                ))
            }),
            "{diagnostics:?}"
        );
    }
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
