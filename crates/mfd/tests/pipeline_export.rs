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

fn make_chain_with_intermediate_output(directory: &Path) -> PathBuf {
    let path = make_chain(directory);
    let original = std::fs::read_to_string(&path).unwrap();
    let marked = original.replace(
        "<document schema=\"buffer.xsd\" instanceroot=\"{}Buffer\"/>",
        "<document schema=\"buffer.xsd\" inputinstance=\"buffer-preview.xml\" outputinstance=\"buffer.xml\" instanceroot=\"{}Buffer\"/>",
    );
    assert_ne!(marked, original);
    std::fs::write(&path, marked).unwrap();
    path
}

fn make_csv_final_chain(directory: &Path) -> PathBuf {
    let path = make_chain(directory);
    let original = std::fs::read_to_string(&path).unwrap();
    let xml_target = r#"<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>"#;
    let csv_target = r#"<component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows"><entry name="Result" inpkey="40"/></entry></entry></entry></root><text type="csv" outputinstance="target.csv"><settings separator=";" quote="&quot;" firstrownames="false"><names root="Target" block="Rows"><field0 name="Result" type="string"/></names></settings></text></data></component>"#;
    let csv_chain = original.replace(xml_target, csv_target);
    assert_ne!(csv_chain, original);
    std::fs::write(&path, csv_chain).unwrap();
    path
}

fn make_repeated_csv_final_chain(directory: &Path) -> PathBuf {
    for (file, root) in [("source.xsd", "Source"), ("buffer.xsd", "Buffer")] {
        std::fs::write(
            directory.join(file),
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"Row\" maxOccurs=\"unbounded\"><xs:complexType><xs:sequence><xs:element name=\"Value\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"
            ),
        )
        .unwrap();
    }
    let path = directory.join("source-repeated-csv-chain.mfd");
    std::fs::write(
        &path,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Value" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="buffer" library="xml" kind="14"><properties PassThrough="1"/><data><root><entry name="Buffer"><entry name="Row" inpkey="20" outkey="30"><entry name="Value" inpkey="21" outkey="31"/></entry></entry></root><document schema="buffer.xsd" instanceroot="{}Buffer"/></data></component>
          <component name="target" library="text" kind="16"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Rows" inpkey="40"><entry name="Result" inpkey="41"/></entry></entry></entry></root><text type="csv" outputinstance="target.csv"><settings separator=";" quote="&quot;" firstrownames="true"><names root="Target" block="Rows"><field0 name="Result" type="string"/></names></settings></text></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="31"><edges><edge vertexkey="41"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )
    .unwrap();
    path
}

fn make_csv_final_with_xml_named_target(directory: &Path) -> PathBuf {
    let path = make_csv_final_chain(directory);
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

fn make_four_stage_repeated_host_chain(directory: &Path, same_port: bool) -> PathBuf {
    let path = make_four_stage_chain(directory);
    std::fs::write(
        directory.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Item" type="xs:string"/><xs:element name="Other" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    for (schema_path, root, field) in [
        ("buffer2.xsd", "Buffer2", "Step2"),
        ("target.xsd", "Target", "Result"),
    ] {
        std::fs::write(
            directory.join(schema_path),
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{field}\" type=\"xs:string\"/><xs:element name=\"Lookup\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:schema>"
            ),
        )
        .unwrap();
    }
    let original = std::fs::read_to_string(&path).unwrap();
    let with_catalog = original.replace(
        "<component name=\"buffer-3\"",
        r#"<component name="catalog" library="xml" kind="14"><data><root><entry name="Catalog"><entry name="Item" outkey="90"/><entry name="Other" outkey="91"/></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component><component name="buffer-3""#,
    );
    let with_early = with_catalog.replace(
        "<entry name=\"Step2\" inpkey=\"40\" outkey=\"50\"/>",
        "<entry name=\"Step2\" inpkey=\"40\" outkey=\"50\"/><entry name=\"Lookup\" inpkey=\"100\" outkey=\"110\"/>",
    );
    let with_late = with_early.replace(
        "<entry name=\"Result\" inpkey=\"80\"/>",
        "<entry name=\"Result\" inpkey=\"80\"/><entry name=\"Lookup\" inpkey=\"120\"/>",
    );
    let catalog_vertices = if same_port {
        "<vertex vertexkey=\"90\"><edges><edge vertexkey=\"100\"/><edge vertexkey=\"120\"/></edges></vertex>"
    } else {
        "<vertex vertexkey=\"90\"><edges><edge vertexkey=\"100\"/></edges></vertex><vertex vertexkey=\"91\"><edges><edge vertexkey=\"120\"/></edges></vertex>"
    };
    let with_edges = with_late.replace(
        "</vertices></graph>",
        &format!("{catalog_vertices}</vertices></graph>"),
    );
    assert_ne!(with_edges, original);
    std::fs::write(&path, with_edges).unwrap();
    path
}

