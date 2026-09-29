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
    assert!(
        error.contains("does not feed the selected final target"),
        "{error}"
    );
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
