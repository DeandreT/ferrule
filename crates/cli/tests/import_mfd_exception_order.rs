use std::ffi::OsString;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use engine::EngineError;
use mapping::Node;

const DESIGN: &str = include_str!("../../mfd/tests/fixtures/exception.mfd");
const SOURCE: &str = include_str!("../../mfd/tests/fixtures/exception-source.xsd");
const TARGET: &str = include_str!("../../mfd/tests/fixtures/exception-target.xsd");

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_item_import_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking()
            || std::env::var("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref() == Ok("1")
        {
            eprintln!("retained CLI item-import originals: {}", self.0.display());
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn retain(directory: &Path, name: &str, value: &impl Debug) {
    std::fs::write(
        directory.join(format!("{name}.debug.txt")),
        format!("{value:#?}\n"),
    )
    .unwrap();
}

fn fixture(directory: &Path, design: &str) -> PathBuf {
    std::fs::write(directory.join("exception-source.xsd"), SOURCE).unwrap();
    std::fs::write(directory.join("exception-target.xsd"), TARGET).unwrap();
    std::fs::write(directory.join("expenses.xml"), "<Expenses><Expense><Allowed>true</Allowed><Description>fixture</Description></Expense></Expenses>").unwrap();
    let path = directory.join("design.mfd");
    std::fs::write(&path, design).unwrap();
    path
}

fn two_exceptions() -> String {
    DESIGN.replace("        <component name=\"reject-expense\"", r#"        <component name="constant" library="core" kind="2" uid="6"><targets><datapoint pos="0" key="60"/></targets><data><constant value="first rule" datatype="string"/></data></component>
        <component name="first-exception" library="core" kind="18" uid="7"><sources><datapoint pos="0" key="61"/><datapoint pos="1" key="62"/></sources><data><exception/></data></component>
        <component name="reject-expense""#)
        .replace("      <edge from=\"23\" to=\"40\"/>", "      <edge from=\"10\" to=\"61\"/>\n      <edge from=\"60\" to=\"62\"/>\n      <edge from=\"23\" to=\"40\"/>")
}

fn exception_pipeline() -> String {
    DESIGN.replace("<properties XSLTDefaultOutput=\"1\"/>", "<properties PassThrough=\"1\"/>")
        .replace("<entry name=\"Expense\" inpkey=\"30\"><entry name=\"Description\" inpkey=\"31\"/>", "<entry name=\"Expense\" inpkey=\"30\" outkey=\"32\"><entry name=\"Description\" inpkey=\"31\" outkey=\"33\"/>")
        .replace("    </children>", r#"        <component name="Final" library="xml" kind="14" uid="6"><properties XSLTDefaultOutput="1"/><data><root><entry name="Accepted"><entry name="Expense" inpkey="50"><entry name="Description" inpkey="51"/></entry></entry></root><document schema="exception-target.xsd" outputinstance="final.xml" instanceroot="{}Accepted"/></data></component>
    </children>"#)
        .replace("    </connections>", "      <edge from=\"32\" to=\"50\"/>\n      <edge from=\"33\" to=\"51\"/>\n    </connections>")
}

fn cli_import(
    directory: &Path,
    label: &str,
    design: &Path,
    out: &Path,
    pipeline: bool,
    item_order: bool,
) -> Output {
    let mut args: Vec<OsString> = ["--diagnostics", "json", "import-mfd", "--mfd"]
        .into_iter()
        .map(Into::into)
        .collect();
    args.push(design.as_os_str().to_owned());
    args.push("--out".into());
    args.push(out.as_os_str().to_owned());
    args.push("--package-root".into());
    args.push(directory.as_os_str().to_owned());
    if pipeline {
        args.push("--pipeline".into());
    }
    if item_order {
        args.push("--item-ordered-exceptions".into());
    }
    retain(directory, &format!("{label}-command"), &args);
    let result = Command::new(env!("CARGO_BIN_EXE_ferrule"))
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
    if out.exists() {
        std::fs::write(
            directory.join(format!("{label}-destination-original.bin")),
            std::fs::read(out).unwrap(),
        )
        .unwrap();
    }
    output
}

fn project(directory: &Path, label: &str, path: &Path) -> mapping::Project {
    let decoded = mapping::project_file::decode_bytes(&std::fs::read(path).unwrap());
    retain(directory, label, &decoded);
    decoded.unwrap()
}

fn assert_refusal(output: &Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostics: Vec<serde_json::Value> = std::str::from_utf8(&output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics
            .iter()
            .all(|d| d["command"] == "import-mfd" && d["severity"] == "error")
    );
    assert!(
        diagnostics.iter().any(|d| d["message"]
            .as_str()
            .is_some_and(|m| m.contains("requires one exception"))),
        "{diagnostics:#?}"
    );
}

#[test]
fn flag_selects_node_exception_for_both_polarities_and_default_keeps_global_rule() {
    for throw_true in [false, true] {
        let dir = Directory::new(if throw_true { "true" } else { "false" });
        let text = if throw_true {
            DESIGN
                .replace("from=\"22\" to=\"30\"", "from=\"23\" to=\"30\"")
                .replace("from=\"23\" to=\"40\"", "from=\"22\" to=\"40\"")
        } else {
            DESIGN.into()
        };
        let design = fixture(&dir.0, &text);
        let before = std::fs::read(&design).unwrap();
        let global_path = dir.0.join("global.json");
        let item_path = dir.0.join("item.json");
        let global = cli_import(&dir.0, "global", &design, &global_path, false, false);
        let item = cli_import(&dir.0, "item", &design, &item_path, false, true);
        assert!(global.status.success(), "{global:#?}");
        assert!(item.status.success(), "{item:#?}");
        let global = project(&dir.0, "global-project", &global_path);
        let item = project(&dir.0, "item-project", &item_path);
        assert_eq!(global.failure_rules.len(), 1);
        assert!(
            !global
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Raise { .. }))
        );
        assert!(item.failure_rules.is_empty());
        let raises = item
            .graph
            .nodes
            .iter()
            .filter_map(|(&id, node)| matches!(node, Node::Raise { .. }).then_some(id))
            .collect::<Vec<_>>();
        assert_eq!(raises.len(), 1);
        let literal = format!(
            "<Expenses><Expense><Allowed>{}</Allowed><Description>unselected</Description></Expense><Expense><Allowed>{throw_true}</Allowed><Description>late 😀</Description></Expense></Expenses>",
            !throw_true
        );
        std::fs::write(dir.0.join("selected-input.xml"), &literal).unwrap();
        let input = format_xml::from_str(&literal, &item.source);
        retain(&dir.0, "input-typed", &input);
        let input = input.unwrap();
        let global_result = engine::run(&global, &input);
        let item_result = engine::run(&item, &input);
        retain(&dir.0, "global-result", &global_result);
        retain(&dir.0, "item-result", &item_result);
        assert_eq!(
            global_result,
            Err(EngineError::MappingFailure {
                rule: 1,
                message: Some("late 😀".into())
            })
        );
        assert_eq!(
            item_result,
            Err(EngineError::MappingException {
                node: raises[0],
                message: Some("late 😀".into())
            })
        );
        let legacy_path = dir.0.join("legacy-api.json");
        let legacy = cli::import_mfd(&design, &legacy_path, Some(&dir.0), None, &[], &[]);
        retain(&dir.0, "legacy-api-result", &legacy);
        assert!(legacy.is_ok());
        assert_eq!(
            std::fs::read(&global_path).unwrap(),
            std::fs::read(&legacy_path).unwrap()
        );
        assert_eq!(std::fs::read(&design).unwrap(), before);
        assert!(!dir.0.join("accepted.xml").exists());
    }
}

#[test]
fn unsupported_opt_in_refuses_fresh_and_existing_project_without_global_fallback() {
    let dir = Directory::new("unsupported");
    let design = fixture(&dir.0, &two_exceptions());
    let before = std::fs::read(&design).unwrap();
    let ordinary = dir.0.join("ordinary.json");
    let result = cli_import(&dir.0, "ordinary", &design, &ordinary, false, false);
    assert!(result.status.success(), "{result:#?}");
    assert_eq!(
        project(&dir.0, "ordinary-project", &ordinary)
            .failure_rules
            .len(),
        2
    );
    for existing in [false, true] {
        let label = if existing { "sentinel" } else { "fresh" };
        let out = dir.0.join(format!("{label}.json"));
        if existing {
            std::fs::write(&out, b"existing project sentinel\n").unwrap();
        }
        let result = cli_import(&dir.0, label, &design, &out, false, true);
        assert_refusal(&result);
        if existing {
            assert_eq!(std::fs::read(&out).unwrap(), b"existing project sentinel\n");
        } else {
            assert!(!out.exists());
        }
    }
    let error = cli::import_mfd_with_exception_order(
        &design,
        &dir.0.join("api-fresh.json"),
        Some(&dir.0),
        None,
        &[],
        &[],
        true,
    );
    retain(&dir.0, "typed-api-error-full-chain", &error);
    let error = error.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<mfd::MfdError>(),
        Some(mfd::MfdError::IncompatibleImport(_))
    ));
    assert!(!dir.0.join("api-fresh.json").exists());
    assert_eq!(std::fs::read(&design).unwrap(), before);
    assert!(!dir.0.join("accepted.xml").exists());
}

