use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_passthrough_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn write_three_stage_chain(directory: &Path) -> PathBuf {
    let schema = |root: &str, required: &str, optional: &str| {
        format!(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{required}\" type=\"xs:string\"/><xs:element name=\"{optional}\" type=\"xs:string\" minOccurs=\"0\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
        )
    };
    write(
        &directory.join("source.xsd"),
        &schema("Source", "Start", "Unused"),
    );
    write(
        &directory.join("first.xsd"),
        &schema("FirstBuffer", "First", "Cycle"),
    );
    write(
        &directory.join("second.xsd"),
        &schema("SecondBuffer", "Second", "Skipped"),
    );
    write(
        &directory.join("target.xsd"),
        &schema("Target", "Result", "Bypass"),
    );
    let mapping = directory.join("three-stage.mfd");
    // The intermediate components are deliberately declared in reverse stage
    // order so the importer must derive their order from the graph.
    write(
        &mapping,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="second-buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="SecondBuffer"><entry name="Second" inpkey="40" outkey="50"/><entry name="Skipped" inpkey="41" outkey="51"/></entry></root><document schema="second.xsd" instanceroot="{}SecondBuffer"/></data></component>
          <component name="first-buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="FirstBuffer"><entry name="First" inpkey="20" outkey="30"/><entry name="Cycle" inpkey="21" outkey="31"/></entry></root><document schema="first.xsd" instanceroot="{}FirstBuffer"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="60"/><entry name="Bypass" inpkey="61"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="50"><edges><edge vertexkey="60"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );
    mapping
}

fn write_serial_chain(directory: &Path, intermediate_count: usize) -> PathBuf {
    assert!(intermediate_count > 0);
    let schema = |root: &str, field: &str| {
        format!(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
        )
    };
    write(&directory.join("source.xsd"), &schema("Source", "Start"));
    write(&directory.join("target.xsd"), &schema("Target", "Result"));
    let mut children = String::from(
        r#"<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>"#,
    );
    // Declare the pass-through components in reverse order. Stage order must
    // come from graph feeds, never from component position.
    for index in (1..=intermediate_count).rev() {
        let root = format!("Buffer{index}");
        let field = format!("Step{index}");
        write(
            &directory.join(format!("buffer{index}.xsd")),
            &schema(&root, &field),
        );
        children.push_str(&format!(
            "<component name=\"buffer-{index}\" library=\"xml\" kind=\"14\"><properties PassThrough=\"1\"/><data><root><entry name=\"{root}\"><entry name=\"{field}\" inpkey=\"{}\" outkey=\"{}\"/></entry></root><document schema=\"buffer{index}.xsd\" instanceroot=\"{{}}{root}\"/></data></component>",
            index * 20,
            index * 20 + 10,
        ));
    }
    children.push_str(&format!(
        "<component name=\"target\" library=\"xml\" kind=\"14\"><properties XSLTDefaultOutput=\"1\"/><data><root><entry name=\"Target\"><entry name=\"Result\" inpkey=\"{}\"/></entry></root><document schema=\"target.xsd\" outputinstance=\"target.xml\" instanceroot=\"{{}}Target\"/></data></component>",
        (intermediate_count + 1) * 20,
    ));
    let mut vertices = String::new();
    for index in 0..=intermediate_count {
        vertices.push_str(&format!(
            "<vertex vertexkey=\"{}\"><edges><edge vertexkey=\"{}\"/></edges></vertex>",
            index * 20 + 10,
            (index + 1) * 20,
        ));
    }
    let mapping = directory.join("serial-chain.mfd");
    write(
        &mapping,
        &format!(
            "<mapping version=\"26\"><component name=\"map\"><structure><children>{children}</children><graph><vertices>{vertices}</vertices></graph></structure></component></mapping>"
        ),
    );
    mapping
}

#[test]
fn four_serial_xml_stages_import_and_execute_in_graph_order() {
    let dir = TempDir::new();
    let mapping = write_serial_chain(&dir.0, 3);
    let imported = mfd::import_pipeline(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages.len(), 4);
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    for (index, stage) in imported.pipeline.stages.iter().enumerate() {
        assert_eq!(stage.id, format!("mfd-stage-{}", index + 1));
        if index == 0 {
            assert_eq!(
                stage.source,
                mapping::PipelineInput::Host {
                    name: "source".into()
                }
            );
        } else {
            assert_eq!(
                stage.source,
                mapping::PipelineInput::StageTarget {
                    stage: format!("mfd-stage-{index}"),
                    target: None,
                }
            );
        }
    }
    let source = Instance::Group(vec![(
        "Start".into(),
        Instance::Scalar(Value::String("four stages".into())),
    )]);
    let outputs = engine::run_pipeline(
        &imported.pipeline,
        &BTreeMap::from([("source".to_string(), source)]),
    )
    .unwrap();
    for (id, field) in [
        ("mfd-stage-1", "Step1"),
        ("mfd-stage-2", "Step2"),
        ("mfd-stage-3", "Step3"),
        ("mfd-stage-4", "Result"),
    ] {
        assert_eq!(
            outputs
                .stage(id)
                .unwrap()
                .primary
                .field(field)
                .and_then(Instance::as_scalar),
            Some(&Value::String("four stages".into())),
            "{id}"
        );
    }
}

#[test]
fn serial_xml_chain_rejects_bypass_cycle_and_disconnected_component() {
    let dir = TempDir::new();
    let mapping = write_serial_chain(&dir.0, 3);
    let original = std::fs::read_to_string(&mapping).unwrap();
    let bypass = original
        .replace(
            "<entry name=\"Result\" inpkey=\"80\"/>",
            "<entry name=\"Result\" inpkey=\"80\"/><entry name=\"Bypass\" inpkey=\"81\"/>",
        )
        .replace(
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"81\"/></edges></vertex>",
        );
    write(&mapping, &bypass);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );

    let cycle = original
        .replace(
            "<entry name=\"Step1\" inpkey=\"20\" outkey=\"30\"/>",
            "<entry name=\"Step1\" inpkey=\"20\" outkey=\"30\"/><entry name=\"Cycle\" inpkey=\"21\" outkey=\"31\"/>",
        )
        .replace(
            "<vertex vertexkey=\"70\"><edges><edge vertexkey=\"80\"/></edges></vertex>",
            "<vertex vertexkey=\"70\"><edges><edge vertexkey=\"80\"/><edge vertexkey=\"21\"/></edges></vertex>",
        );
    write(&mapping, &cycle);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );

    let disconnected = original.replace(
        "</children><graph>",
        "<component name=\"detached\" library=\"xml\" kind=\"14\"><properties PassThrough=\"1\"/><data><root><entry name=\"Buffer1\"><entry name=\"Step1\" inpkey=\"91\" outkey=\"90\"/></entry></root><document schema=\"buffer1.xsd\" instanceroot=\"{}Buffer1\"/></data></component></children><graph>",
    );
    write(&mapping, &disconnected);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(
        error.contains("cannot classify component `detached`"),
        "{error}"
    );
}