fn make_first_and_late_named_source_chain(directory: &Path) -> PathBuf {
    let path = make_late_named_source_chain(directory);
    std::fs::write(
        directory.join("buffer.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Buffer"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Lookup" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let with_buffer = original.replace(
        "<entry name=\"Value\" inpkey=\"20\" outkey=\"30\"/>",
        "<entry name=\"Value\" inpkey=\"20\" outkey=\"30\"/><entry name=\"Lookup\" inpkey=\"70\" outkey=\"80\"/>",
    );
    let with_fanout = with_buffer.replace(
        "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/></edges></vertex>",
        "<vertex vertexkey=\"50\"><edges><edge vertexkey=\"60\"/><edge vertexkey=\"70\"/></edges></vertex>",
    );
    assert_ne!(with_fanout, original);
    std::fs::write(&path, with_fanout).unwrap();
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

fn repeated_host_inputs(pipeline: &mapping::Pipeline) -> BTreeMap<String, Instance> {
    let mut inputs = late_named_inputs(pipeline);
    let PipelineInput::Host { name: catalog } = &pipeline.stages[0].extra_sources[0].from else {
        panic!("named source must read an original host");
    };
    inputs.insert(
        catalog.clone(),
        Instance::Group(vec![
            (
                "Item".into(),
                Instance::Scalar(Value::String("host item".into())),
            ),
            (
                "Other".into(),
                Instance::Scalar(Value::String("host other".into())),
            ),
        ]),
    );
    inputs
}

fn assert_catalog_item_fanout(exported: &str, expected_edges: usize) {
    let document = roxmltree::Document::parse(exported).unwrap();
    let catalog_components = document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("name") == Some("catalog"))
        .collect::<Vec<_>>();
    assert_eq!(catalog_components.len(), 1);
    let item_key = catalog_components[0]
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("Item"))
        .and_then(|node| node.attribute("outkey"))
        .unwrap();
    let item_vertices = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex") && node.attribute("vertexkey") == Some(item_key))
        .collect::<Vec<_>>();
    assert_eq!(item_vertices.len(), 1);
    assert_eq!(
        item_vertices[0]
            .descendants()
            .filter(|node| node.has_tag_name("edge"))
            .count(),
        expected_edges
    );
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
fn serial_pipeline_preserves_intermediate_output_instance_and_execution() {
    let directory = TempDir::new();
    let imported = mfd::import_pipeline(&make_chain_with_intermediate_output(&directory.0))
        .expect("connected intermediate output imports");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.pipeline.stages[0].project.target_path.as_deref(),
        Some("buffer.xml")
    );
    assert_eq!(
        imported.pipeline.stages[1].project.source_path.as_deref(),
        Some("buffer-preview.xml")
    );
    let before = execute(&imported.pipeline);

    let exported_path = directory.0.join("materialized-intermediate.mfd");
    let report = mfd::preflight_pipeline_export(&imported.pipeline, &exported_path).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(!exported_path.exists());
    mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    let exported = std::fs::read_to_string(&exported_path).unwrap();
    let document = roxmltree::Document::parse(&exported).unwrap();
    let intermediate = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .expect("one pass-through component");
    assert_eq!(
        intermediate
            .descendants()
            .find(|node| node.has_tag_name("document"))
            .and_then(|node| node.attribute("outputinstance")),
        Some("buffer.xml")
    );
    assert_eq!(
        intermediate
            .descendants()
            .find(|node| node.has_tag_name("document"))
            .and_then(|node| node.attribute("inputinstance")),
        Some("buffer-preview.xml")
    );

    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.pipeline.stages[0].project.target_path.as_deref(),
        Some("buffer.xml")
    );
    assert_eq!(
        reimported.pipeline.stages[1].project.source_path.as_deref(),
        Some("buffer-preview.xml")
    );
    let after = execute(&reimported.pipeline);
    for index in 0..2 {
        assert_eq!(
            before
                .stage(&imported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary
        );
    }

    let mut unsupported = imported.pipeline;
    unsupported.stages[0].project.target_path = Some("buffer.csv".into());
    let rejected_path = directory.0.join("not-created/rejected.mfd");
    let error = mfd::export_pipeline(&unsupported, &rejected_path)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!rejected_path.parent().unwrap().exists());

    let mut unsupported = reimported.pipeline;
    unsupported.stages[1].project.source_path = Some("buffer-preview.csv".into());
    let rejected_path = directory.0.join("not-created/source-rejected.mfd");
    let error = mfd::export_pipeline(&unsupported, &rejected_path)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!rejected_path.parent().unwrap().exists());
}