#[test]
fn pipeline_selection_propagates_opt_in_and_plain_pipeline_default_stays_exact() {
    let dir = Directory::new("pipeline");
    let design = fixture(&dir.0, &exception_pipeline());
    let ordinary_path = dir.0.join("ordinary.pipeline.json");
    let ordinary = cli_import(&dir.0, "ordinary", &design, &ordinary_path, true, false);
    assert!(ordinary.status.success(), "{ordinary:#?}");
    let pipeline = mapping::pipeline_file::decode_bytes(&std::fs::read(&ordinary_path).unwrap());
    retain(&dir.0, "ordinary-pipeline", &pipeline);
    let pipeline = pipeline.unwrap();
    assert_eq!(pipeline.stages.len(), 2);
    assert_eq!(pipeline.stages[0].project.failure_rules.len(), 1);
    assert!(pipeline.stages[1].project.failure_rules.is_empty());
    let legacy_path = dir.0.join("legacy.pipeline.json");
    let legacy = cli::import_mfd_pipeline(&design, &legacy_path, Some(&dir.0), None, &[], &[]);
    retain(&dir.0, "legacy-pipeline-api-result", &legacy);
    assert!(legacy.is_ok());
    assert_eq!(
        std::fs::read(&ordinary_path).unwrap(),
        std::fs::read(&legacy_path).unwrap()
    );
    for existing in [false, true] {
        let label = if existing { "sentinel" } else { "fresh" };
        let out = dir.0.join(format!("{label}.pipeline.json"));
        if existing {
            std::fs::write(&out, b"existing pipeline sentinel\n").unwrap();
        }
        let result = cli_import(&dir.0, label, &design, &out, true, true);
        assert_refusal(&result);
        if existing {
            assert_eq!(
                std::fs::read(&out).unwrap(),
                b"existing pipeline sentinel\n"
            );
        } else {
            assert!(!out.exists());
        }
    }
    for (file, root) in [
        ("source.xsd", "Source"),
        ("buffer.xsd", "Buffer"),
        ("target.xsd", "Target"),
    ] {
        std::fs::write(dir.0.join(file), format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#)).unwrap();
    }
    std::fs::write(
        dir.0.join("source.xml"),
        "<Source><Value>unchanged</Value></Source>",
    )
    .unwrap();
    let plain = dir.0.join("plain.mfd");
    std::fs::write(&plain, r#"<mapping version="26"><component name="map"><structure><children>
<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
<component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Value" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#).unwrap();
    let plain_default = dir.0.join("plain-default.pipeline.json");
    let plain_opted = dir.0.join("plain-opted.pipeline.json");
    let a = cli_import(&dir.0, "plain-default", &plain, &plain_default, true, false);
    let b = cli_import(&dir.0, "plain-opted", &plain, &plain_opted, true, true);
    assert!(a.status.success(), "{a:#?}");
    assert!(b.status.success(), "{b:#?}");
    assert_eq!(
        std::fs::read(plain_default).unwrap(),
        std::fs::read(plain_opted).unwrap()
    );
    assert!(!dir.0.join("accepted.xml").exists());
    assert!(!dir.0.join("final.xml").exists());
    assert!(!dir.0.join("target.xml").exists());
}
