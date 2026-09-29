use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::PipelineInput;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_pipeline_export_{}_{}",
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

fn write_schema(path: &Path, root: &str, field: &str) {
    std::fs::write(
        path,
        format!(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
        ),
    )
    .unwrap();
}

fn make_chain(directory: &Path) -> PathBuf {
    write_schema(&directory.join("source.xsd"), "Source", "Start");
    write_schema(&directory.join("buffer.xsd"), "Buffer", "Value");
    write_schema(&directory.join("target.xsd"), "Target", "Result");
    let path = directory.join("source-chain.mfd");
    std::fs::write(
        &path,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Value" inpkey="20" outkey="30"/></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    path
}

fn make_four_stage_chain(directory: &Path) -> PathBuf {
    write_schema(&directory.join("source.xsd"), "Source", "Start");
    write_schema(&directory.join("target.xsd"), "Target", "Result");
    let mut children = String::from(
        r#"<component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Start" outkey="10"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>"#,
    );
    for index in (1..=3).rev() {
        let root = format!("Buffer{index}");
        let field = format!("Step{index}");
        write_schema(&directory.join(format!("buffer{index}.xsd")), &root, &field);
        children.push_str(&format!(
            "<component name=\"buffer-{index}\" library=\"xml\" kind=\"14\"><properties PassThrough=\"1\"/><data><root><entry name=\"{root}\"><entry name=\"{field}\" inpkey=\"{}\" outkey=\"{}\"/></entry></root><document schema=\"buffer{index}.xsd\" instanceroot=\"{{}}{root}\"/></data></component>",
            index * 20,
            index * 20 + 10,
        ));
    }
    children.push_str(
        r#"<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="80"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>"#,
    );
    let vertices = (0..=3)
        .map(|index| {
            format!(
                "<vertex vertexkey=\"{}\"><edges><edge vertexkey=\"{}\"/></edges></vertex>",
                index * 20 + 10,
                (index + 1) * 20,
            )
        })
        .collect::<String>();
    let path = directory.join("source-four-stage-chain.mfd");
    std::fs::write(
        &path,
        format!(
            "<mapping version=\"26\"><component name=\"map\"><structure><children>{children}</children><graph><vertices>{vertices}</vertices></graph></structure></component></mapping>"
        ),
    )
    .unwrap();
    path
}

fn execute(pipeline: &mapping::Pipeline) -> engine::PipelineOutputs {
    let PipelineInput::Host { name } = &pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    engine::run_pipeline(
        pipeline,
        &BTreeMap::from([(
            name.clone(),
            Instance::Group(vec![(
                "Start".into(),
                Instance::Scalar(Value::String("serial value".into())),
            )]),
        )]),
    )
    .unwrap()
}

#[test]
fn serial_xml_pipeline_exports_one_design_and_preserves_execution() {
    let directory = TempDir::new();
    let original = mfd::import_pipeline(&make_chain(&directory.0)).unwrap();
    assert!(original.warnings.is_empty(), "{:?}", original.warnings);
    let original_outputs = execute(&original.pipeline);
    let exported_path = directory.0.join("exported.mfd");
    let report = mfd::preflight_pipeline_export(&original.pipeline, &exported_path).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(!exported_path.exists());
    let published = mfd::export_pipeline_with_profile(
        &original.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    assert!(published.is_native_compatible(), "{published:?}");
    let encoded = std::fs::read_to_string(&exported_path).unwrap();
    assert!(encoded.contains("PassThrough=\"1\""));
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
    let reimported_outputs = execute(&reimported.pipeline);
    for index in 0..2 {
        let original_stage = &original.pipeline.stages[index];
        let reimported_stage = &reimported.pipeline.stages[index];
        assert_eq!(
            original_outputs.stage(&original_stage.id).unwrap().primary,
            reimported_outputs
                .stage(&reimported_stage.id)
                .unwrap()
                .primary
        );
    }
}

#[test]
fn four_stage_pipeline_remaps_every_stage_and_preserves_execution() {
    let directory = TempDir::new();
    let original = mfd::import_pipeline(&make_four_stage_chain(&directory.0)).unwrap();
    assert!(original.warnings.is_empty(), "{:?}", original.warnings);
    assert_eq!(original.pipeline.stages.len(), 4);
    let original_outputs = execute(&original.pipeline);
    let exported_path = directory.0.join("exported-four-stage.mfd");
    assert!(
        mfd::export_pipeline(&original.pipeline, &exported_path)
            .unwrap()
            .is_empty()
    );
    let exported = std::fs::read_to_string(&exported_path).unwrap();
    assert_eq!(exported.matches("PassThrough=\"1\"").count(), 3);
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.pipeline.stages.len(), 4);
    let reimported_outputs = execute(&reimported.pipeline);
    for index in 0..4 {
        let original_stage = &original.pipeline.stages[index];
        let reimported_stage = &reimported.pipeline.stages[index];
        assert_eq!(
            original_outputs.stage(&original_stage.id).unwrap().primary,
            reimported_outputs
                .stage(&reimported_stage.id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1,
        );
    }
}

#[test]
fn nonserial_pipeline_rejects_without_publishing() {
    let directory = TempDir::new();
    let mut pipeline = mfd::import_pipeline(&make_chain(&directory.0))
        .unwrap()
        .pipeline;
    pipeline.stages[1].source = PipelineInput::Host {
        name: "second-host".into(),
    };
    let destination = directory.0.join("not-created").join("rejected.mfd");
    let error = mfd::export_pipeline(&pipeline, &destination)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("serial primary-target chain"), "{error}");
    assert!(!destination.exists());
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn late_unused_host_input_rejects_without_publishing() {
    let directory = TempDir::new();
    let mut pipeline = mfd::import_pipeline(&make_chain(&directory.0))
        .unwrap()
        .pipeline;
    let binding = pipeline.stages[1].extra_sources.first_mut().unwrap();
    binding.from = PipelineInput::Host {
        name: "new-unused-host".into(),
    };
    let destination = directory.0.join("not-created").join("rejected.mfd");
    let error = mfd::export_pipeline(&pipeline, &destination)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("introduces host input"), "{error}");
    assert!(!destination.exists());
    assert!(!destination.parent().unwrap().exists());
}
