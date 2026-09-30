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

fn write_four_stage_chain(directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let mapping = write_three_stage_chain(directory)?;
    let maps = mapping.parent().unwrap();
    std::fs::write(
        maps.join("third.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="ThirdBuffer"><xs:complexType><xs:sequence><xs:element name="Third" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = std::fs::read_to_string(&mapping)?
        .replace(
            "<component name=\"target\" library=\"xml\"",
            "<component name=\"third-buffer\" library=\"xml\" kind=\"14\"><properties PassThrough=\"1\"/><data><root><entry name=\"ThirdBuffer\"><entry name=\"Third\" inpkey=\"60\" outkey=\"70\"/></entry></root><document schema=\"third.xsd\" instanceroot=\"{}ThirdBuffer\"/></data></component><component name=\"target\" library=\"xml\"",
        )
        .replace(
            "<entry name=\"Result\" inpkey=\"60\"/>",
            "<entry name=\"Result\" inpkey=\"80\"/>",
        )
        .replace(
            "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/></edges></vertex>",
            "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/></edges></vertex><vertex vertexkey=\"70\"><edges><edge vertexkey=\"80\"/></edges></vertex>",
        );
    std::fs::write(&mapping, design)?;
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
fn imports_and_runs_a_connected_xlsx_workbook_design() -> Result<(), Box<dyn Error>> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures");
    let source_path = fixtures.join("xlsx-hierarchical-source.xml");
    let imported = mfd::import(&fixtures.join("xlsx-hierarchical.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut workbook_project = imported.project;
    let source_schema = workbook_project.source.clone();
    let source_options = workbook_project.source_options.clone();
    workbook_project.source_path = None;
    workbook_project.target_path = Some("converted.xlsx".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some("source.xml".into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Default::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    };
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![
            mapping::PipelineStage {
                id: "copy".into(),
                mapping_path: None,
                project: copy_project,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            mapping::PipelineStage {
                id: "workbook".into(),
                mapping_path: None,
                project: workbook_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());

    let directory = TempDir::new()?;
    let maps = directory.0.join("maps");
    std::fs::create_dir_all(&maps)?;
    let input = maps.join("source.xml");
    std::fs::copy(&source_path, &input)?;
    let design = maps.join("workbook.mfd");
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report:?}");

    let saved_pipeline = directory.0.join("flow.json");
    let import = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&design)
        .args(["--package-root"])
        .arg(&directory.0)
        .args(["--out"])
        .arg(&saved_pipeline)
        .output()?;
    assert!(import.status.success(), "{}", output_message(&import));
    let imported_pipeline: mapping::Pipeline =
        serde_json::from_slice(&std::fs::read(&saved_pipeline)?)?;
    let PipelineInput::Host { name } = &imported_pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    let result = directory.0.join("result.xlsx");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&saved_pipeline)
        .args(["--input"])
        .arg(name)
        .arg(&input)
        .args(["--output", "mfd-stage-2"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));
    let target = &imported_pipeline.stages[1].project;
    let layout = target.target_options.xlsx_hierarchical.as_ref().unwrap();
    let published = format_xlsx::read_hierarchical(&result, &target.target, layout)?;
    let source = format_xml::read(&source_path, &pipeline.stages[0].project.source)?;
    let expected = engine::run_pipeline(
        &pipeline,
        &std::collections::BTreeMap::from([("input".into(), source)]),
    )?;
    let (expected_bytes, _) = format_xlsx::to_bytes_hierarchical(
        &pipeline.stages[1].project.target,
        &expected.stage("workbook").unwrap().primary,
        pipeline.stages[1]
            .project
            .target_options
            .xlsx_hierarchical
            .as_ref()
            .unwrap(),
    )?;
    let expected_cells = format_xlsx::from_bytes_hierarchical(
        &expected_bytes,
        &pipeline.stages[1].project.target,
        pipeline.stages[1]
            .project
            .target_options
            .xlsx_hierarchical
            .as_ref()
            .unwrap(),
    )?;
    assert_eq!(published, expected_cells);
    Ok(())
}

#[test]
fn imports_and_runs_a_connected_flextext_design_with_its_generated_layout()
-> Result<(), Box<dyn Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let source_path = samples.join("Quotations.xml");
    let sample_output = samples.join("QuotationsDoc.txt");
    let sample_design = samples.join("QuotationsDoc.mfd");
    if !source_path.is_file() || !sample_output.is_file() || !sample_design.is_file() {
        return Ok(());
    }
    let imported = mfd::import(&sample_design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut text_project = imported.project;
    let source_schema = text_project.source.clone();
    let source_options = text_project.source_options.clone();
    text_project.source_path = None;
    text_project.target_path = Some("converted.txt".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some("Quotations.xml".into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Default::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    };
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![
            mapping::PipelineStage {
                id: "copy".into(),
                mapping_path: None,
                project: copy_project,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            mapping::PipelineStage {
                id: "text".into(),
                mapping_path: None,
                project: text_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());

    let directory = TempDir::new()?;
    let maps = directory.0.join("maps");
    std::fs::create_dir_all(&maps)?;
    let input = maps.join("Quotations.xml");
    std::fs::copy(&source_path, &input)?;
    let design = maps.join("quotations.mfd");
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report:?}");
    let layout_path = maps.join("quotations-stage-2-target.mft");
    assert!(layout_path.is_file());
    assert!(std::fs::read_to_string(&design)?.contains("config=\"quotations-stage-2-target.mft\""));

    let saved_pipeline = directory.0.join("flow.json");
    let import = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&design)
        .args(["--package-root"])
        .arg(&directory.0)
        .args(["--out"])
        .arg(&saved_pipeline)
        .output()?;
    assert!(import.status.success(), "{}", output_message(&import));
    let imported_pipeline: mapping::Pipeline =
        serde_json::from_slice(&std::fs::read(&saved_pipeline)?)?;
    let PipelineInput::Host { name } = &imported_pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    let result = directory.0.join("result.txt");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&saved_pipeline)
        .args(["--input"])
        .arg(name)
        .arg(&input)
        .args(["--output", "mfd-stage-2"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));
    let source = format_xml::read(&source_path, &pipeline.stages[0].project.source)?;
    let direct = engine::run_pipeline(
        &pipeline,
        &std::collections::BTreeMap::from([("input".into(), source)]),
    )?;
    let expected = format_flextext::to_string(
        &pipeline.stages[1].project.target,
        &direct.stage("text").unwrap().primary,
        pipeline.stages[1]
            .project
            .target_options
            .flextext
            .as_ref()
            .unwrap(),
    )?;
    assert_eq!(std::fs::read(&result)?, expected.as_bytes());
    assert_eq!(std::fs::read(&result)?, std::fs::read(&sample_output)?);
    Ok(())
}

#[test]
fn imports_and_runs_a_connected_protobuf_design_with_its_generated_schema()
-> Result<(), Box<dyn Error>> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures");
    let imported = mfd::import(&fixtures.join("protobuf-target.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut protobuf_project = imported.project;
    let source_schema = protobuf_project.source.clone();
    let source_options = protobuf_project.source_options.clone();
    protobuf_project.source_path = None;
    protobuf_project.target_path = Some("directory.bin".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some("protobuf-target-source.xml".into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Default::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    };
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![
            mapping::PipelineStage {
                id: "copy".into(),
                mapping_path: None,
                project: copy_project,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            mapping::PipelineStage {
                id: "protobuf".into(),
                mapping_path: None,
                project: protobuf_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());

    let directory = TempDir::new()?;
    let maps = directory.0.join("maps");
    std::fs::create_dir_all(&maps)?;
    let source_path = fixtures.join("protobuf-target-source.xml");
    let input = maps.join("protobuf-target-source.xml");
    std::fs::copy(&source_path, &input)?;
    let design = maps.join("protobuf.mfd");
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report:?}");
    let schema_path = maps.join("protobuf-stage-2-target.proto");
    assert!(schema_path.is_file());
    assert!(
        std::fs::read_to_string(&design)?.contains("schemafile=\"protobuf-stage-2-target.proto\"")
    );

    let saved_pipeline = directory.0.join("flow.json");
    let import = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&design)
        .args(["--package-root"])
        .arg(&directory.0)
        .args(["--out"])
        .arg(&saved_pipeline)
        .output()?;
    assert!(import.status.success(), "{}", output_message(&import));
    let imported_pipeline: mapping::Pipeline =
        serde_json::from_slice(&std::fs::read(&saved_pipeline)?)?;
    let PipelineInput::Host { name } = &imported_pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    let result = directory.0.join("result.bin");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&saved_pipeline)
        .args(["--input"])
        .arg(name)
        .arg(&input)
        .args(["--output", "mfd-stage-2"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));

    let source = format_xml::read(&source_path, &pipeline.stages[0].project.source)?;
    let direct = engine::run_pipeline(
        &pipeline,
        &std::collections::BTreeMap::from([("input".into(), source)]),
    )?;
    let options = pipeline.stages[1]
        .project
        .target_options
        .protobuf
        .as_ref()
        .unwrap();
    let layout = format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )?;
    let expected = format_protobuf::to_vec(
        &layout,
        &options.root_message,
        &direct.stage("protobuf").unwrap().primary,
    )?;
    assert_eq!(
        expected,
        [
            0x0a, 0x04, b'D', b'e', b'm', b'o', 0x12, 0x0e, 0x08, 0x07, 0x12, 0x03, b'O', b'n',
            b'e', 0x18, 0x01, 0x22, 0x03, 0x0a, 0x01, b'A', 0x12, 0x0e, 0x08, 0x09, 0x12, 0x03,
            b'T', b'w', b'o', 0x18, 0x00, 0x22, 0x03, 0x0a, 0x01, b'B',
        ]
    );
    let published = std::fs::read(&result)?;
    assert_eq!(published, expected);
    assert_eq!(
        format_protobuf::from_slice(&layout, &options.root_message, &published)?,
        direct.stage("protobuf").unwrap().primary
    );
    Ok(())
}

#[test]
fn imports_and_runs_a_connected_xbrl_design_with_exact_instance_bytes() -> Result<(), Box<dyn Error>>
{
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures");
    let imported = mfd::import(&fixtures.join("xbrl-final.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut xbrl_project = imported.project;
    let source_schema = xbrl_project.source.clone();
    let source_options = xbrl_project.source_options.clone();
    xbrl_project.source_path = None;
    xbrl_project.target_path = Some("filing.xbrl".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some("xbrl-final-source.xml".into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Default::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    };
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![
            mapping::PipelineStage {
                id: "copy".into(),
                mapping_path: None,
                project: copy_project,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            mapping::PipelineStage {
                id: "xbrl".into(),
                mapping_path: None,
                project: xbrl_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());

    let directory = TempDir::new()?;
    let maps = directory.0.join("maps");
    std::fs::create_dir_all(&maps)?;
    let source_path = fixtures.join("xbrl-final-source.xml");
    let input = maps.join("xbrl-final-source.xml");
    std::fs::copy(&source_path, &input)?;
    std::fs::copy(
        fixtures.join("xbrl-final-taxonomy.xsd"),
        maps.join("xbrl-final-taxonomy.xsd"),
    )?;
    let design = maps.join("xbrl.mfd");
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd)?;
    assert!(report.is_native_compatible(), "{report:?}");

    let saved_pipeline = directory.0.join("flow.json");
    let import = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--pipeline", "--mfd"])
        .arg(&design)
        .args(["--package-root"])
        .arg(&directory.0)
        .args(["--out"])
        .arg(&saved_pipeline)
        .output()?;
    assert!(import.status.success(), "{}", output_message(&import));
    let imported_pipeline: mapping::Pipeline =
        serde_json::from_slice(&std::fs::read(&saved_pipeline)?)?;
    let PipelineInput::Host { name } = &imported_pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    let result = directory.0.join("result.xbrl");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&saved_pipeline)
        .args(["--input"])
        .arg(name)
        .arg(&input)
        .args(["--output", "mfd-stage-2"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));

    let source = format_xml::read(&source_path, &pipeline.stages[0].project.source)?;
    let direct = engine::run_pipeline(
        &pipeline,
        &std::collections::BTreeMap::from([("input".into(), source)]),
    )?;
    let final_project = &pipeline.stages[1].project;
    let options = final_project.target_options.xbrl.as_ref().unwrap();
    let expected = format_xbrl::to_string(
        &final_project.target,
        &direct.stage("xbrl").unwrap().primary,
        options,
    )?;
    let published = std::fs::read_to_string(&result)?;
    assert_eq!(published.as_bytes(), expected.as_bytes());
    assert!(published.contains("2026-06-30"));
    assert!(published.contains("reported"));
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
fn imports_and_runs_a_connected_four_stage_design() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let mapping = write_four_stage_chain(&directory.0)?;
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
    let pipeline: mapping::Pipeline = serde_json::from_slice(&std::fs::read(&output)?)?;
    assert_eq!(pipeline.stages.len(), 4);
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    for (index, (source, target)) in [
        ("Source", "FirstBuffer"),
        ("FirstBuffer", "SecondBuffer"),
        ("SecondBuffer", "ThirdBuffer"),
        ("ThirdBuffer", "Target"),
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
    let result = directory.0.join("result.xml");
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["run-pipeline", "--pipeline"])
        .arg(&output)
        .args(["--input"])
        .arg(name)
        .arg(directory.0.join("maps/source.xml"))
        .args(["--output", "mfd-stage-4"])
        .arg(&result)
        .output()?;
    assert!(run.status.success(), "{}", output_message(&run));
    let xml = std::fs::read_to_string(&result)?;
    assert!(
        xml.contains("<Result>crossed every stage</Result>"),
        "{xml}"
    );
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
