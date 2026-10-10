#[allow(dead_code, unused_imports)]
mod support;

use ir::{Instance, Value};
use mapping::Node;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use support::{ConnectionStyle, ScalarContext, ScalarLiteral, ScalarMfdBuilder};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn unit_float(value: &Value) {
    let Value::Float(value) = value else {
        panic!("random output must remain a Float")
    };
    assert!(value.is_finite() && value.is_sign_positive());
    assert!((0.0..1.0).contains(value));
}

fn retain(design: &Path, tag: &str) -> TestResult<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "ferrule_random_mfd_{tag}_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&root)?;
    for entry in std::fs::read_dir(design.parent().expect("fixture directory"))? {
        let entry = entry?;
        assert!(
            entry.file_type()?.is_file(),
            "authored fixture must be flat"
        );
        std::fs::copy(entry.path(), root.join(entry.file_name()))?;
    }
    eprintln!("complete random MFD evidence: {}", root.display());
    Ok(root.join(design.file_name().expect("fixture filename")))
}

fn record(root: &Path, label: &str, outcome: &impl Debug) -> std::io::Result<()> {
    std::fs::write(root.join(format!("{label}.txt")), format!("{outcome:#?}\n"))
}

fn import(root: &Path, label: &str, design: &Path) -> TestResult<mfd::Imported> {
    match mfd::import(design) {
        Ok(imported) => {
            record(
                root,
                label,
                &(
                    &imported.project,
                    &imported.warnings,
                    &imported.mapping_path,
                ),
            )?;
            Ok(imported)
        }
        Err(error) => {
            record(root, label, &error)?;
            Err(error.into())
        }
    }
}

#[test]
fn random_preserves_zero_inputs_and_float_results_through_contexts_and_roundtrips() -> TestResult {
    for context in [
        ScalarContext::Main,
        ScalarContext::UserDefined,
        ScalarContext::NestedUserDefined,
    ] {
        for style in [ConnectionStyle::Graph, ConnectionStyle::Legacy] {
            let fixture =
                ScalarMfdBuilder::new("random-roundtrip", "random", "lang", vec![], "decimal")
                    .context(context)
                    .connection_style(style)
                    .write()?;
            let design = retain(fixture.design(), &format!("{context:?}_{style:?}"))?;
            let root = design.parent().expect("retained directory");
            let imported = import(root, "import", &design)?;
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
            let validation = engine::validate(&imported.project);
            record(root, "validation", &validation)?;
            assert!(validation.is_empty());
            let source_text = "<Source><Seed>neutral</Seed></Source>";
            std::fs::write(root.join("source.xml"), source_text)?;
            let source = format_xml::from_str(source_text, &imported.project.source)?;
            let exported = root.join("roundtrip.mfd");
            let export = mfd::export(&imported.project, &exported);
            record(root, "export", &export)?;
            assert!(export?.is_empty());
            assert!(
                std::fs::read_to_string(&exported)?.contains("name=\"random\" library=\"lang\"")
            );
            let reimported = import(root, "reimport", &exported)?;
            assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
            let validation = engine::validate(&reimported.project);
            record(root, "roundtrip-validation", &validation)?;
            assert!(validation.is_empty());
            for (label, project) in [
                ("original", &imported.project),
                ("roundtrip", &reimported.project),
            ] {
                let lower = codegen::lower(project);
                record(root, &format!("{label}-lower"), &lower)?;
                lower?;
                let outcome = engine::run(project, &source);
                record(root, &format!("{label}-run"), &outcome)?;
                let output = outcome?;
                unit_float(
                    output
                        .field("Result")
                        .and_then(Instance::as_scalar)
                        .expect("result scalar"),
                );
            }
            if context == ScalarContext::Main {
                assert!(imported.project.graph.nodes.values().any(|node| matches!(node, Node::Call { function, args } if function == "random" && args.is_empty())));
            }
        }
    }
    Ok(())
}

#[test]
fn random_arity_and_nonstandard_identities_refuse_executable_admission() -> TestResult {
    let fixture = ScalarMfdBuilder::new(
        "random-arity",
        "random",
        "lang",
        vec![ScalarLiteral::Integer(1)],
        "decimal",
    )
    .write()?;
    let design = retain(fixture.design(), "arity")?;
    let root = design.parent().expect("retained directory");
    let imported = import(root, "import", &design)?;
    let issues = engine::validate(&imported.project);
    record(root, "validation", &issues)?;
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("random") && issue.message.contains("0")),
        "{issues:?}"
    );
    let lower = codegen::lower(&imported.project);
    record(root, "lower", &lower)?;
    assert!(lower.is_err());
    for (tag, library, wrong_kind) in [
        ("custom", "neutral-extension", false),
        ("core", "core", false),
        ("xpath", "xpath2", false),
        ("wrong-kind", "lang", true),
    ] {
        let fixture = ScalarMfdBuilder::new(tag, "random", library, vec![], "decimal").write()?;
        let design = retain(fixture.design(), tag)?;
        let root = design.parent().expect("retained directory");
        if wrong_kind {
            let original = std::fs::read_to_string(&design)?;
            std::fs::write(root.join("before-kind-change.mfd"), &original)?;
            let component = original.find("name=\"random\"").expect("random component");
            let kind = component
                + original[component..]
                    .find("kind=\"5\"")
                    .expect("standard function kind");
            let mut body = original;
            body.replace_range(kind..kind + "kind=\"5\"".len(), "kind=\"7\"");
            std::fs::write(&design, body)?;
        }
        let imported = import(root, "import", &design)?;
        assert!(!imported.warnings.is_empty());
        assert!(
            !imported
                .project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::Call { function, .. } if function == "random"))
        );
        if library != "neutral-extension" {
            let identity = format!(
                "unsupported:{library}:{}:random",
                if wrong_kind { 7 } else { 5 }
            );
            assert!(
                imported.project.graph.nodes.values().any(
                    |node| matches!(node, Node::Call { function, .. } if function == &identity)
                )
            );
            let validation = engine::validate(&imported.project);
            record(root, "validation", &validation)?;
            assert!(
                validation
                    .iter()
                    .any(|issue| issue.message.contains(&identity)),
                "{validation:?}"
            );
            let lower = codegen::lower(&imported.project);
            record(root, "lower", &lower)?;
            assert!(lower.is_err());
        }
        let executable = mfd::import_with_profile(
            &design,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        match &executable {
            Ok(outcome) => record(
                root,
                "executable-admission",
                &(
                    &outcome.imported.project,
                    &outcome.imported.warnings,
                    &outcome.imported.mapping_path,
                    &outcome.report,
                ),
            )?,
            Err(error) => record(root, "executable-admission", error)?,
        }
        assert!(executable.is_err());
    }
    Ok(())
}
