use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::PipelineInput;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-cli-chain-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_chain(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let maps = directory.join("maps");
    std::fs::create_dir_all(&maps)?;
    let source_schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    std::fs::write(maps.join("source.xsd"), source_schema)?;
    std::fs::write(
        maps.join("target.xsd"),
        source_schema.replace("name=\"Source\"", "name=\"Target\""),
    )?;
    std::fs::write(
        maps.join("source.xml"),
        "<Source><Value>passed through</Value></Source>",
    )?;
    let mapping = maps.join("mapping.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Source"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="source.xsd" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Value" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(mapping)
}

fn write_three_stage_chain(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let maps = directory.join("maps");
    std::fs::create_dir_all(&maps)?;
    for (file, root, field) in [
        ("source.xsd", "Source", "Start"),
        ("first.xsd", "FirstBuffer", "First"),
        ("second.xsd", "SecondBuffer", "Second"),
        ("target.xsd", "Target", "Result"),
    ] {
        std::fs::write(
            maps.join(file),
            format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="{root}"><xs:complexType><xs:sequence><xs:element name="{field}" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
            ),
        )?;
    }
    std::fs::write(
        maps.join("source.xml"),
        "<Source><Start>crossed every stage</Start></Source>",
    )?;
    let mapping = maps.join("mapping.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="first-buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="FirstBuffer"><entry name="First" inpkey="20" outkey="30"/></entry></root><document schema="first.xsd" instanceroot="{}FirstBuffer"/></data></component>
          <component name="second-buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="SecondBuffer"><entry name="Second" inpkey="40" outkey="50"/></entry></root><document schema="second.xsd" instanceroot="{}SecondBuffer"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="60"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="50"><edges><edge vertexkey="60"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(mapping)
}

fn output_message(result: &std::process::Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
}

#[test]
fn imports_and_runs_a_connected_two_stage_design() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mapping = write_chain(&directory.0)?;
    let output = directory.0.join("flow.json");
    let imported = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&mapping)
        .args(["--package-root"])
        .arg(&directory.0)
        .args(["--out"])
        .arg(&output)
        .output()?;
    assert!(imported.status.success(), "{}", output_message(&imported));
    assert!(
        String::from_utf8_lossy(&imported.stdout).contains("(0 warning(s))"),
        "{}",
        output_message(&imported)
    );
    let pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&output)?)?;
    assert_eq!(pipeline.stages.len(), 2);
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    assert_eq!(
        pipeline.main_mapping_path.as_deref(),
        Some("maps/mapping.mfd")
    );
    assert!(
        pipeline
            .stages
            .iter()
            .all(|stage| stage.mapping_path.as_deref() == Some("maps/mapping.mfd"))
    );
    let PipelineInput::Host { name } = &pipeline.stages[0].source else {
        panic!("first stage must have a host input");
    };
    let result = directory.0.join("result.xml");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&output)
        .args(["--input"])
        .arg(name)
        .arg(directory.0.join("maps/source.xml"))
        .args(["--output", "mfd-stage-2"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));
    let xml = std::fs::read_to_string(&result)?;
    assert!(xml.contains("<Value>passed through</Value>"), "{xml}");
    Ok(())
}

#[test]
fn imports_and_runs_a_connected_three_stage_design() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mapping = write_three_stage_chain(&directory.0)?;
    let output = directory.0.join("flow.json");
    let imported = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&mapping)
        .args(["--package-root"])
        .arg(&directory.0)
        .args(["--out"])
        .arg(&output)
        .output()?;
    assert!(imported.status.success(), "{}", output_message(&imported));
    assert!(
        String::from_utf8_lossy(&imported.stdout).contains("(0 warning(s))"),
        "{}",
        output_message(&imported)
    );
    let pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&output)?)?;
    assert_eq!(pipeline.stages.len(), 3);
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    assert_eq!(
        pipeline.main_mapping_path.as_deref(),
        Some("maps/mapping.mfd")
    );
    assert!(
        pipeline
            .stages
            .iter()
            .all(|stage| stage.mapping_path.as_deref() == Some("maps/mapping.mfd"))
    );
    for (index, (source, target)) in [
        ("Source", "FirstBuffer"),
        ("FirstBuffer", "SecondBuffer"),
        ("SecondBuffer", "Target"),
    ]
    .into_iter()
    .enumerate()
    {
        let stage = &pipeline.stages[index];
        assert_eq!(stage.id, format!("mfd-stage-{}", index + 1));
        assert_eq!(stage.project.source.name, source);
        assert_eq!(stage.project.target.name, target);
    }
    let PipelineInput::Host { name } = &pipeline.stages[0].source else {
        panic!("first stage must have a host input");
    };
    assert_eq!(name, "source");
    for index in 1..3 {
        assert_eq!(
            pipeline.stages[index].source,
            PipelineInput::StageTarget {
                stage: format!("mfd-stage-{index}"),
                target: None,
            }
        );
    }
    let first = directory.0.join("first.xml");
    let second = directory.0.join("second.xml");
    let result = directory.0.join("result.xml");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&output)
        .args(["--input"])
        .arg(name)
        .arg(directory.0.join("maps/source.xml"))
        .args(["--output", "mfd-stage-1"])
        .arg(&first)
        .args(["--output", "mfd-stage-2"])
        .arg(&second)
        .args(["--output", "mfd-stage-3"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));
    for (path, field) in [(&first, "First"), (&second, "Second"), (&result, "Result")] {
        let xml = std::fs::read_to_string(path)?;
        assert!(
            xml.contains(&format!("<{field}>crossed every stage</{field}>")),
            "{}: {xml}",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn unsupported_chain_does_not_replace_an_existing_pipeline_file() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mapping = write_chain(&directory.0)?;
    let original = std::fs::read_to_string(&mapping)?;
    std::fs::write(
        &mapping,
        original.replace("PassThrough=\"1\"", "PassThrough=\"0\""),
    )?;
    let output = directory.0.join("flow.json");
    std::fs::write(&output, "preserve this file")?;
    let imported = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&mapping)
        .args(["--out"])
        .arg(&output)
        .output()?;
    assert!(!imported.status.success(), "{}", output_message(&imported));
    assert_eq!(std::fs::read_to_string(&output)?, "preserve this file");
    Ok(())
}