#[test]
fn local_chain_retains_intermediate_preview_and_output_instances_when_available() {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples/Tutorial/ChainedReports.mfd");
    if !sample.is_file() {
        return;
    }
    let imported = mfd::import_pipeline(&sample).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.pipeline.stages[0].project.target_path.as_deref(),
        Some("ReportB.xml")
    );
    assert_eq!(
        imported.pipeline.stages[1].project.source_path.as_deref(),
        Some("ReportB.xml")
    );
    let directory = TempDir::new();
    let destination = directory.0.join("reports-chain.mfd");
    let report = mfd::preflight_pipeline_export(&imported.pipeline, &destination).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    let reimported = mfd::import_pipeline(&destination).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.pipeline.stages[0].project.target_path.as_deref(),
        Some("ReportB.xml")
    );
    assert_eq!(
        reimported.pipeline.stages[1].project.source_path.as_deref(),
        Some("ReportB.xml")
    );
}

#[test]
fn synthetic_xml_chain_with_csv_final_target_imports_executes_and_roundtrips() {
    let directory = TempDir::new();
    let imported = mfd::import_pipeline(&make_csv_final_chain(&directory.0)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages.len(), 2);
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    let final_stage = &imported.pipeline.stages[1];
    assert_eq!(
        final_stage.project.target_path.as_deref(),
        Some("target.csv")
    );
    assert_eq!(final_stage.project.target_options.delimiter, Some(';'));
    assert_eq!(
        final_stage.project.target_options.has_header_row,
        Some(false)
    );
    let original_outputs = execute(&imported.pipeline);
    let final_instance = &original_outputs.stage(&final_stage.id).unwrap().primary;
    let rows = final_instance.as_repeated().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].field("Result").and_then(Instance::as_scalar),
        Some(&Value::String("serial value".into())),
        "{final_instance:?}"
    );
    let csv_path = directory.0.join("result.csv");
    let options = &final_stage.project.target_options;
    format_csv::write_with_dialect(
        &csv_path,
        &final_stage.project.target,
        rows,
        options.delimiter,
        options.csv_quote,
        options.csv_quote_disabled,
        options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&csv_path).unwrap(),
        "serial value\n"
    );
    let read_back = format_csv::read_with_dialect(
        &csv_path,
        &final_stage.project.target,
        options.delimiter,
        options.csv_quote,
        options.csv_quote_disabled,
        options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    assert_eq!(read_back, rows);

    let exported_path = directory.0.join("csv-final-export.mfd");
    let report = mfd::preflight_pipeline_export(&imported.pipeline, &exported_path).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(!exported_path.exists());
    let published = mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    assert!(published.is_native_compatible(), "{published:?}");
    let exported = std::fs::read_to_string(&exported_path).unwrap();
    assert_eq!(exported.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(exported.matches("library=\"text\"").count(), 1);
    assert!(exported.contains("<text type=\"csv\""));
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
    let reimported_outputs = execute(&reimported.pipeline);
    for index in 0..2 {
        let original_stage = &imported.pipeline.stages[index];
        let reimported_stage = &reimported.pipeline.stages[index];
        assert_eq!(
            original_outputs.stage(&original_stage.id).unwrap().primary,
            reimported_outputs
                .stage(&reimported_stage.id)
                .unwrap()
                .primary,
        );
    }
}

#[test]
fn repeated_xml_rows_write_header_and_quoted_csv_after_pipeline_roundtrip() {
    let directory = TempDir::new();
    let imported = mfd::import_pipeline(&make_repeated_csv_final_chain(&directory.0)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate_pipeline(&imported.pipeline).is_empty());
    let PipelineInput::Host { name } = &imported.pipeline.stages[0].source else {
        panic!("first stage must read a host source");
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(
            ["alpha;beta", "say \"hi\""]
                .into_iter()
                .map(|value| {
                    Instance::Group(vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String(value.into())),
                    )])
                })
                .collect(),
        ),
    )]);
    let input = BTreeMap::from([(name.clone(), source.clone())]);
    let original_outputs = engine::run_pipeline(&imported.pipeline, &input).unwrap();
    let final_stage = &imported.pipeline.stages[1];
    assert_eq!(final_stage.project.target_options.delimiter, Some(';'));
    assert_eq!(final_stage.project.target_options.csv_quote, None);
    assert_eq!(
        final_stage.project.target_options.has_header_row,
        Some(true)
    );
    let original_rows = original_outputs
        .stage(&final_stage.id)
        .unwrap()
        .primary
        .as_repeated()
        .unwrap();
    assert_eq!(original_rows.len(), 2);
    for (row, expected) in original_rows.iter().zip(["alpha;beta", "say \"hi\""]) {
        assert_eq!(
            row.field("Result").and_then(Instance::as_scalar),
            Some(&Value::String(expected.into())),
        );
    }
    let csv_path = directory.0.join("rows.csv");
    let options = &final_stage.project.target_options;
    format_csv::write_with_dialect(
        &csv_path,
        &final_stage.project.target,
        original_rows,
        options.delimiter,
        options.csv_quote,
        options.csv_quote_disabled,
        options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&csv_path).unwrap(),
        "Result\n\"alpha;beta\"\n\"say \"\"hi\"\"\"\n"
    );
    let read_back = format_csv::read_with_dialect(
        &csv_path,
        &final_stage.project.target,
        options.delimiter,
        options.csv_quote,
        options.csv_quote_disabled,
        options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    assert_eq!(read_back, original_rows);

    let exported_path = directory.0.join("repeated-csv-export.mfd");
    let preflight = mfd::preflight_pipeline_export(&imported.pipeline, &exported_path).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported_path.exists());
    let published = mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    assert!(published.is_native_compatible(), "{published:?}");
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let reimported_input = BTreeMap::from([(name.clone(), source)]);
    let reimported_outputs = engine::run_pipeline(&reimported.pipeline, &reimported_input).unwrap();
    for index in 0..2 {
        assert_eq!(
            original_outputs
                .stage(&imported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            reimported_outputs
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
        );
    }
    let reimported_stage = &reimported.pipeline.stages[1];
    let reimported_rows = reimported_outputs
        .stage(&reimported_stage.id)
        .unwrap()
        .primary
        .as_repeated()
        .unwrap();
    let reimported_csv_path = directory.0.join("reimported-rows.csv");
    let options = &reimported_stage.project.target_options;
    format_csv::write_with_dialect(
        &reimported_csv_path,
        &reimported_stage.project.target,
        reimported_rows,
        options.delimiter,
        options.csv_quote,
        options.csv_quote_disabled,
        options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(reimported_csv_path).unwrap(),
        std::fs::read_to_string(csv_path).unwrap(),
    );
}

