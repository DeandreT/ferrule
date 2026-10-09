use std::ffi::OsString;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mfd::{ImportIssueKind, ImportProfile, MfdError};

const DESIGN: &str = include_str!("fixtures/executable-import/design.mfd");
const INPUT_XSD: &str = include_str!("fixtures/executable-import/input.xsd");
const OUTPUT_XSD: &str = include_str!("fixtures/executable-import/output.xsd");

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_executable_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("input.xsd"), INPUT_XSD).unwrap();
        std::fs::write(path.join("output.xsd"), OUTPUT_XSD).unwrap();
        Self(path)
    }

    fn design(&self, text: &str) -> PathBuf {
        let path = self.0.join("design.mfd");
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!("retained executable-import originals: {}", self.0.display());
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn retain(path: &Path, name: &str, value: &impl Debug) {
    std::fs::write(
        path.join(format!("{name}.debug.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}

fn run(directory: &Path, label: &str, destination: &Path, flags: &[OsString]) -> Output {
    let mut args: Vec<OsString> = [
        "--diagnostics",
        "json",
        "import-mfd",
        "--mfd",
        "design.mfd",
        "--out",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(destination.as_os_str().to_owned());
    args.extend_from_slice(flags);
    retain(directory, &format!("{label}-command"), &args);
    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .current_dir(directory)
        .args(&args)
        .output();
    retain(directory, &format!("{label}-process"), &result);
    let output = result.unwrap();
    std::fs::write(
        directory.join(format!("{label}-stdout.bin")),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        directory.join(format!("{label}-stderr.bin")),
        &output.stderr,
    )
    .unwrap();
    if destination.exists() {
        std::fs::write(
            directory.join(format!("{label}-destination.bin")),
            std::fs::read(destination).unwrap(),
        )
        .unwrap();
    }
    output
}

fn diagnostics(output: &Output) -> Vec<serde_json::Value> {
    std::str::from_utf8(&output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn decode(directory: &Path, label: &str, path: &Path) -> mapping::Project {
    let result = mapping::project_file::decode_bytes(&std::fs::read(path).unwrap());
    retain(directory, label, &result);
    result.unwrap()
}

#[test]
fn complete_import_has_identical_default_and_strict_projects_and_exact_values() {
    for resource_flags in [false, true] {
        let dir = Directory::new("complete");
        let design = dir.design(DESIGN);
        std::fs::write(
            dir.0.join("ferrule-package.json"),
            r#"{"schemaVersion":1,"kind":"ferrule.mapping-package","catalogs":[]}"#,
        )
        .unwrap();
        let mut flags = Vec::new();
        if resource_flags {
            flags.extend(
                [
                    "--package-manifest",
                    "ferrule-package.json",
                    "--edi-catalog-root",
                    ".",
                    "--json-schema-root",
                    ".",
                ]
                .map(Into::into),
            );
        } else {
            flags.extend(["--package-root", "."].map(Into::into));
        }
        let ordinary = dir.0.join("ordinary.json");
        let strict = dir.0.join("strict.json");
        let default_result = run(&dir.0, "ordinary", &ordinary, &flags);
        flags.push("--require-executable".into());
        let strict_result = run(&dir.0, "strict", &strict, &flags);
        assert!(default_result.status.success(), "{default_result:#?}");
        assert!(strict_result.status.success(), "{strict_result:#?}");
        assert!(default_result.stderr.is_empty());
        assert!(strict_result.stderr.is_empty());
        assert_eq!(
            std::fs::read(&ordinary).unwrap(),
            std::fs::read(&strict).unwrap()
        );
        let project = decode(&dir.0, "strict-project", &strict);
        assert!(project.extra_sources.is_empty());
        assert!(project.extra_targets.is_empty());
        assert!(project.failure_rules.is_empty());
        assert_eq!(project.source.name, "Input");
        assert_eq!(project.target.name, "Output");
        let input =
            Instance::Group(vec![("Number".into(), Instance::Scalar(Value::Int(37)))].into());
        let result = engine::run(&project, &input);
        retain(&dir.0, "complete-mapping-result", &result);
        assert_eq!(
            result.unwrap(),
            Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(37)))].into())
        );
        let opted = dir.0.join("opted.json");
        flags.push("--item-ordered-exceptions".into());
        let combined = run(&dir.0, "combined", &opted, &flags);
        assert!(combined.status.success(), "{combined:#?}");
        assert_eq!(
            std::fs::read(&strict).unwrap(),
            std::fs::read(&opted).unwrap()
        );
        assert_eq!(std::fs::read(&design).unwrap(), DESIGN.as_bytes());
        assert!(!dir.0.join("input.xml").exists());
        assert!(!dir.0.join("output.xml").exists());
    }
}

fn warning_design() -> String {
    DESIGN.replace("</children>", "<component name=\"unused\" library=\"fixture-unsupported\" kind=\"5\" uid=\"4\"/></children>")
}

fn invalid_design() -> String {
    DESIGN.replace("</children>", "<component name=\"logical-not\" library=\"core\" kind=\"5\" uid=\"4\"><sources><datapoint pos=\"0\" key=\"30\"/><datapoint pos=\"1\" key=\"31\"/></sources><targets><datapoint pos=\"0\" key=\"32\"/></targets></component></children>")
        .replace("<edge from=\"11\" to=\"21\"/>", "<edge from=\"11\" to=\"30\"/><edge from=\"11\" to=\"31\"/><edge from=\"32\" to=\"21\"/>")
}

fn dependency_design() -> String {
    DESIGN
        .replace(
            "name=\"Input\" library=\"xml\" kind=\"14\"",
            "name=\"Input\" library=\"text\" kind=\"16\"",
        )
        .replace(
            "<document schema=\"input.xsd\" inputinstance=\"input.xml\" instanceroot=\"{}Input\"/>",
            "<text type=\"edi\" kind=\"EDIX12\" inputinstance=\"input.edi\"/>",
        )
}

#[test]
fn all_static_blocker_kinds_refuse_before_destination_mutation() {
    for (label, design_text, expected_kinds, detail) in [
        (
            "warning",
            warning_design(),
            vec![ImportIssueKind::ImportWarning],
            "unsupported library `fixture-unsupported`",
        ),
        (
            "validation",
            invalid_design(),
            vec![ImportIssueKind::Validation],
            "function `not` expects exactly 1 argument(s), got 2",
        ),
        (
            "dependency",
            dependency_design(),
            vec![
                ImportIssueKind::ImportWarning,
                ImportIssueKind::RuntimeDependency,
            ],
            "requires an EDI configuration",
        ),
    ] {
        let dir = Directory::new(label);
        let design = dir.design(&design_text);
        let ordinary = dir.0.join("ordinary.json");
        let default_result = run(&dir.0, "ordinary", &ordinary, &[]);
        assert!(default_result.status.success(), "{default_result:#?}");
        let imported = decode(&dir.0, "ordinary-project", &ordinary);
        let validation = engine::validate(&imported);
        retain(&dir.0, "ordinary-validation", &validation);
        assert_eq!(validation.is_empty(), label != "validation");
        for existing in [false, true] {
            let destination = if existing {
                dir.0.join("existing.json")
            } else {
                dir.0.join("absent/project.json")
            };
            let sentinel = b"existing project\n";
            let before = if existing {
                std::fs::write(&destination, sentinel).unwrap();
                Some(std::fs::metadata(&destination).unwrap().modified().unwrap())
            } else {
                None
            };
            let result = run(
                &dir.0,
                &format!("strict-{existing}"),
                &destination,
                &["--require-executable".into()],
            );
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
            let diagnostics = diagnostics(&result);
            assert!(!diagnostics.is_empty());
            assert!(
                diagnostics
                    .iter()
                    .all(|d| d["command"] == "import-mfd" && d["severity"] == "error")
            );
            assert!(
                diagnostics.iter().any(|d| d["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(detail))),
                "{diagnostics:#?}"
            );
            if existing {
                assert_eq!(std::fs::read(&destination).unwrap(), sentinel);
                assert_eq!(
                    std::fs::metadata(&destination).unwrap().modified().unwrap(),
                    before.unwrap()
                );
            } else {
                assert!(!destination.parent().unwrap().exists());
            }
        }
        let result = cli::import_mfd_with_profile(
            &design,
            &dir.0.join("api.json"),
            Some(&dir.0),
            None,
            &[],
            &[],
            ImportProfile::Executable,
        );
        retain(&dir.0, "typed-api-result", &result);
        let error = result.unwrap_err();
        let Some(MfdError::IncompatibleImport(report)) = error.downcast_ref::<MfdError>() else {
            panic!("wrong error: {error:#?}")
        };
        assert_eq!(
            report
                .issues
                .iter()
                .map(|issue| issue.kind)
                .collect::<Vec<_>>(),
            expected_kinds
        );
        assert!(!dir.0.join("api.json").exists());
        let human = Command::new(env!("CARGO_BIN_EXE_ferrule"))
            .current_dir(&dir.0)
            .args([
                "import-mfd",
                "--mfd",
                "design.mfd",
                "--out",
                "human.json",
                "--require-executable",
            ])
            .output();
        retain(&dir.0, "human-diagnostic-process", &human);
        let human = human.unwrap();
        assert!(!human.status.success());
        assert!(String::from_utf8_lossy(&human.stderr).contains("executable import rejected"));
        assert!(!dir.0.join("human.json").exists());
        assert_eq!(std::fs::read(&design).unwrap(), design_text.as_bytes());
    }
}

#[test]
fn executable_admission_preserves_global_failure_identity_and_order() {
    for multiple in [false, true] {
        let dir = Directory::new("global");
        let original = include_str!("../../mfd/tests/fixtures/exception.mfd");
        let text = if multiple {
            original.replace("        <component name=\"reject-expense\"", r#"        <component name="constant" library="core" kind="2" uid="6"><targets><datapoint pos="0" key="60"/></targets><data><constant value="first rule" datatype="string"/></data></component>
        <component name="first-exception" library="core" kind="18" uid="7"><sources><datapoint pos="0" key="61"/><datapoint pos="1" key="62"/></sources><data><exception/></data></component>
        <component name="reject-expense""#)
        .replace("      <edge from=\"23\" to=\"40\"/>", "      <edge from=\"10\" to=\"61\"/>\n      <edge from=\"60\" to=\"62\"/>\n      <edge from=\"23\" to=\"40\"/>")
        } else {
            original.into()
        };
        dir.design(&text);
        for (name, contents) in [
            (
                "exception-source.xsd",
                include_str!("../../mfd/tests/fixtures/exception-source.xsd"),
            ),
            (
                "exception-target.xsd",
                include_str!("../../mfd/tests/fixtures/exception-target.xsd"),
            ),
        ] {
            std::fs::write(dir.0.join(name), contents).unwrap();
        }
        let ordinary = dir.0.join("ordinary.json");
        let strict = dir.0.join("strict.json");
        let default_result = run(&dir.0, "ordinary", &ordinary, &[]);
        let strict_result = run(&dir.0, "strict", &strict, &["--require-executable".into()]);
        assert!(default_result.status.success(), "{default_result:#?}");
        assert!(strict_result.status.success(), "{strict_result:#?}");
        assert_eq!(
            std::fs::read(&ordinary).unwrap(),
            std::fs::read(&strict).unwrap()
        );
        let project = decode(&dir.0, "global-project", &strict);
        assert_eq!(project.failure_rules.len(), if multiple { 2 } else { 1 });
        assert!(
            !project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, mapping::Node::Raise { .. }))
        );
        let xml = "<Expenses><Expense><Allowed>true</Allowed><Description>accepted</Description></Expense><Expense><Allowed>false</Allowed><Description>late selection</Description></Expense></Expenses>";
        std::fs::write(dir.0.join("authored-input.xml"), xml).unwrap();
        let input = format_xml::from_str(xml, &project.source);
        retain(&dir.0, "global-input", &input);
        let result = engine::run(&project, &input.unwrap());
        retain(&dir.0, "global-result", &result);
        assert_eq!(
            result,
            Err(engine::EngineError::MappingFailure {
                rule: 1,
                message: Some(
                    if multiple {
                        "first rule"
                    } else {
                        "late selection"
                    }
                    .into()
                )
            })
        );
        let combined_path = dir.0.join("combined.json");
        let combined = run(
            &dir.0,
            "combined",
            &combined_path,
            &[
                "--require-executable".into(),
                "--item-ordered-exceptions".into(),
            ],
        );
        if multiple {
            assert!(!combined.status.success());
            assert!(!combined_path.exists());
            assert!(diagnostics(&combined).iter().any(|d| {
                d["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("requires one exception"))
            }));
        } else {
            assert!(combined.status.success(), "{combined:#?}");
            let item = decode(&dir.0, "combined-project", &combined_path);
            let input = format_xml::from_str(xml, &item.source);
            retain(&dir.0, "combined-input", &input);
            let item_result = engine::run(&item, &input.unwrap());
            retain(&dir.0, "combined-result", &item_result);
            assert!(item.failure_rules.is_empty());
            assert!(
                matches!(item_result, Err(engine::EngineError::MappingException { message: Some(ref message), .. }) if message == "late selection")
            );
        }
        assert!(!dir.0.join("accepted.xml").exists());
    }
}

#[test]
fn pipeline_flag_conflict_refuses_without_publication() {
    let dir = Directory::new("conflict");
    dir.design(DESIGN);
    let destination = dir.0.join("absent/pipeline.json");
    let result = run(
        &dir.0,
        "conflict",
        &destination,
        &["--pipeline".into(), "--require-executable".into()],
    );
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let diagnostics = diagnostics(&result);
    assert!(
        diagnostics.iter().any(|d| d["message"]
            .as_str()
            .is_some_and(|message| message.contains("--require-executable")
                && message.contains("--pipeline")))
    );
    assert!(!destination.parent().unwrap().exists());
}
