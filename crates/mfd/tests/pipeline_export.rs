use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::{Node, PipelineInput};

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

fn make_terminal_fanout(directory: &Path) -> PathBuf {
    let path = make_chain(directory);
    write_schema(&directory.join("secondary.xsd"), "Secondary", "Copy");
    let original = std::fs::read_to_string(&path).unwrap();
    let with_target = original.replace(
        "</children><graph>",
        r#"<component name="secondary" library="xml" kind="14"><data><root><entry name="Secondary"><entry name="Copy" inpkey="50"/></entry></root><document schema="secondary.xsd" outputinstance="secondary.xml" instanceroot="{}Secondary"/></data></component></children><graph>"#,
    );
    let with_fanout = with_target.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
    );
    assert_ne!(with_fanout, original);
    std::fs::write(&path, with_fanout).unwrap();
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

fn make_late_named_source_chain(directory: &Path) -> PathBuf {
    let path = make_chain(directory);
    write_schema(&directory.join("catalog.xsd"), "Catalog", "Item");
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/><xs:element name="Lookup" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let with_catalog = original.replace(
        "<component name=\"buffer\"",
        r#"<component name="catalog" library="xml" kind="14"><data><root><entry name="Catalog"><entry name="Item" outkey="50"/></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component><component name="buffer""#,
    );
    let with_target = with_catalog.replace(
        "<entry name=\"Result\" inpkey=\"40\"/>",
        "<entry name=\"Result\" inpkey=\"40\"/><entry name=\"Lookup\" inpkey=\"60\"/>",
    );
    let with_edge = with_target.replace(
        "</vertices></graph>",
        "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/></edges></vertex></vertices></graph>",
    );
    assert_ne!(with_edge, original);
    std::fs::write(&path, with_edge).unwrap();
    path
}