#[test]
fn serial_xml_chain_rejects_more_than_64_intermediates_before_lowering() {
    let dir = TempDir::new();
    let mapping = write_serial_chain(&dir.0, 65);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(error.contains("at most 64 intermediate targets"), "{error}");
}

#[test]
fn three_serial_xml_stages_import_and_execute_in_graph_order() {
    let dir = TempDir::new();
    let mapping = write_three_stage_chain(&dir.0);
    let imported = mfd::import_pipeline(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages.len(), 3);
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    for (index, stage) in imported.pipeline.stages.iter().enumerate() {
        assert_eq!(stage.id, format!("mfd-stage-{}", index + 1));
        if index == 0 {
            assert_eq!(
                stage.source,
                mapping::PipelineInput::Host {
                    name: "source".into()
                }
            );
        } else {
            assert_eq!(
                stage.source,
                mapping::PipelineInput::StageTarget {
                    stage: format!("mfd-stage-{index}"),
                    target: None,
                }
            );
        }
    }
    let source = Instance::Group(vec![(
        "Start".into(),
        Instance::Scalar(Value::String("through all stages".into())),
    )]);
    let outputs = engine::run_pipeline(
        &imported.pipeline,
        &BTreeMap::from([("source".to_string(), source)]),
    )
    .unwrap();
    for (id, field) in [
        ("mfd-stage-1", "First"),
        ("mfd-stage-2", "Second"),
        ("mfd-stage-3", "Result"),
    ] {
        assert_eq!(
            outputs
                .stage(id)
                .unwrap()
                .primary
                .field(field)
                .and_then(Instance::as_scalar),
            Some(&Value::String("through all stages".into())),
            "{id}"
        );
    }
}

