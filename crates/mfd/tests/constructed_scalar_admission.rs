use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ir::{Instance, ScalarType, SchemaKind, Value};
use mapping::ScopeConstruction;

const EXTRA: &str = r#"<component name="Text" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="string" inpkey="30"/></entry></entry></entry></root><json schema="text.schema.json"/></data></component>"#;

fn fixture(extra: bool) -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "ferrule_constructed_admission_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&path)?;
    let design = format!(
        r#"<mapping><component name="map"><structure><children>
      <component name="Source" library="xml" kind="14"><data><root><entry name="Input" outkey="10"><entry name="Code" outkey="11"/></entry></root><document schema="input.xsd" instanceroot="{{}}Input"/></data></component>
      <component name="Primary" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Output" inpkey="20"><entry name="Code" inpkey="21"/></entry></root><document schema="output.xsd" instanceroot="{{}}Output"/></data></component>
      {}</children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/>{}</edges></vertex></vertices></graph></structure></component></mapping>"#,
        if extra { EXTRA } else { "" },
        if extra {
            "<edge vertexkey=\"30\"/>"
        } else {
            ""
        }
    );
    fs::write(path.join("mapping.mfd"), design)?;
    for name in ["Input", "Output"] {
        fs::write(
            path.join(format!("{}.xsd", name.to_ascii_lowercase())),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{name}"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
            ),
        )?;
    }
    fs::write(path.join("text.schema.json"), "{\"type\":\"string\"}\n")?;
    fs::write(
        path.join("expected-before-import.txt"),
        if extra {
            "Best-effort preserves Text singular String/default Constructed and reports Validation. Executable profile refuses the incompatible constructor."
        } else {
            "Ordinary group constructor remains executable and returns the complete authored Code String value."
        },
    )?;
    eprintln!(
        "complete authored construction admission evidence: {}",
        path.display()
    );
    Ok(path)
}

fn observe(
    path: &std::path::Path,
    label: &str,
    outcome: &Result<mfd::ImportOutcome, mfd::MfdError>,
) -> Result<(), std::io::Error> {
    let complete = match outcome {
        Ok(outcome) => format!(
            "Ok({:#?})",
            (
                &outcome.imported.project,
                &outcome.imported.warnings,
                &outcome.imported.mapping_path,
                &outcome.report
            )
        ),
        Err(error) => format!("Err({error:#?})"),
    };
    fs::write(path.join(format!("{label}.debug.txt")), complete)
}

#[test]
fn executable_import_refuses_preserved_named_scalar_group_construction()
-> Result<(), Box<dyn Error>> {
    let path = fixture(true)?;
    let options = mfd::ImportOptions::default().with_package_root(&path);
    let imported = mfd::import_with_profile(
        &path.join("mapping.mfd"),
        &options,
        mfd::ImportProfile::BestEffort,
    );
    observe(&path, "best-effort-complete", &imported)?;
    let strict = mfd::import_with_profile(
        &path.join("mapping.mfd"),
        &options,
        mfd::ImportProfile::Executable,
    );
    observe(&path, "executable-complete", &strict)?;
    let imported = imported?;
    assert_eq!(imported.imported.project.extra_targets.len(), 1);
    let target = &imported.imported.project.extra_targets[0];
    assert_eq!(target.name, "Text");
    assert_eq!(
        target.schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );
    assert!(!target.schema.repeating);
    assert_eq!(target.root.construction, ScopeConstruction::Constructed);
    assert!(target.root.bindings.is_empty() && target.root.children.is_empty());
    assert!(
        imported
            .report
            .issues
            .iter()
            .any(|issue| issue.kind == mfd::ImportIssueKind::Validation
                && issue.message == "root scope: constructed scope requires a group target schema")
    );
    assert!(
        matches!(strict, Err(mfd::MfdError::IncompatibleImport(report)) if report.issues.iter().any(|issue|
        issue.kind == mfd::ImportIssueKind::Validation && issue.message == "root scope: constructed scope requires a group target schema"))
    );
    Ok(())
}

#[test]
fn executable_group_control_preserves_complete_native_value() -> Result<(), Box<dyn Error>> {
    let path = fixture(false)?;
    let imported = mfd::import_with_profile(
        &path.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&path),
        mfd::ImportProfile::Executable,
    );
    observe(&path, "executable-complete", &imported)?;
    let imported = imported?;
    let input = Instance::Group(
        vec![(
            "Code".into(),
            Instance::Scalar(Value::String("AUTHORED".into())),
        )]
        .into(),
    );
    let expected = input.clone();
    fs::write(
        path.join("expected-full-value-before-run.txt"),
        format!("{expected:#?}"),
    )?;
    let actual = engine::run(&imported.imported.project, &input);
    fs::write(
        path.join("complete-native-run.txt"),
        format!("input={input:#?}\noutput={actual:#?}"),
    )?;
    let lowered = codegen::lower(&imported.imported.project);
    fs::write(path.join("complete-lowering.txt"), format!("{lowered:#?}"))?;
    assert!(imported.report.executable && imported.report.issues.is_empty());
    assert_eq!(actual?, expected);
    codegen::validate_program(&lowered?)?;
    Ok(())
}