fn make_four_stage_late_named_source_chain(directory: &Path, late_stage: usize) -> PathBuf {
    let path = make_four_stage_chain(directory);
    write_schema(&directory.join("catalog.xsd"), "Catalog", "Item");
    let (schema_path, root, field, entry) = match late_stage {
        3 => (
            "buffer3.xsd",
            "Buffer3",
            "Step3",
            "<entry name=\"Step3\" inpkey=\"60\" outkey=\"70\"/>",
        ),
        4 => (
            "target.xsd",
            "Target",
            "Result",
            "<entry name=\"Result\" inpkey=\"80\"/>",
        ),
        _ => panic!("late source fixture needs stage three or four"),
    };
    std::fs::write(
        directory.join(schema_path),
        format!(
            "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/><xs:element name=\"Lookup\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
        ),
    )
    .unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let with_catalog = original.replace(
        "<component name=\"buffer-3\"",
        r#"<component name="catalog" library="xml" kind="14"><data><root><entry name="Catalog"><entry name="Item" outkey="90"/></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component><component name="buffer-3""#,
    );
    let lookup_entry = if late_stage == 3 {
        "<entry name=\"Lookup\" inpkey=\"100\" outkey=\"110\"/>"
    } else {
        "<entry name=\"Lookup\" inpkey=\"100\"/>"
    };
    let with_lookup = with_catalog.replace(entry, &format!("{entry}{lookup_entry}"));
    let with_edge = with_lookup.replace(
        "</vertices></graph>",
        "<vertex vertexkey=\"90\"><edges><edge vertexkey=\"100\"/></edges></vertex></vertices></graph>",
    );
    assert_ne!(with_edge, original);
    std::fs::write(&path, with_edge).unwrap();
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

fn late_named_inputs(pipeline: &mapping::Pipeline) -> BTreeMap<String, Instance> {
    let PipelineInput::Host { name: primary } = &pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    let PipelineInput::Host { name: catalog } = &pipeline.stages[0].extra_sources[0].from else {
        panic!("named source must read an original host");
    };
    BTreeMap::from([
        (
            primary.clone(),
            Instance::Group(vec![(
                "Start".into(),
                Instance::Scalar(Value::String("serial value".into())),
            )]),
        ),
        (
            catalog.clone(),
            Instance::Group(vec![(
                "Item".into(),
                Instance::Scalar(Value::String("host value".into())),
            )]),
        ),
    ])
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
fn late_named_xml_source_reuses_one_original_host_component() {
    let directory = TempDir::new();
    let original = mfd::import_pipeline(&make_late_named_source_chain(&directory.0)).unwrap();
    assert!(original.warnings.is_empty(), "{:?}", original.warnings);
    assert_eq!(original.pipeline.stages.len(), 2);
    assert_eq!(
        original.pipeline.stages[0].project.extra_sources[0].name,
        "catalog"
    );
    assert!(
        original.pipeline.stages[1]
            .extra_sources
            .iter()
            .any(|binding| {
                binding.name == "catalog"
                    && matches!(&binding.from, PipelineInput::Host { name } if name == "catalog")
            })
    );
    let original_outputs =
        engine::run_pipeline(&original.pipeline, &late_named_inputs(&original.pipeline)).unwrap();
    let original_final = &original_outputs.stage("mfd-stage-2").unwrap().primary;
    assert_eq!(
        original_final.field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("serial value".into()))
    );
    assert_eq!(
        original_final.field("Lookup").and_then(Instance::as_scalar),
        Some(&Value::String("host value".into()))
    );

    let exported_path = directory.0.join("late-host-export.mfd");
    let report = mfd::preflight_pipeline_export(&original.pipeline, &exported_path).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let published = mfd::export_pipeline_with_profile(
        &original.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    assert!(published.is_native_compatible(), "{published:?}");
    let exported = std::fs::read_to_string(&exported_path).unwrap();
    assert_eq!(
        exported.matches("name=\"catalog\" library=\"xml\"").count(),
        1
    );
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
    let reimported_outputs = engine::run_pipeline(
        &reimported.pipeline,
        &late_named_inputs(&reimported.pipeline),
    )
    .unwrap();
    assert_eq!(
        reimported_outputs.stage("mfd-stage-2").unwrap().primary,
        *original_final
    );
}

#[test]
fn four_stage_chain_reuses_unused_original_host_in_intermediate_or_final_stage() {
    for late_stage in [3, 4] {
        let directory = TempDir::new();
        let original = mfd::import_pipeline(&make_four_stage_late_named_source_chain(
            &directory.0,
            late_stage,
        ))
        .unwrap();
        assert!(original.warnings.is_empty(), "{:?}", original.warnings);
        assert_eq!(original.pipeline.stages.len(), 4);
        assert!(original.pipeline.stages[late_stage - 1]
            .extra_sources
            .iter()
            .any(|binding| {
                binding.name == "catalog"
                    && matches!(&binding.from, PipelineInput::Host { name } if name == "catalog")
            }));
        let inputs = late_named_inputs(&original.pipeline);
        let original_outputs = engine::run_pipeline(&original.pipeline, &inputs).unwrap();
        let named_output = &original_outputs
            .stage(&format!("mfd-stage-{late_stage}"))
            .unwrap()
            .primary;
        assert_eq!(
            named_output.field("Lookup").and_then(Instance::as_scalar),
            Some(&Value::String("host value".into()))
        );

        let exported_path = directory
            .0
            .join(format!("late-host-stage-{late_stage}.mfd"));
        let report = mfd::preflight_pipeline_export(&original.pipeline, &exported_path).unwrap();
        assert!(report.is_native_compatible(), "{report:?}");
        let published = mfd::export_pipeline_with_profile(
            &original.pipeline,
            &exported_path,
            mfd::ExportProfile::NativeMfd,
        )
        .unwrap();
        assert!(published.is_native_compatible(), "{published:?}");
        let exported = std::fs::read_to_string(&exported_path).unwrap();
        assert_eq!(
            exported.matches("name=\"catalog\" library=\"xml\"").count(),
            1
        );
        let reimported = mfd::import_pipeline(&exported_path).unwrap();
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
        let reimported_outputs = engine::run_pipeline(
            &reimported.pipeline,
            &late_named_inputs(&reimported.pipeline),
        )
        .unwrap();
        for index in 0..4 {
            let original_id = &original.pipeline.stages[index].id;
            let reimported_id = &reimported.pipeline.stages[index].id;
            assert_eq!(
                original_outputs.stage(original_id).unwrap().primary,
                reimported_outputs.stage(reimported_id).unwrap().primary,
                "late source at stage {late_stage}, output stage {}",
                index + 1,
            );
        }
    }
}

#[test]
fn four_stage_late_host_rejects_changed_boundary_or_earlier_use_without_publishing() {
    let directory = TempDir::new();
    let imported =
        mfd::import_pipeline(&make_four_stage_late_named_source_chain(&directory.0, 4)).unwrap();

    let mut changed = imported.pipeline.clone();
    changed.stages[3].project.extra_sources[0].path = "different.xml".into();
    let destination = directory.0.join("not-created/changed.mfd");
    let error = mfd::export_pipeline(&changed, &destination)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("does not match its original XML boundary"),
        "{error}"
    );
    assert!(!destination.parent().unwrap().exists());

    let mut used = imported.pipeline;
    let second = &mut used.stages[1].project;
    let node = second.graph.nodes.keys().next_back().copied().unwrap_or(0) + 1;
    second.graph.nodes.insert(
        node,
        Node::SourceField {
            path: vec!["catalog".into(), "Item".into()],
            frame: None,
        },
    );
    second.root.bindings[0].node = node;
    let destination = directory.0.join("not-created/used.mfd");
    let error = mfd::export_pipeline(&used, &destination)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("already connected before this stage"),
        "{error}"
    );
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn late_named_xml_source_rejects_changed_or_previously_used_host() {
    let directory = TempDir::new();
    let imported = mfd::import_pipeline(&make_late_named_source_chain(&directory.0)).unwrap();

    let mut changed = imported.pipeline.clone();
    changed.stages[1]
        .project
        .extra_sources
        .iter_mut()
        .find(|source| source.name == "catalog")
        .unwrap()
        .path = "different.xml".into();
    let destination = directory.0.join("not-created/changed.mfd");
    let error = mfd::export_pipeline(&changed, &destination)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("does not match its original XML boundary"),
        "{error}"
    );
    assert!(!destination.parent().unwrap().exists());

    let mut used = imported.pipeline;
    let first = &mut used.stages[0].project;
    let node = first.graph.nodes.keys().next_back().copied().unwrap_or(0) + 1;
    first.graph.nodes.insert(
        node,
        Node::SourceField {
            path: vec!["catalog".into(), "Item".into()],
            frame: None,
        },
    );
    first.root.bindings[0].node = node;
    let destination = directory.0.join("not-created/used.mfd");
    let error = mfd::export_pipeline(&used, &destination)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("already connected before this stage"),
        "{error}"
    );
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn synthetic_terminal_xml_fanout_imports_exports_and_runs() {
    let directory = TempDir::new();
    let source_path = make_terminal_fanout(&directory.0);
    let original = mfd::import_pipeline(&source_path).unwrap();
    assert!(original.warnings.is_empty(), "{:?}", original.warnings);
    assert_eq!(original.pipeline.stages.len(), 2);
    let [secondary] = original.pipeline.stages[1].project.extra_targets.as_slice() else {
        panic!("expected terminal named target");
    };
    assert_eq!(secondary.name, "secondary");
    assert_eq!(secondary.path.as_deref(), Some("secondary.xml"));
    let original_outputs = execute(&original.pipeline);
    let final_output = original_outputs.stage("mfd-stage-2").unwrap();
    assert_eq!(final_output.extras.len(), 1);
    assert_eq!(final_output.extras[0].name, "secondary");
    assert_eq!(
        final_output.extras[0]
            .instance
            .field("Copy")
            .and_then(Instance::as_scalar),
        Some(&Value::String("serial value".into()))
    );

    let exported_path = directory.0.join("fanout-export.mfd");
    let report = mfd::preflight_pipeline_export(&original.pipeline, &exported_path).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(
        mfd::export_pipeline(&original.pipeline, &exported_path)
            .unwrap()
            .is_empty()
    );
    let exported = std::fs::read_to_string(&exported_path).unwrap();
    assert_eq!(exported.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(exported.matches("XSLTDefaultOutput=\"1\"").count(), 1);
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
    let reimported_outputs = execute(&reimported.pipeline);
    let reimported_final = reimported_outputs.stage("mfd-stage-2").unwrap();
    assert_eq!(final_output.primary, reimported_final.primary);
    assert_eq!(final_output.extras.len(), reimported_final.extras.len());
    assert_eq!(
        final_output.extras[0].instance,
        reimported_final.extras[0].instance
    );

    // A secondary target reading the original host would bypass the last
    // intermediate, so it cannot be represented as this stage's named output.
    let bypass = std::fs::read_to_string(&source_path)
        .unwrap()
        .replace(
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        )
        .replace(
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/><edge vertexkey=\"50\"/></edges></vertex>",
        );
    std::fs::write(&source_path, bypass).unwrap();
    let error = mfd::import_pipeline(&source_path)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );
}

#[test]
fn disconnected_terminal_target_rejects_before_publishing() {
    let directory = TempDir::new();
    let mut pipeline = mfd::import_pipeline(&make_terminal_fanout(&directory.0))
        .unwrap()
        .pipeline;
    pipeline.stages[1].project.extra_targets[0]
        .root
        .bindings
        .clear();
    let destination = directory.0.join("not-created").join("disconnected.mfd");
    let error = mfd::export_pipeline(&pipeline, &destination)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("pipeline export graph is not supported"),
        "{error}"
    );
    assert!(!destination.exists());
    assert!(!destination.parent().unwrap().exists());
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