#[test]
fn three_stage_xml_binds_original_host_sources_in_later_stages() {
    let dir = TempDir::new();
    let mapping = write_three_stage_chain(&dir.0);
    write(
        &dir.0.join("supplement.xsd"),
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"Supplement\"><xs:complexType><xs:sequence><xs:element name=\"Addition\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>",
    );
    let modified = std::fs::read_to_string(&mapping)
        .unwrap()
        .replace(
            "</children><graph>",
            "<component name=\"supplement\" library=\"xml\" kind=\"14\"><data><root><entry name=\"Supplement\"><entry name=\"Addition\" outkey=\"70\"/></entry></root><document schema=\"supplement.xsd\" inputinstance=\"supplement.xml\" instanceroot=\"{}Supplement\"/></data></component></children><graph>",
        )
        .replace(
            "</vertices></graph>",
            "<vertex vertexkey=\"70\"><edges><edge vertexkey=\"41\"/><edge vertexkey=\"61\"/></edges></vertex></vertices></graph>",
        );
    write(&mapping, &modified);

    let imported = mfd::import_pipeline(&mapping).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages.len(), 3);
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    for stage in &imported.pipeline.stages[1..] {
        assert!(stage.extra_sources.iter().any(|binding| {
            binding.from
                == mapping::PipelineInput::Host {
                    name: "supplement".into(),
                }
        }));
    }
    let source = Instance::Group(vec![(
        "Start".into(),
        Instance::Scalar(Value::String("main".into())),
    )]);
    let supplement = Instance::Group(vec![(
        "Addition".into(),
        Instance::Scalar(Value::String("later host".into())),
    )]);
    let outputs = engine::run_pipeline(
        &imported.pipeline,
        &BTreeMap::from([("source".into(), source), ("supplement".into(), supplement)]),
    )
    .unwrap();
    assert_eq!(
        outputs
            .stage("mfd-stage-2")
            .unwrap()
            .primary
            .field("Skipped")
            .and_then(Instance::as_scalar),
        Some(&Value::String("later host".into()))
    );
    assert_eq!(
        outputs
            .stage("mfd-stage-3")
            .unwrap()
            .primary
            .field("Bypass")
            .and_then(Instance::as_scalar),
        Some(&Value::String("later host".into()))
    );
}

#[test]
fn three_stage_xml_rejects_bypass_and_cycle() {
    let dir = TempDir::new();
    let mapping = write_three_stage_chain(&dir.0);
    let original = std::fs::read_to_string(&mapping).unwrap();
    let bypass = original.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"61\"/></edges></vertex>",
    );
    write(&mapping, &bypass);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );

    let cycle = original.replace(
        "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/></edges></vertex>",
        "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/><edge vertexkey=\"21\"/></edges></vertex>",
    );
    write(&mapping, &cycle);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );
}

#[test]
fn connected_xml_passthrough_reports_unrepresented_chain() {
    let dir = TempDir::new();
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), schema);
    write(
        &dir.0.join("target.xsd"),
        &schema.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Value" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Source"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="source.xsd" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Value" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0].contains("chained target `buffer`"),
        "{:?}",
        imported.warnings
    );
    let strict = mfd::import_with_profile(
        &dir.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    );
    assert!(matches!(strict, Err(mfd::MfdError::IncompatibleImport(_))));
    assert!(imported.project.extra_sources.is_empty());
    assert_eq!(imported.project.target.name, "Target");
    let [passthrough] = imported.project.extra_targets.as_slice() else {
        panic!("expected the pass-through component as an additional target");
    };
    assert_eq!(passthrough.name, "buffer");
    assert_eq!(passthrough.schema.name, "Source");
    assert_eq!(passthrough.path.as_deref(), Some("buffer.xml"));
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Value".to_string(),
        Instance::Scalar(Value::String("passed through".to_string())),
    )]);
    let pipeline_import = mfd::import_pipeline(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        pipeline_import.warnings.is_empty(),
        "{:?}",
        pipeline_import.warnings
    );
    assert_eq!(pipeline_import.pipeline.stages.len(), 2);
    assert!(engine::validate_pipeline(&pipeline_import.pipeline).is_empty());
    let piped = engine::run_pipeline(
        &pipeline_import.pipeline,
        &BTreeMap::from([("source".to_string(), source.clone())]),
    )
    .unwrap();
    assert_eq!(
        piped
            .stage("mfd-stage-1")
            .unwrap()
            .primary
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("passed through".to_string()))
    );
    assert_eq!(
        piped
            .stage("mfd-stage-2")
            .unwrap()
            .primary
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("passed through".to_string()))
    );
    let outputs = engine::run_outputs(&imported.project, &source).unwrap();
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("passed through".to_string()))
    );
    let [passthrough] = outputs.extras.as_slice() else {
        panic!("expected one pass-through output");
    };
    assert_eq!(passthrough.name, "buffer");
    assert_eq!(
        passthrough
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("passed through".to_string()))
    );

    let roundtrip_path = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&roundtrip_path).unwrap();
    assert_eq!(exported.matches("XSLTDefaultOutput=\"1\"").count(), 1);

    let roundtrip = mfd::import(&roundtrip_path).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert_eq!(roundtrip.project.target.name, "Target");
    let [passthrough] = roundtrip.project.extra_targets.as_slice() else {
        panic!("expected one pass-through output after re-import");
    };
    assert_eq!(passthrough.name, "buffer");
    assert_eq!(passthrough.path.as_deref(), Some("buffer.xml"));
    assert!(engine::validate(&roundtrip.project).is_empty());

    let outputs = engine::run_outputs(&roundtrip.project, &source).unwrap();
    assert_eq!(outputs.extras.len(), 1);
    assert_eq!(
        outputs.primary.field("Value").and_then(Instance::as_scalar),
        outputs.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar)
    );

    // A connected intermediate output is insufficient when the final target
    // actually reads the original source instead.
    let mapping = dir.0.join("mapping.mfd");
    let disconnected = std::fs::read_to_string(&mapping)
        .unwrap()
        .replace(
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/><edge vertexkey=\"40\"/></edges></vertex>",
        )
        .replace(
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"50\"/></edges></vertex>",
        );
    write(&mapping, &disconnected);
    let error = mfd::import_pipeline(&mapping).err().unwrap().to_string();
    assert!(error.contains("unclassified component"), "{error}");
}

