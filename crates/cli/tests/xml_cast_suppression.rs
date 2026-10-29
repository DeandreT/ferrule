use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Package(PathBuf);
impl Package {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_suppress_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let package = Self(path);
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/target_float_cast");
        for name in ["source.xsd", "target.xsd"] {
            std::fs::copy(fixtures.join(name), package.0.join(name))?;
        }
        let text = std::fs::read_to_string(fixtures.join("cast-first.mfd"))?;
        std::fs::write(
            package.0.join("mapping.mfd"),
            text.replace("cast-in-subtree", "suppress-cast-in-subtree"),
        )?;
        Ok(package)
    }
    fn project(&self) -> Result<mapping::Project, Box<dyn Error>> {
        let imported = mfd::import(&self.0.join("mapping.mfd"))?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let mut project = imported.project;
        project.source_path = Some("input.xml".into());
        project.target_path = Some("output.xml".into());
        Ok(project)
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn xml(value: &str, denominator: &str) -> String {
    format!(
        "<Envelope><Numerator>34</Numerator><Denominator>{denominator}</Denominator><Text>{value}</Text><Label>safe</Label></Envelope>"
    )
}

#[test]
fn raw_xml_matches_file_payload_and_filename_independent_profiles() -> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let project = package.project()?;
    let path = package.0.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    for value in ["9007199254740993", "\t 2.5 \n", "not-a-number", "00029"] {
        let source = xml(value, "2");
        std::fs::write(package.0.join("input.xml"), &source)?;
        cli::run_project(
            &path,
            &package.0.join("input.xml"),
            &package.0.join("output.xml"),
        )?;
        let expected = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Envelope>\n  <Amount>{value}</Amount>\n  <Amount>17</Amount>\n  <Label>safe</Label>\n</Envelope>"
        );
        assert_eq!(
            std::fs::read_to_string(package.0.join("output.xml"))?,
            expected
        );
        let document = cli::PayloadDocument::new(Path::new("input.xml"), source.as_bytes())?;
        let options = cli::PayloadRunOptions::new(document);
        let output = cli::run_project_value_payloads(&project, &path, &options)?;
        assert_eq!(output.artifacts.len(), 1);
        assert_eq!(output.artifacts[0].bytes, expected.as_bytes());
        for bounded in [
            cli::PayloadRunOptions::new(document).with_bounded_documents(),
            cli::PayloadRunOptions::new(document).with_bounded_tabular(),
        ] {
            let output = cli::run_project_value_payloads(
                &project,
                &path,
                &bounded.with_output_path(Path::new("result.bin")),
            )?;
            assert_eq!(output.artifacts[0].bytes, expected.as_bytes());
        }
    }
    Ok(())
}

#[test]
fn expression_and_xml_safety_failures_leave_existing_and_absent_targets_untouched()
-> Result<(), Box<dyn Error>> {
    let package = Package::new()?;
    let project = package.project()?;
    let path = package.0.join("project.json");
    std::fs::write(&path, mapping::project_file::encode_pretty(&project)?)?;
    let input = package.0.join("input.xml");
    let output = package.0.join("output.xml");
    for source in [xml("not-a-number", "0"), xml("&#0;", "2")] {
        std::fs::write(&input, &source)?;
        std::fs::write(&output, b"unchanged")?;
        assert!(cli::run_project(&path, &input, &output).is_err());
        assert_eq!(std::fs::read(&output)?, b"unchanged");
        let absent = package.0.join("absent/output.xml");
        assert!(cli::run_project(&path, &input, &absent).is_err());
        assert!(!absent.parent().unwrap().exists());
    }
    std::fs::write(&input, xml("00029", "2"))?;
    let invalid = package.0.join("wrong.json");
    assert!(cli::run_project(&path, &input, &invalid).is_err());
    assert!(!invalid.exists());
    let mut late = project.clone();
    let label = late
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Label")
        .unwrap()
        .node;
    late.graph.nodes.insert(
        label,
        mapping::Node::Const {
            value: ir::Value::String("\0".into()),
        },
    );
    std::fs::write(&path, mapping::project_file::encode_pretty(&late)?)?;
    std::fs::write(&output, b"unchanged")?;
    let error = cli::run_project(&path, &input, &output).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<format_xml::XmlFormatError>(),
        Some(format_xml::XmlFormatError::InvalidXmlCharacter { .. })
    ));
    assert_eq!(std::fs::read(&output)?, b"unchanged");
    let absent = package.0.join("late-absent/output.xml");
    assert!(cli::run_project(&path, &input, &absent).is_err());
    assert!(!absent.exists());
    Ok(())
}