#[test]
fn csv_primary_may_keep_a_connected_xml_named_target() {
    let directory = TempDir::new();
    let imported =
        mfd::import_pipeline(&make_csv_final_with_xml_named_target(&directory.0)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages[1].project.extra_targets.len(), 1);
    let original_outputs = execute(&imported.pipeline);
    let exported_path = directory.0.join("csv-and-xml-export.mfd");
    let report = mfd::preflight_pipeline_export(&imported.pipeline, &exported_path).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let reimported_outputs = execute(&reimported.pipeline);
    let original_final = original_outputs.stage("mfd-stage-2").unwrap();
    let reimported_final = reimported_outputs.stage("mfd-stage-2").unwrap();
    assert_eq!(original_final.primary, reimported_final.primary);
    assert_eq!(original_final.extras, reimported_final.extras);
}

#[test]
fn csv_pipeline_rejects_non_xml_intermediate_and_non_csv_final_without_artifacts() {
    let directory = TempDir::new();
    let imported = mfd::import_pipeline(&make_csv_final_chain(&directory.0)).unwrap();

    let mut intermediate_csv = imported.pipeline.clone();
    intermediate_csv.stages[0].project.target_path = Some("buffer.csv".into());
    let destination = directory.0.join("not-created/intermediate.mfd");
    let error = mfd::export_pipeline(&intermediate_csv, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut final_xlsx = imported.pipeline;
    final_xlsx.stages[1].project.target_path = Some("target.xlsx".into());
    let destination = directory.0.join("not-created/final.mfd");
    let error = mfd::export_pipeline(&final_xlsx, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut named_csv = mfd::import_pipeline(&make_csv_final_chain(&directory.0))
        .unwrap()
        .pipeline;
    let final_project = &mut named_csv.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.csv".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: final_project.root.clone(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    let error = mfd::export_pipeline(&named_csv, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn csv_pipeline_import_rejects_non_csv_text_terminal() {
    let directory = TempDir::new();
    let path = make_csv_final_chain(&directory.0);
    let original = std::fs::read_to_string(&path).unwrap();
    let unsupported = original.replace("<text type=\"csv\"", "<text type=\"fixed-length\"");
    assert_ne!(unsupported, original);
    std::fs::write(&path, unsupported).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("non-CSV text terminal must reject")
        .to_string();
    assert!(error.contains("XML or CSV final target"), "{error}");
    assert!(!directory.0.join("not-created").exists());
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
fn original_host_can_feed_first_and_later_stage_from_one_vertex() {
    let directory = TempDir::new();
    let imported =
        mfd::import_pipeline(&make_first_and_late_named_source_chain(&directory.0)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages.len(), 2);
    for stage in &imported.pipeline.stages {
        assert!(stage.extra_sources.iter().any(|binding| {
            binding.name == "catalog"
                && matches!(&binding.from, PipelineInput::Host { name } if name == "catalog")
        }));
    }
    let original_outputs =
        engine::run_pipeline(&imported.pipeline, &late_named_inputs(&imported.pipeline)).unwrap();
    for stage in &imported.pipeline.stages {
        assert_eq!(
            original_outputs
                .stage(&stage.id)
                .unwrap()
                .primary
                .field("Lookup")
                .and_then(Instance::as_scalar),
            Some(&Value::String("host value".into()))
        );
    }

    let exported_path = directory.0.join("first-and-late-host.mfd");
    let report = mfd::preflight_pipeline_export(&imported.pipeline, &exported_path).unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let published = mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &exported_path,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    assert!(published.is_native_compatible(), "{published:?}");
    assert_catalog_item_fanout(&std::fs::read_to_string(&exported_path).unwrap(), 2);
    let reimported = mfd::import_pipeline(&exported_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
    let reimported_outputs = engine::run_pipeline(
        &reimported.pipeline,
        &late_named_inputs(&reimported.pipeline),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            original_outputs
                .stage(&imported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            reimported_outputs
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
        );
    }
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
fn four_stage_chain_reuses_original_host_across_later_stages() {
    for same_port in [false, true] {
        let directory = TempDir::new();
        let imported = mfd::import_pipeline(&make_four_stage_repeated_host_chain(
            &directory.0,
            same_port,
        ))
        .unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.pipeline.stages.len(), 4);
        for stage in [1, 3] {
            assert!(imported.pipeline.stages[stage]
                .extra_sources
                .iter()
                .any(|binding| {
                    binding.name == "catalog"
                        && matches!(&binding.from, PipelineInput::Host { name } if name == "catalog")
                }));
        }
        let original_outputs = engine::run_pipeline(
            &imported.pipeline,
            &repeated_host_inputs(&imported.pipeline),
        )
        .unwrap();
        assert_eq!(
            original_outputs
                .stage("mfd-stage-2")
                .unwrap()
                .primary
                .field("Lookup")
                .and_then(Instance::as_scalar),
            Some(&Value::String("host item".into()))
        );
        assert_eq!(
            original_outputs
                .stage("mfd-stage-4")
                .unwrap()
                .primary
                .field("Lookup")
                .and_then(Instance::as_scalar),
            Some(&Value::String(
                if same_port { "host item" } else { "host other" }.into()
            ))
        );

        let exported_path = directory.0.join("reused-host-export.mfd");
        let report = mfd::preflight_pipeline_export(&imported.pipeline, &exported_path).unwrap();
        assert!(report.is_native_compatible(), "{report:?}");
        let published = mfd::export_pipeline_with_profile(
            &imported.pipeline,
            &exported_path,
            mfd::ExportProfile::NativeMfd,
        )
        .unwrap();
        assert!(published.is_native_compatible(), "{published:?}");
        let exported = std::fs::read_to_string(&exported_path).unwrap();
        assert_catalog_item_fanout(&exported, if same_port { 2 } else { 1 });
        let reimported = mfd::import_pipeline(&exported_path).unwrap();
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        assert!(engine::validate_pipeline(&reimported.pipeline).is_empty());
        let reimported_outputs = engine::run_pipeline(
            &reimported.pipeline,
            &repeated_host_inputs(&reimported.pipeline),
        )
        .unwrap();
        for index in 0..4 {
            assert_eq!(
                original_outputs
                    .stage(&imported.pipeline.stages[index].id)
                    .unwrap()
                    .primary,
                reimported_outputs
                    .stage(&reimported.pipeline.stages[index].id)
                    .unwrap()
                    .primary,
                "same port: {same_port}, stage {}",
                index + 1,
            );
        }
    }
}

#[test]
fn four_stage_late_host_rejects_changed_boundary_or_unknown_host_without_publishing() {
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

    let mut unknown = imported.pipeline;
    unknown.stages[3].extra_sources[0].from = PipelineInput::Host {
        name: "introduced-late".into(),
    };
    let destination = directory.0.join("not-created/unknown.mfd");
    let error = mfd::export_pipeline(&unknown, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("introduces host input"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn late_named_xml_source_rejects_changed_boundary_or_name_without_publishing() {
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

    let mut renamed = imported.pipeline;
    renamed.stages[1].extra_sources[0].name = "other".into();
    let destination = directory.0.join("not-created/renamed.mfd");
    let error = mfd::export_pipeline(&renamed, &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no pipeline binding"), "{error}");
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