#[test]
fn local_reference_chain_reports_loss_when_samples_are_available() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples/Tutorial/BasicTutorials/Tut3-ChainedMapping.mfd");
    if !path.is_file() {
        return;
    }
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("chained target `MergedLibrary`")),
        "{:?}",
        imported.warnings
    );
    let pipeline = mfd::import_pipeline(&path).unwrap();
    assert!(pipeline.warnings.is_empty(), "{:?}", pipeline.warnings);
    assert_eq!(pipeline.pipeline.stages.len(), 2);
    assert!(engine::validate_pipeline(&pipeline.pipeline).is_empty());
    let first = &pipeline.pipeline.stages[0];
    let mut hosts = BTreeMap::new();
    let mapping::PipelineInput::Host { name } = &first.source else {
        panic!("first stage must read a file-host source");
    };
    let directory = path.parent().unwrap();
    hosts.insert(
        name.clone(),
        format_xml::read(
            &directory.join(format!("{name}.xml")),
            &first.project.source,
        )
        .unwrap(),
    );
    for binding in &first.extra_sources {
        let mapping::PipelineInput::Host { name } = &binding.from else {
            panic!("named source must read a file-host source");
        };
        let source = first
            .project
            .extra_sources
            .iter()
            .find(|source| source.name == binding.name)
            .unwrap();
        hosts.insert(
            name.clone(),
            format_xml::read(&directory.join(format!("{name}.xml")), &source.schema).unwrap(),
        );
    }
    let execution =
        engine::ExecutionContext::new(&path).with_current_datetime("2026-09-29T12:00:00-07:00");
    let outputs =
        engine::run_pipeline_with_context(&pipeline.pipeline, &hosts, &execution).unwrap();
    let merged = outputs.stage("mfd-stage-1").unwrap();
    let filtered = outputs.stage("mfd-stage-2").unwrap();
    let Instance::Repeated(merged_publications) = merged.primary.field("publication").unwrap()
    else {
        panic!("merged target has no publications");
    };
    let Instance::Repeated(filtered_publications) = filtered.primary.field("publication").unwrap()
    else {
        panic!("filtered target has no publications");
    };
    assert_eq!(merged_publications.len(), 7);
    assert_eq!(filtered_publications.len(), 1);
    assert_eq!(
        filtered_publications[0]
            .field("author")
            .and_then(Instance::as_scalar),
        Some(&Value::String("Franz Kafka".to_string()))
    );

    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    for relative in [
        "ChainedPersonList.mfd",
        "Tutorial/ChainedReports.mfd",
        "Tutorial/Tut-ExpReport-chain.mfd",
    ] {
        let sample = samples.join(relative);
        if sample.is_file() {
            let imported =
                mfd::import_pipeline(&sample).unwrap_or_else(|error| panic!("{relative}: {error}"));
            assert!(
                imported.warnings.is_empty(),
                "{relative}: {:?}",
                imported.warnings
            );
            assert!(
                engine::validate_pipeline(&imported.pipeline).is_empty(),
                "{relative}"
            );
        }
    }
}
