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

fn make_json_final_chain(directory: &Path) -> PathBuf {
    let path = make_chain(directory);
    std::fs::write(
        directory.join("target.schema.json"),
        r#"{"title":"Target","type":"object","properties":{"Result":{"type":"string"}},"additionalProperties":false}"#,
    )
    .unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    let xml_target = r#"<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Result" inpkey="40"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>"#;
    let json_target = r#"<component name="target" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object"><entry name="Result" type="json-property"><entry name="string" inpkey="40"/></entry></entry></entry></entry></entry></root><json schema="target.schema.json" outputinstance="target.json"/></data></component>"#;
    let json_chain = original.replace(xml_target, json_target);
    assert_ne!(json_chain, original);
    std::fs::write(&path, json_chain).unwrap();
    path
}

fn make_xml_final_with_json_named_target(directory: &Path) -> PathBuf {
    let path = make_json_final_chain(directory);
    write_schema(&directory.join("secondary.xsd"), "Secondary", "Copy");
    let original = std::fs::read_to_string(&path).unwrap();
    let with_xml = original.replace(
        "</children><graph>",
        r#"<component name="secondary" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Secondary"><entry name="Copy" inpkey="50"/></entry></root><document schema="secondary.xsd" outputinstance="secondary.xml" instanceroot="{}Secondary"/></data></component></children><graph>"#,
    );
    let with_fanout = with_xml.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
    );
    let named = with_fanout.replace(
        "<component name=\"target\" library=\"json\" kind=\"31\"><properties XSLTDefaultOutput=\"1\"/>",
        "<component name=\"target\" library=\"json\" kind=\"31\">",
    );
    assert_ne!(named, original);
    std::fs::write(&path, named).unwrap();
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

fn make_xml_final_with_csv_named_target(directory: &Path) -> PathBuf {
    let path = make_csv_final_with_xml_named_target(directory);
    let original = std::fs::read_to_string(&path).unwrap();
    let changed = original
        .replace(
            "<component name=\"target\" library=\"text\" kind=\"16\"><properties XSLTDefaultOutput=\"1\"/>",
            "<component name=\"target\" library=\"text\" kind=\"16\">",
        )
        .replace(
            "<component name=\"secondary\" library=\"xml\" kind=\"14\"><data>",
            "<component name=\"secondary\" library=\"xml\" kind=\"14\"><properties XSLTDefaultOutput=\"1\"/><data>",
        );
    assert_ne!(changed, original);
    std::fs::write(&path, changed).unwrap();
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
fn local_chains_keep_only_declared_intermediate_instance_paths_when_available() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    for (relative, output_instance, input_instance) in [
        ("ChainedPersonList.mfd", None, None),
        (
            "Tutorial/ChainedReports.mfd",
            Some("ReportB.xml"),
            Some("ReportB.xml"),
        ),
        (
            "Tutorial/Tut-ExpReport-chain.mfd",
            None,
            Some("mf-ExpReport-co.xml"),
        ),
        (
            "Tutorial/BasicTutorials/Tut3-ChainedMapping.mfd",
            Some("MergedLibrary.xml"),
            None,
        ),
    ] {
        let sample = samples.join(relative);
        if !sample.is_file() {
            continue;
        }
        let imported =
            mfd::import_pipeline(&sample).unwrap_or_else(|error| panic!("{relative}: {error}"));
        assert!(
            imported.warnings.is_empty(),
            "{relative}: {:?}",
            imported.warnings
        );
        assert_eq!(imported.pipeline.stages.len(), 2, "{relative}");
        assert_eq!(
            imported.pipeline.stages[0].project.target_path.as_deref(),
            output_instance,
            "{relative}: intermediate output path"
        );
        assert_eq!(
            imported.pipeline.stages[1].project.source_path.as_deref(),
            input_instance,
            "{relative}: intermediate input preview"
        );

        let is_tut3 = relative.ends_with("Tut3-ChainedMapping.mfd");
        let before = if is_tut3 {
            Some(execute_local_tut3(&imported.pipeline, &sample))
        } else {
            None
        };
        let directory = TempDir::new();
        let destination = directory.0.join("exported-chain.mfd");
        let report = mfd::preflight_pipeline_export(&imported.pipeline, &destination)
            .unwrap_or_else(|error| panic!("{relative}: {error}"));
        assert!(report.is_native_compatible(), "{relative}: {report:?}");
        mfd::export_pipeline_with_profile(
            &imported.pipeline,
            &destination,
            mfd::ExportProfile::NativeMfd,
        )
        .unwrap_or_else(|error| panic!("{relative}: {error}"));
        let exported = std::fs::read_to_string(&destination).unwrap();
        let document = roxmltree::Document::parse(&exported).unwrap();
        if is_tut3 {
            assert!(!document.descendants().any(|node| {
                node.has_tag_name("component") && node.attribute("library") == Some("ferrule")
            }));
            let typed_targets = document
                .descendants()
                .filter(|node| {
                    node.has_tag_name("component")
                        && node.attribute("library") == Some("xml")
                        && node.descendants().any(|entry| {
                            entry.has_tag_name("entry")
                                && entry.attribute("name") == Some("last_updated")
                                && entry.attribute("inpkey").is_some()
                        })
                })
                .collect::<Vec<_>>();
            assert_eq!(typed_targets.len(), 2, "{relative}: target count");
            for component in typed_targets {
                let target_document = component
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("entry") && node.attribute("name") == Some("document")
                    })
                    .unwrap();
                assert_eq!(
                    target_document.attribute("casttotargettypemode"),
                    Some("cast-in-subtree")
                );
                let schema_file = component
                    .descendants()
                    .find(|node| node.has_tag_name("document"))
                    .and_then(|node| node.attribute("schema"))
                    .unwrap();
                let schema_xml = std::fs::read_to_string(directory.0.join(schema_file)).unwrap();
                let schema = roxmltree::Document::parse(&schema_xml).unwrap();
                assert_eq!(
                    schema
                        .descendants()
                        .find(|node| {
                            node.has_tag_name("element")
                                && node.attribute("name") == Some("last_updated")
                        })
                        .and_then(|node| node.attribute("type")),
                    Some("xs:dateTime")
                );
                assert_eq!(
                    schema
                        .descendants()
                        .find(|node| {
                            node.has_tag_name("element")
                                && node.attribute("name") == Some("publish_year")
                        })
                        .and_then(|node| node.attribute("type")),
                    Some("xs:integer")
                );
            }
        }
        let intermediate = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component")
                    && node.children().any(|child| {
                        child.has_tag_name("properties")
                            && child.attribute("PassThrough") == Some("1")
                    })
            })
            .unwrap_or_else(|| panic!("{relative}: missing pass-through component"));
        let document = intermediate
            .descendants()
            .find(|node| node.has_tag_name("document"))
            .unwrap();
        assert_eq!(
            document.attribute("outputinstance"),
            output_instance,
            "{relative}: exported output instance"
        );
        assert_eq!(
            document.attribute("inputinstance"),
            input_instance,
            "{relative}: exported input preview"
        );
        assert!(
            !intermediate.descendants().any(|node| {
                node.has_tag_name("file") && node.attribute("role") == Some("outputinstance")
            }),
            "{relative}: unexpected nested output instance"
        );

        let reimported = mfd::import_pipeline(&destination)
            .unwrap_or_else(|error| panic!("{relative}: {error}"));
        assert!(
            reimported.warnings.is_empty(),
            "{relative}: {:?}",
            reimported.warnings
        );
        assert_eq!(
            reimported.pipeline.stages[0].project.target_path.as_deref(),
            output_instance,
            "{relative}: reimported output path"
        );
        assert_eq!(
            reimported.pipeline.stages[1].project.source_path.as_deref(),
            input_instance,
            "{relative}: reimported input preview"
        );
        if let Some(before) = before {
            let after = execute_local_tut3(&reimported.pipeline, &sample);
            for index in 0..2 {
                assert_eq!(
                    before
                        .stage(&imported.pipeline.stages[index].id)
                        .unwrap()
                        .primary,
                    after
                        .stage(&reimported.pipeline.stages[index].id)
                        .unwrap()
                        .primary,
                    "{relative}: stage {} changed execution",
                    index + 1
                );
            }
        }
    }
}

fn execute_local_tut3(pipeline: &mapping::Pipeline, sample: &Path) -> engine::PipelineOutputs {
    let first = &pipeline.stages[0];
    let PipelineInput::Host { name } = &first.source else {
        panic!("local chain needs a host primary source");
    };
    let directory = sample.parent().unwrap();
    let input_path = |name: &str| {
        ["Library.xml", "Books.xml"]
            .into_iter()
            .find(|file| file.trim_end_matches(".xml").eq_ignore_ascii_case(name))
            .map(|file| directory.join(file))
            .unwrap_or_else(|| panic!("unknown local source {name}"))
    };
    let mut hosts = BTreeMap::from([(
        name.clone(),
        format_xml::read(&input_path(name), &first.project.source).unwrap(),
    )]);
    for binding in &first.extra_sources {
        let PipelineInput::Host { name } = &binding.from else {
            panic!("local chain needs host named sources");
        };
        let source = first
            .project
            .extra_sources
            .iter()
            .find(|source| source.name == binding.name)
            .unwrap();
        hosts.insert(
            name.clone(),
            format_xml::read(&input_path(name), &source.schema).unwrap(),
        );
    }
    let execution =
        engine::ExecutionContext::new(sample).with_current_datetime("2026-09-29T12:00:00-07:00");
    engine::run_pipeline_with_context(pipeline, &hosts, &execution).unwrap()
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
fn xml_primary_may_keep_one_connected_csv_named_target_with_exact_output() {
    let (pipeline, hosts) = local_xml_with_named_csv_pipeline();
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_stage = &pipeline.stages[1];
    let final_before = before.stage(&final_stage.id).unwrap();
    assert_eq!(final_before.primary, hosts["input"]);
    let [csv] = final_stage.project.extra_targets.as_slice() else {
        panic!("final stage must keep one named CSV target");
    };
    let [named_before] = final_before.extras.as_slice() else {
        panic!("final stage must produce one named CSV output");
    };
    assert_eq!(named_before.name, csv.name);
    let xml_before =
        format_xml::to_string(&final_stage.project.target, &final_before.primary).unwrap();
    let directory = TempDir::new();
    let csv_before_path = directory.0.join("before.csv");
    format_csv::write_with_dialect(
        &csv_before_path,
        &csv.schema,
        named_before.instance.as_repeated().unwrap(),
        csv.options.delimiter,
        csv.options.csv_quote,
        csv.options.csv_quote_disabled,
        csv.options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    let csv_before = std::fs::read(&csv_before_path).unwrap();
    assert_eq!(csv_before, b"Alice Carter;34\nBo Diaz;41\n");

    let exported = directory.0.join("xml-with-named-csv.mfd");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let design = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(design.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(design.matches("library=\"text\"").count(), 1);
    assert_eq!(design.matches("XSLTDefaultOutput=\"1\"").count(), 1);
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_after = after.stage(&reimported.pipeline.stages[1].id).unwrap();
    assert_eq!(final_before.extras, final_after.extras);
    let xml_after = format_xml::to_string(
        &reimported.pipeline.stages[1].project.target,
        &final_after.primary,
    )
    .unwrap();
    assert_eq!(xml_after.as_bytes(), xml_before.as_bytes());
    let csv = &reimported.pipeline.stages[1].project.extra_targets[0];
    let csv_after_path = directory.0.join("after.csv");
    format_csv::write_with_dialect(
        &csv_after_path,
        &csv.schema,
        final_after.extras[0].instance.as_repeated().unwrap(),
        csv.options.delimiter,
        csv.options.csv_quote,
        csv.options.csv_quote_disabled,
        csv.options.has_header_row.unwrap_or(true),
    )
    .unwrap();
    assert_eq!(std::fs::read(csv_after_path).unwrap(), csv_before);
}

#[test]
fn xml_primary_named_csv_export_rejects_nonfinal_and_multiple_targets_without_artifacts() {
    let (pipeline, _) = local_xml_with_named_csv_pipeline();
    let directory = TempDir::new();

    let mut nonfinal = pipeline.clone();
    let named = nonfinal.stages[1].project.extra_targets.remove(0);
    nonfinal.stages[0].project.graph = nonfinal.stages[1].project.graph.clone();
    nonfinal.stages[0].project.extra_targets.push(named);
    assert!(engine::validate_pipeline(&nonfinal).is_empty());
    let path = directory.0.join("not-created/nonfinal.mfd");
    let error = mfd::export_pipeline_with_profile(&nonfinal, &path, mfd::ExportProfile::NativeMfd)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("independent targets before the final stage"),
        "{error}"
    );
    assert!(!path.parent().unwrap().exists());

    let mut multiple = pipeline;
    let mut other = multiple.stages[1].project.extra_targets[0].clone();
    other.name = "other rows".into();
    other.path = Some("other.csv".into());
    multiple.stages[1].project.extra_targets.push(other);
    assert!(engine::validate_pipeline(&multiple).is_empty());
    let path = directory.0.join("not-created/multiple.mfd");
    let error = mfd::export_pipeline_with_profile(&multiple, &path, mfd::ExportProfile::NativeMfd)
        .unwrap_err()
        .to_string();
    assert!(error.contains("more than one named CSV target"), "{error}");
    assert!(!path.parent().unwrap().exists());
}

#[test]
fn xml_primary_named_csv_import_rejects_ambiguous_malformed_and_bypassed_targets() {
    let directory = TempDir::new();
    let path = make_xml_final_with_csv_named_target(&directory.0);
    let original = std::fs::read_to_string(&path).unwrap();
    let imported = mfd::import_pipeline(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages[1].project.extra_targets.len(), 1);

    let ambiguous = original.replace(
        "<component name=\"target\" library=\"text\" kind=\"16\"><data>",
        "<component name=\"target\" library=\"text\" kind=\"16\"><properties XSLTDefaultOutput=\"1\"/><data>",
    );
    assert_ne!(ambiguous, original);
    std::fs::write(&path, ambiguous).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("ambiguous final target must reject")
        .to_string();
    assert!(error.contains("one connected XML"), "{error}");

    let malformed = original.replace(
        "<component name=\"target\" library=\"text\" kind=\"16\">",
        "<component name=\"target\" library=\"text\" kind=\"15\">",
    );
    assert_ne!(malformed, original);
    std::fs::write(&path, malformed).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("malformed named CSV target must reject")
        .to_string();
    assert!(
        error.contains("components only as the final primary target"),
        "{error}"
    );

    let bypassed = original
        .replace(
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/><edge vertexkey=\"40\"/></edges></vertex>",
        )
        .replace(
            "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"21\"/></edges></vertex>",
            "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"21\"/><edge vertexkey=\"41\"/></edges></vertex>",
        )
        .replace(
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"50\"/></edges></vertex>",
        )
        .replace(
            "<vertex vertexkey=\"31\"><edges><edge vertexkey=\"41\"/></edges></vertex>",
            "<vertex vertexkey=\"31\"><edges/></vertex>",
        );
    assert_ne!(bypassed, original);
    std::fs::write(&path, bypassed).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("bypassed named CSV target must reject")
        .to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn xml_primary_may_keep_one_connected_json_named_target_with_exact_output() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    if !samples.join("Altova_Hierarchical_JSON.mfd").is_file()
        || !samples.join("Altova_Hierarchical.xml").is_file()
    {
        return;
    }
    let (pipeline, hosts) = local_xml_with_named_json_pipeline();
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_stage = &pipeline.stages[1];
    let final_before = before.stage(&final_stage.id).unwrap();
    assert_eq!(final_before.primary, hosts["input"]);
    let [json] = final_stage.project.extra_targets.as_slice() else {
        panic!("final stage must keep one named JSON target");
    };
    let [named_before] = final_before.extras.as_slice() else {
        panic!("final stage must produce one named JSON output");
    };
    assert_eq!(named_before.name, json.name);
    let xml_before =
        format_xml::to_string(&final_stage.project.target, &final_before.primary).unwrap();
    let json_before = format_json::to_string(&json.schema, &named_before.instance).unwrap();
    assert!(json_before.contains("\"Office\""));

    let directory = TempDir::new();
    let exported = directory.0.join("xml-with-named-json.mfd");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let design = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(design.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(design.matches("library=\"json\"").count(), 1);
    assert_eq!(design.matches("XSLTDefaultOutput=\"1\"").count(), 1);
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_after = after.stage(&reimported.pipeline.stages[1].id).unwrap();
    assert_eq!(final_before.extras, final_after.extras);
    let xml_after = format_xml::to_string(
        &reimported.pipeline.stages[1].project.target,
        &final_after.primary,
    )
    .unwrap();
    assert_eq!(xml_after.as_bytes(), xml_before.as_bytes());
    let json_after = format_json::to_string(
        &reimported.pipeline.stages[1].project.extra_targets[0].schema,
        &final_after.extras[0].instance,
    )
    .unwrap();
    assert_eq!(json_after.as_bytes(), json_before.as_bytes());
}

#[test]
fn xml_primary_named_json_export_rejects_nonfinal_multiple_and_json_lines_without_artifacts() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    if !samples.join("Altova_Hierarchical_JSON.mfd").is_file()
        || !samples.join("Altova_Hierarchical.xml").is_file()
    {
        return;
    }
    let (pipeline, _) = local_xml_with_named_json_pipeline();
    let directory = TempDir::new();

    let mut nonfinal = pipeline.clone();
    let named = nonfinal.stages[1].project.extra_targets.remove(0);
    nonfinal.stages[0].project.graph = nonfinal.stages[1].project.graph.clone();
    nonfinal.stages[0].project.extra_targets.push(named);
    assert!(engine::validate_pipeline(&nonfinal).is_empty());
    let path = directory.0.join("not-created/nonfinal.mfd");
    let error = mfd::export_pipeline_with_profile(&nonfinal, &path, mfd::ExportProfile::NativeMfd)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("independent targets before the final stage"),
        "{error}"
    );
    assert!(!path.parent().unwrap().exists());

    let mut multiple = pipeline.clone();
    let mut other = multiple.stages[1].project.extra_targets[0].clone();
    other.name = "other report".into();
    other.path = Some("other.json".into());
    multiple.stages[1].project.extra_targets.push(other);
    assert!(engine::validate_pipeline(&multiple).is_empty());
    let path = directory.0.join("not-created/multiple.mfd");
    let error = mfd::export_pipeline_with_profile(&multiple, &path, mfd::ExportProfile::NativeMfd)
        .unwrap_err()
        .to_string();
    assert!(error.contains("more than one named JSON target"), "{error}");
    assert!(!path.parent().unwrap().exists());

    let mut json_lines = pipeline;
    json_lines.stages[1].project.extra_targets[0].path = Some("report.jsonl".into());
    let path = directory.0.join("not-created/json-lines.mfd");
    let error =
        mfd::export_pipeline_with_profile(&json_lines, &path, mfd::ExportProfile::NativeMfd)
            .unwrap_err()
            .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!path.parent().unwrap().exists());
}

#[test]
fn xml_primary_named_json_import_rejects_ambiguous_malformed_bypass_and_unrepresented_pins() {
    let directory = TempDir::new();
    let path = make_xml_final_with_json_named_target(&directory.0);
    let original = std::fs::read_to_string(&path).unwrap();
    let imported = mfd::import_pipeline(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pipeline.stages[1].project.extra_targets.len(), 1);

    let ambiguous = original.replace(
        "<component name=\"target\" library=\"json\" kind=\"31\"><data>",
        "<component name=\"target\" library=\"json\" kind=\"31\"><properties XSLTDefaultOutput=\"1\"/><data>",
    );
    assert_ne!(ambiguous, original);
    std::fs::write(&path, ambiguous).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("ambiguous final target must reject")
        .to_string();
    assert!(error.contains("one connected XML"), "{error}");

    let malformed = original.replace(
        "<component name=\"target\" library=\"json\" kind=\"31\">",
        "<component name=\"target\" library=\"json\" kind=\"30\">",
    );
    assert_ne!(malformed, original);
    std::fs::write(&path, malformed).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("malformed named JSON target must reject")
        .to_string();
    assert!(
        error.contains("JSON components only as the final primary target"),
        "{error}"
    );

    let json_lines = original.replace(
        "<json schema=\"target.schema.json\" outputinstance=\"target.json\"/>",
        "<json schema=\"target.schema.json\" outputinstance=\"target.json\" jsonlines=\"1\"/>",
    );
    assert_ne!(json_lines, original);
    std::fs::write(&path, json_lines).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("named JSON Lines target must reject")
        .to_string();
    assert!(
        error.contains("JSON components only as the final primary target"),
        "{error}"
    );

    let disconnected = original.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"50\"/></edges></vertex>",
    );
    assert_ne!(disconnected, original);
    std::fs::write(&path, &disconnected).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("disconnected named JSON target must reject")
        .to_string();
    assert!(
        error.contains("JSON components only as the final primary target"),
        "{error}"
    );

    let bypassed = disconnected.replace(
        "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
        "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/><edge vertexkey=\"40\"/></edges></vertex>",
    );
    assert_ne!(bypassed, disconnected);
    std::fs::write(&path, bypassed).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("bypassed named JSON target must reject")
        .to_string();
    assert!(
        error.contains("without branches, cycles, or bypasses"),
        "{error}"
    );

    let extra_input = original
        .replace(
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
            "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/><edge vertexkey=\"4294967289\"/></edges></vertex>",
        )
        .replace(
            "<root><entry name=\"FileInstance\">",
            "<root><entry name=\"bogus\" inpkey=\"4294967289\"/><entry name=\"FileInstance\">",
        );
    assert_ne!(extra_input, original);
    std::fs::write(&path, extra_input).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("unrepresented connected named JSON pin must reject")
        .to_string();
    assert!(
        error.contains("unrepresented connected input port"),
        "{error}"
    );
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn json_final_chain_imports_exports_and_preserves_exact_serialization() {
    let directory = TempDir::new();
    let imported = mfd::import_pipeline(&make_json_final_chain(&directory.0)).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported.pipeline.stages[1]
            .project
            .target_options
            .json_document
    );
    let before = execute(&imported.pipeline);
    let final_before = &before.stage("mfd-stage-2").unwrap().primary;
    let json_before =
        format_json::to_string(&imported.pipeline.stages[1].project.target, final_before).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json_before).unwrap();
    assert_eq!(value["Result"], "serial value");

    let exported = directory.0.join("exported-json-chain.mfd");
    let preflight = mfd::preflight_pipeline_export(&imported.pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report = mfd::export_pipeline_with_profile(
        &imported.pipeline,
        &exported,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let xml = std::fs::read_to_string(&exported).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .filter(|node| {
                node.has_tag_name("component") && node.attribute("library") == Some("json")
            })
            .count(),
        1
    );
    assert_eq!(xml.matches("PassThrough=\"1\"").count(), 1);
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
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
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_after = &after.stage("mfd-stage-2").unwrap().primary;
    let json_after =
        format_json::to_string(&reimported.pipeline.stages[1].project.target, final_after).unwrap();
    assert_eq!(json_after, json_before);
}

fn identity_xml_to_xlsx_pipeline(
    design: &Path,
    input: &Path,
) -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    let imported = mfd::import(design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut final_project = imported.project;
    assert!(final_project.target_options.xlsx_hierarchical.is_some());
    let source = format_xml::read(input, &final_project.source).unwrap();
    let source_schema = final_project.source.clone();
    let source_options = final_project.source_options.clone();
    final_project.source_path = None;
    final_project.target_path = Some("converted.xlsx".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some(input.file_name().unwrap().to_str().unwrap().into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: mapping::Graph::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..mapping::Scope::default()
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
                project: final_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    (pipeline, BTreeMap::from([("input".into(), source)]))
}

fn xlsx_instance_by_named_fields(instance: &Instance) -> Instance {
    match instance {
        Instance::Group(fields) => {
            let mut fields = fields
                .iter()
                .map(|(name, value)| (name.clone(), xlsx_instance_by_named_fields(value)))
                .collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            Instance::Group(fields)
        }
        Instance::Repeated(items) => {
            Instance::Repeated(items.iter().map(xlsx_instance_by_named_fields).collect())
        }
        _ => instance.clone(),
    }
}

fn assert_identity_xml_to_xlsx_roundtrip(design: &Path, input: &Path) {
    let (pipeline, hosts) = identity_xml_to_xlsx_pipeline(design, input);
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_project = &pipeline.stages[1].project;
    let layout = final_project
        .target_options
        .xlsx_hierarchical
        .as_ref()
        .unwrap();
    let (workbook_before, worksheet_count) = format_xlsx::to_bytes_hierarchical(
        &final_project.target,
        &before.stage("workbook").unwrap().primary,
        layout,
    )
    .unwrap();
    assert!(worksheet_count > 0);
    let cells_before =
        format_xlsx::from_bytes_hierarchical(&workbook_before, &final_project.target, layout)
            .unwrap();

    let directory = TempDir::new();
    std::fs::copy(input, directory.0.join(input.file_name().unwrap())).unwrap();
    let exported = directory.0.join("xlsx-chain.mfd");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(xml.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(xml.matches("library=\"xlsx\"").count(), 1);
    assert!(xml.contains("<excel outputinstance=\"converted.xlsx\"/>"));
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        let original = &before.stage(&pipeline.stages[index].id).unwrap().primary;
        let roundtripped = &after
            .stage(&reimported.pipeline.stages[index].id)
            .unwrap()
            .primary;
        if index == 0 {
            assert_eq!(original, roundtripped, "stage {}", index + 1);
        } else {
            // Workbook coordinates come from the retained layout; group field
            // insertion order can differ while worksheet and row order must not.
            assert_eq!(
                xlsx_instance_by_named_fields(original),
                xlsx_instance_by_named_fields(roundtripped),
                "stage {}",
                index + 1
            );
        }
    }
    let final_project = &reimported.pipeline.stages[1].project;
    let layout = final_project
        .target_options
        .xlsx_hierarchical
        .as_ref()
        .unwrap();
    let (workbook_after, worksheet_count_after) = format_xlsx::to_bytes_hierarchical(
        &final_project.target,
        &after
            .stage(&reimported.pipeline.stages[1].id)
            .unwrap()
            .primary,
        layout,
    )
    .unwrap();
    assert_eq!(worksheet_count_after, worksheet_count);
    let cells_after =
        format_xlsx::from_bytes_hierarchical(&workbook_after, &final_project.target, layout)
            .unwrap();
    assert_eq!(cells_after, cells_before);
}

#[test]
fn synthetic_hierarchical_xlsx_final_chain_roundtrips_workbook_cells() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    assert_identity_xml_to_xlsx_roundtrip(
        &fixtures.join("xlsx-hierarchical.mfd"),
        &fixtures.join("xlsx-hierarchical-source.xml"),
    );
}

#[test]
fn local_xml_to_xlsx_mapping_runs_after_an_identity_xml_stage() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("Altova_Hierarchical_Excel.mfd");
    let input = samples.join("Altova_Hierarchical.xml");
    if !design.is_file() || !input.is_file() {
        return;
    }
    assert_identity_xml_to_xlsx_roundtrip(&design, &input);
}

fn identity_xml_to_fixed_width_pipeline(
    mut final_project: mapping::Project,
    input: &Path,
) -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    assert!(final_project.target_options.fixed_width.is_some());
    let source = format_xml::read(input, &final_project.source).unwrap();
    let source_schema = final_project.source.clone();
    let source_options = final_project.source_options.clone();
    final_project.source_path = None;
    final_project.target_path = Some("converted.dat".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some(input.file_name().unwrap().to_str().unwrap().into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: mapping::Graph::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..mapping::Scope::default()
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
                id: "fixed".into(),
                mapping_path: None,
                project: final_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    (pipeline, BTreeMap::from([("input".into(), source)]))
}

fn assert_identity_xml_to_fixed_width_roundtrip(
    pipeline: &mapping::Pipeline,
    hosts: &BTreeMap<String, Instance>,
    input: &Path,
) {
    let before = engine::run_pipeline(pipeline, hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_project = &pipeline.stages[1].project;
    let layout = final_project.target_options.fixed_width.as_ref().unwrap();
    let rows_before = before
        .stage("fixed")
        .unwrap()
        .primary
        .as_repeated()
        .unwrap();
    let text_before =
        format_csv::to_string_fixed_width(&final_project.target, rows_before, layout).unwrap();
    assert!(!text_before.is_empty());
    let parsed_before =
        format_csv::from_str_fixed_width(&text_before, &final_project.target, layout).unwrap();

    let directory = TempDir::new();
    std::fs::copy(input, directory.0.join(input.file_name().unwrap())).unwrap();
    let exported = directory.0.join("fixed-width-chain.mfd");
    let preflight = mfd::preflight_pipeline_export(pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report =
        mfd::export_pipeline_with_profile(pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(xml.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(xml.matches("<text type=\"flf\"").count(), 1);
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_project = &reimported.pipeline.stages[1].project;
    let layout = final_project.target_options.fixed_width.as_ref().unwrap();
    let rows_after = after
        .stage(&reimported.pipeline.stages[1].id)
        .unwrap()
        .primary
        .as_repeated()
        .unwrap();
    let text_after =
        format_csv::to_string_fixed_width(&final_project.target, rows_after, layout).unwrap();
    assert_eq!(text_after, text_before);
    let parsed_after =
        format_csv::from_str_fixed_width(&text_after, &final_project.target, layout).unwrap();
    assert_eq!(parsed_after, parsed_before);
}

fn synthetic_fixed_width_pipeline() -> (mapping::Pipeline, BTreeMap<String, Instance>, PathBuf) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("people-to-csv.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut final_project = imported.project;
    final_project.target_options = mapping::FormatOptions {
        fixed_width: Some(
            mapping::FixedWidthLayout::new(
                vec![
                    mapping::FixedFieldWidth::new(30).unwrap(),
                    mapping::FixedFieldWidth::new(4).unwrap(),
                ],
                ' ',
                true,
                true,
            )
            .unwrap(),
        ),
        ..mapping::FormatOptions::default()
    };
    let input = fixtures.join("people.xml");
    let (pipeline, hosts) = identity_xml_to_fixed_width_pipeline(final_project, &input);
    (pipeline, hosts, input)
}

#[test]
fn synthetic_fixed_width_final_chain_roundtrips_exact_text() {
    let (pipeline, hosts, input) = synthetic_fixed_width_pipeline();
    assert_identity_xml_to_fixed_width_roundtrip(&pipeline, &hosts, &input);
}

#[test]
fn local_xml_to_fixed_width_mapping_runs_after_an_identity_xml_stage() {
    let samples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples/Tutorial");
    let design = samples.join("MissingFields.mfd");
    let input = samples.join("MissingFields.xml");
    if !design.is_file() || !input.is_file() {
        return;
    }
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let (pipeline, hosts) = identity_xml_to_fixed_width_pipeline(imported.project, &input);
    assert_identity_xml_to_fixed_width_roundtrip(&pipeline, &hosts, &input);
}

#[test]
fn fixed_width_final_chain_rejects_unsupported_boundaries_without_artifacts() {
    let (pipeline, _, _) = synthetic_fixed_width_pipeline();
    let directory = TempDir::new();

    let mut intermediate_fixed = mfd::import_pipeline(&make_chain(&directory.0))
        .unwrap()
        .pipeline;
    intermediate_fixed.stages[0].project.target_path = Some("buffer.dat".into());
    intermediate_fixed.stages[0]
        .project
        .target_options
        .fixed_width = Some(
        mapping::FixedWidthLayout::new(
            vec![mapping::FixedFieldWidth::new(30).unwrap()],
            ' ',
            true,
            true,
        )
        .unwrap(),
    );
    let destination = directory.0.join("not-created/intermediate.mfd");
    let error = mfd::export_pipeline_with_profile(
        &intermediate_fixed,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut named_fixed = pipeline.clone();
    let final_project = &mut named_fixed.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.dat".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: final_project.root.clone(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    let error = mfd::export_pipeline_with_profile(
        &named_fixed,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut width_mismatch = pipeline.clone();
    width_mismatch.stages[1].project.target_options.fixed_width = Some(
        mapping::FixedWidthLayout::new(
            vec![mapping::FixedFieldWidth::new(30).unwrap()],
            ' ',
            true,
            true,
        )
        .unwrap(),
    );
    let destination = directory.0.join("not-created/width-mismatch.mfd");
    let error = mfd::export_pipeline_with_profile(
        &width_mismatch,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("fixed-width"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut disconnected = pipeline;
    disconnected.stages[1].project.root = mapping::Scope::default();
    disconnected.stages[1].project.prune_unreachable_nodes();
    let destination = directory.0.join("not-created/disconnected.mfd");
    let error = mfd::export_pipeline_with_profile(
        &disconnected,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("pipeline export graph"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

fn add_connected_fixed_width_target(xml: &str, default_output: bool) -> String {
    const EXTRA_KEY: &str = "4294967289";
    let document = roxmltree::Document::parse(xml).unwrap();
    let pass_through = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .unwrap();
    let vertex = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .find(|node| {
            pass_through.descendants().any(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("outkey") == node.attribute("vertexkey")
            })
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let mut with_edge = xml.to_owned();
    with_edge.insert_str(
        edges.range().end - "</edges>".len(),
        &format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
    );
    let output_property = if default_output {
        " XSLTDefaultOutput=\"1\""
    } else {
        ""
    };
    let component = format!(
        "<component name=\"other\" library=\"text\" kind=\"16\"><properties{output_property}/><data><root><entry name=\"FileInstance\"><entry name=\"document\"><entry name=\"Rows\" inpkey=\"{EXTRA_KEY}\"/></entry></entry></root><text type=\"flf\" outputinstance=\"other.dat\"/></data></component>"
    );
    let children_end = with_edge.rfind("</children>").unwrap();
    with_edge.insert_str(children_end, &component);
    with_edge
}

#[test]
fn fixed_width_final_chain_import_rejects_malformed_ambiguous_and_disconnected() {
    let (pipeline, _, _) = synthetic_fixed_width_pipeline();
    let directory = TempDir::new();
    let design = directory.0.join("source-fixed-chain.mfd");
    mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd).unwrap();
    let original = std::fs::read_to_string(&design).unwrap();
    assert!(mfd::import_pipeline(&design).is_ok());

    let ambiguous = add_connected_fixed_width_target(&original, true);
    std::fs::write(&design, ambiguous).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("ambiguous fixed-width final must reject")
        .to_string();
    assert!(
        error.contains("one connected XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, XBRL, or XLSX final target"),
        "{error}"
    );

    let named = add_connected_fixed_width_target(&original, false);
    std::fs::write(&design, named).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("named fixed-width terminal must reject")
        .to_string();
    assert!(
        error.contains(
            "CSV, fixed-width text, and FlexText components only as the final primary target"
        ),
        "{error}"
    );

    let document = roxmltree::Document::parse(&original).unwrap();
    let fixed_width = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("text")
                && node.descendants().any(|entry| {
                    entry.has_tag_name("text") && entry.attribute("type") == Some("flf")
                })
        })
        .unwrap();
    let mut disconnected = original.clone();
    for key in fixed_width
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| entry.attribute("inpkey"))
    {
        disconnected = disconnected.replace(&format!("<edge vertexkey=\"{key}\"/>"), "");
    }
    assert_ne!(disconnected, original);
    std::fs::write(&design, disconnected).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("disconnected fixed-width final must reject")
        .to_string();
    assert!(error.contains("connected"), "{error}");

    let malformed = original.replacen("length=\"30\"", "length=\"0\"", 1);
    assert_ne!(malformed, original);
    std::fs::write(&design, malformed).unwrap();
    assert!(
        mfd::import_pipeline(&design).is_err(),
        "zero-width final layout must reject"
    );
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn xlsx_final_chain_rejects_unsupported_boundaries_without_artifacts() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let (pipeline, _) = identity_xml_to_xlsx_pipeline(
        &fixtures.join("xlsx-hierarchical.mfd"),
        &fixtures.join("xlsx-hierarchical-source.xml"),
    );
    let directory = TempDir::new();

    let mut intermediate_xlsx = pipeline.clone();
    intermediate_xlsx.stages[0].project.target_path = Some("buffer.xlsx".into());
    let destination = directory.0.join("not-created/intermediate.mfd");
    let error = mfd::export_pipeline_with_profile(
        &intermediate_xlsx,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut named_xlsx = pipeline.clone();
    let final_project = &mut named_xlsx.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.xlsx".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: final_project.root.clone(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    let error =
        mfd::export_pipeline_with_profile(&named_xlsx, &destination, mfd::ExportProfile::NativeMfd)
            .unwrap_err()
            .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut update_existing = pipeline.clone();
    update_existing.stages[1]
        .project
        .target_options
        .xlsx_update_existing = true;
    let destination = directory.0.join("not-created/update-existing.mfd");
    let error = mfd::export_pipeline_with_profile(
        &update_existing,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("xlsx_update_existing"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut flat_update_existing = mfd::import_pipeline(&make_csv_final_chain(&directory.0))
        .unwrap()
        .pipeline;
    flat_update_existing.stages[1].project.target_path = Some("converted.xlsx".into());
    flat_update_existing.stages[1].project.target_options = mapping::FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
        xlsx_update_existing: true,
        ..mapping::FormatOptions::default()
    };
    let destination = directory.0.join("not-created/flat-update-existing.mfd");
    let error = mfd::export_pipeline_with_profile(
        &flat_update_existing,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("new-workbook XLSX"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut disconnected = pipeline;
    disconnected.stages[1].project.root = mapping::Scope::default();
    disconnected.stages[1].project.prune_unreachable_nodes();
    let destination = directory.0.join("not-created/disconnected.mfd");
    let error = mfd::export_pipeline_with_profile(
        &disconnected,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("pipeline export graph"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

fn add_connected_xlsx_target(xml: &str, default_output: bool) -> String {
    const EXTRA_KEY: &str = "4294967290";
    let document = roxmltree::Document::parse(xml).unwrap();
    let pass_through = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .unwrap();
    let vertex = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .find(|node| {
            pass_through.descendants().any(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("outkey") == node.attribute("vertexkey")
            })
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let mut with_edge = xml.to_owned();
    with_edge.insert_str(
        edges.range().end - "</edges>".len(),
        &format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
    );
    let output_property = if default_output {
        " XSLTDefaultOutput=\"1\""
    } else {
        ""
    };
    let component = format!(
        "<component name=\"other\" library=\"xlsx\" kind=\"26\"><properties{output_property}/><data><root><entry name=\"FileInstance\"><entry name=\"document\"><entry name=\"Workbook\"><entry name=\"Worksheet\" inpkey=\"{EXTRA_KEY}\"/></entry></entry></entry></root><excel outputinstance=\"other.xlsx\"/></data></component>"
    );
    let children_end = with_edge.rfind("</children>").unwrap();
    with_edge.insert_str(children_end, &component);
    with_edge
}

#[test]
fn xlsx_final_chain_import_rejects_ambiguous_disconnected_and_update_existing() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let (pipeline, _) = identity_xml_to_xlsx_pipeline(
        &fixtures.join("xlsx-hierarchical.mfd"),
        &fixtures.join("xlsx-hierarchical-source.xml"),
    );
    let directory = TempDir::new();
    let design = directory.0.join("source-xlsx-chain.mfd");
    mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd).unwrap();
    let original = std::fs::read_to_string(&design).unwrap();
    assert!(mfd::import_pipeline(&design).is_ok());

    let ambiguous = add_connected_xlsx_target(&original, true);
    std::fs::write(&design, ambiguous).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("ambiguous XLSX final must reject")
        .to_string();
    assert!(
        error.contains("one connected XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, XBRL, or XLSX final target"),
        "{error}"
    );

    let named = add_connected_xlsx_target(&original, false);
    std::fs::write(&design, named).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("named XLSX terminal must reject")
        .to_string();
    assert!(
        error.contains("XLSX components only as the final primary target"),
        "{error}"
    );

    let document = roxmltree::Document::parse(&original).unwrap();
    let xlsx = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("library") == Some("xlsx"))
        .unwrap();
    let mut disconnected = original.clone();
    for key in xlsx
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| entry.attribute("inpkey"))
    {
        disconnected = disconnected.replace(&format!("<edge vertexkey=\"{key}\"/>"), "");
    }
    assert_ne!(disconnected, original);
    std::fs::write(&design, disconnected).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("disconnected XLSX final must reject")
        .to_string();
    assert!(error.contains("connected"), "{error}");

    let update_existing = original.replace(
        "<excel outputinstance=\"converted.xlsx\"/>",
        "<excel outputinstance=\"converted.xlsx\" updateexistingfile=\"1\"/>",
    );
    assert_ne!(update_existing, original);
    std::fs::write(&design, update_existing).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("update-existing XLSX final must reject")
        .to_string();
    assert!(error.contains("new-workbook XLSX final target"), "{error}");
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn local_xml_to_json_mapping_runs_after_an_identity_xml_stage() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("Altova_Hierarchical_JSON.mfd");
    let input = samples.join("Altova_Hierarchical.xml");
    if !design.is_file() || !input.is_file() {
        return;
    }
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut final_project = imported.project;
    let source = format_xml::read(&input, &final_project.source).unwrap();
    let source_schema = final_project.source.clone();
    let source_options = final_project.source_options.clone();
    final_project.source_path = None;
    final_project.target_path = Some("converted.json".into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some("Altova_Hierarchical.xml".into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: mapping::Graph::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..mapping::Scope::default()
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
                id: "convert".into(),
                mapping_path: None,
                project: final_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    let hosts = BTreeMap::from([("input".into(), source)]);
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let json_before = format_json::to_string(
        &pipeline.stages[1].project.target,
        &before.stage("convert").unwrap().primary,
    )
    .unwrap();
    assert!(json_before.contains("\"Office\""));

    let directory = TempDir::new();
    std::fs::copy(&input, directory.0.join("Altova_Hierarchical.xml")).unwrap();
    let exported = directory.0.join("corpus-json-chain.mfd");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(xml.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(xml.matches("library=\"json\"").count(), 1);
    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let json_after = format_json::to_string(
        &reimported.pipeline.stages[1].project.target,
        &after
            .stage(&reimported.pipeline.stages[1].id)
            .unwrap()
            .primary,
    )
    .unwrap();
    assert_eq!(json_after, json_before);
}

#[test]
fn json_final_chain_rejects_unsupported_json_layouts_without_artifacts() {
    let directory = TempDir::new();
    let path = make_json_final_chain(&directory.0);
    let imported = mfd::import_pipeline(&path).unwrap();

    let mut intermediate_json = imported.pipeline.clone();
    intermediate_json.stages[0].project.target_path = Some("buffer.json".into());
    let destination = directory.0.join("not-created/intermediate.mfd");
    let error = mfd::export_pipeline_with_profile(
        &intermediate_json,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut named_json = imported.pipeline.clone();
    let final_project = &mut named_json.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.json".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: final_project.root.clone(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    let error =
        mfd::export_pipeline_with_profile(&named_json, &destination, mfd::ExportProfile::NativeMfd)
            .unwrap_err()
            .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut disconnected_json = imported.pipeline;
    disconnected_json.stages[1].project.root = mapping::Scope::default();
    disconnected_json.stages[1]
        .project
        .prune_unreachable_nodes();
    let destination = directory.0.join("not-created/disconnected.mfd");
    let error = mfd::export_pipeline_with_profile(
        &disconnected_json,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("pipeline export graph"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn ambiguous_or_disconnected_json_final_import_rejects() {
    let directory = TempDir::new();
    let path = make_json_final_chain(&directory.0);
    let original = std::fs::read_to_string(&path).unwrap();
    let disconnected = original.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "",
    );
    assert_ne!(disconnected, original);
    std::fs::write(&path, disconnected).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("disconnected JSON final must reject")
        .to_string();
    assert!(
        error.contains("connected XML pass-through target"),
        "{error}"
    );

    let duplicated = original.replace(
        "</children><graph>",
        r#"<component name="other" library="json" kind="31"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object"><entry name="Result" type="json-property"><entry name="string" inpkey="50"/></entry></entry></entry></entry></entry></root><json schema="target.schema.json" outputinstance="other.json"/></data></component></children><graph>"#,
    ).replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"50\"/></edges></vertex>",
    );
    assert_ne!(duplicated, original);
    std::fs::write(&path, duplicated).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("ambiguous JSON final must reject")
        .to_string();
    assert!(
        error.contains("one connected XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, XBRL, or XLSX final target"),
        "{error}"
    );
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn csv_pipeline_rejects_non_xml_intermediate_and_db_final_without_artifacts() {
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

    let mut final_db = imported.pipeline;
    final_db.stages[1].project.target_path = Some("target.db".into());
    let destination = directory.0.join("not-created/final.mfd");
    let error = mfd::export_pipeline(&final_db, &destination)
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
    assert!(
        error.contains(
            "XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, XBRL, or XLSX final target"
        ),
        "{error}"
    );
    assert!(!directory.0.join("not-created").exists());
}

fn identity_xml_to_flextext_pipeline(
    final_project: mapping::Project,
    input: &Path,
) -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    assert!(final_project.target_options.flextext.is_some());
    identity_xml_to_final_pipeline(final_project, input, "converted.txt", "flextext")
}

fn identity_xml_to_protobuf_pipeline(
    final_project: mapping::Project,
    input: &Path,
) -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    assert!(final_project.target_options.protobuf.is_some());
    identity_xml_to_final_pipeline(final_project, input, "converted.bin", "protobuf")
}

fn identity_xml_to_xbrl_pipeline(
    final_project: mapping::Project,
    input: &Path,
) -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    assert!(final_project.target_options.xbrl.is_some());
    identity_xml_to_final_pipeline(final_project, input, "filing.xbrl", "xbrl")
}

fn local_xml_with_named_csv_pipeline() -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("people-to-csv.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut final_project = imported.project;
    let csv = mapping::NamedTarget {
        name: "rows".into(),
        path: final_project.target_path.take(),
        schema: final_project.target.clone(),
        options: std::mem::take(&mut final_project.target_options),
        root: std::mem::take(&mut final_project.root),
    };
    final_project.target = final_project.source.clone();
    final_project.target_options = final_project.source_options.clone();
    final_project.root = mapping::Scope {
        construction: mapping::ScopeConstruction::CopyCurrentSource,
        ..mapping::Scope::default()
    };
    final_project.extra_targets.push(csv);
    identity_xml_to_final_pipeline(
        final_project,
        &fixtures.join("people.xml"),
        "people-copy.xml",
        "xml-and-csv",
    )
}

fn local_xml_with_named_json_pipeline() -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let imported = mfd::import(&samples.join("Altova_Hierarchical_JSON.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut final_project = imported.project;
    let json = mapping::NamedTarget {
        name: "json-report".into(),
        path: final_project.target_path.take(),
        schema: final_project.target.clone(),
        options: std::mem::take(&mut final_project.target_options),
        root: std::mem::take(&mut final_project.root),
    };
    final_project.target = final_project.source.clone();
    final_project.target_options = final_project.source_options.clone();
    final_project.root = mapping::Scope {
        construction: mapping::ScopeConstruction::CopyCurrentSource,
        ..mapping::Scope::default()
    };
    final_project.extra_targets.push(json);
    identity_xml_to_final_pipeline(
        final_project,
        &samples.join("Altova_Hierarchical.xml"),
        "hierarchy-copy.xml",
        "xml-and-json",
    )
}

fn identity_xml_to_final_pipeline(
    mut final_project: mapping::Project,
    input: &Path,
    target_path: &str,
    final_id: &str,
) -> (mapping::Pipeline, BTreeMap<String, Instance>) {
    let source = format_xml::read(input, &final_project.source).unwrap();
    let source_schema = final_project.source.clone();
    let source_options = final_project.source_options.clone();
    final_project.source_path = None;
    final_project.target_path = Some(target_path.into());
    let copy_project = mapping::Project {
        source: source_schema.clone(),
        target: source_schema,
        source_path: Some(input.file_name().unwrap().to_str().unwrap().into()),
        target_path: Some("buffer.xml".into()),
        source_options: source_options.clone(),
        target_options: source_options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: mapping::Graph::default(),
        root: mapping::Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..mapping::Scope::default()
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
                id: final_id.into(),
                mapping_path: None,
                project: final_project,
                source: PipelineInput::StageTarget {
                    stage: "copy".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    assert!(engine::validate_pipeline(&pipeline).is_empty());
    (pipeline, BTreeMap::from([("input".into(), source)]))
}

fn assert_identity_xml_to_flextext_roundtrip(design: &Path, input: &Path, expected: Option<&Path>) {
    let imported = mfd::import(design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let (pipeline, hosts) = identity_xml_to_flextext_pipeline(imported.project, input);
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_project = &pipeline.stages[1].project;
    let before_layout = final_project.target_options.flextext.as_ref().unwrap();
    let before_text = format_flextext::to_string(
        &final_project.target,
        &before.stage("flextext").unwrap().primary,
        before_layout,
    )
    .unwrap();
    assert!(!before_text.is_empty());
    if let Some(expected) = expected {
        assert_eq!(before_text, std::fs::read_to_string(expected).unwrap());
    }
    let before_parsed =
        format_flextext::from_str(&before_text, &final_project.target, before_layout).unwrap();

    let directory = TempDir::new();
    std::fs::copy(input, directory.0.join(input.file_name().unwrap())).unwrap();
    let exported = directory.0.join("flextext-chain.mfd");
    let sibling = directory.0.join("flextext-chain-stage-2-target.mft");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    assert!(!sibling.exists());
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(xml.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(xml.matches("<text type=\"txt\"").count(), 1);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let config = document
        .descendants()
        .find(|node| node.has_tag_name("text") && node.attribute("type") == Some("txt"))
        .and_then(|node| node.attribute("config"))
        .unwrap();
    assert_eq!(config, sibling.file_name().unwrap().to_str().unwrap());
    assert!(sibling.is_file());

    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_project = &reimported.pipeline.stages[1].project;
    let after_layout = final_project.target_options.flextext.as_ref().unwrap();
    let after_text = format_flextext::to_string(
        &final_project.target,
        &after
            .stage(&reimported.pipeline.stages[1].id)
            .unwrap()
            .primary,
        after_layout,
    )
    .unwrap();
    assert_eq!(after_text, before_text);
    let after_parsed =
        format_flextext::from_str(&after_text, &final_project.target, after_layout).unwrap();
    assert_eq!(after_parsed, before_parsed);
}

#[test]
fn flextext_final_chain_roundtrips_exact_text_and_generated_config() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    assert_identity_xml_to_flextext_roundtrip(
        &fixtures.join("flextext-target.mfd"),
        &fixtures.join("flextext/target-source.xml"),
        None,
    );
}

#[test]
fn local_xml_to_flextext_mapping_runs_after_an_identity_xml_stage() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("QuotationsDoc.mfd");
    let input = samples.join("Quotations.xml");
    let expected = samples.join("QuotationsDoc.txt");
    if !design.is_file() || !input.is_file() || !expected.is_file() {
        return;
    }
    assert_identity_xml_to_flextext_roundtrip(&design, &input, Some(&expected));
}

fn protobuf_layout(options: &mapping::ProtobufOptions) -> format_protobuf::Layout {
    format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )
    .unwrap()
}

fn assert_identity_xml_to_protobuf_roundtrip(design: &Path, input: &Path, expected: Option<&[u8]>) {
    let imported = mfd::import(design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let (pipeline, hosts) = identity_xml_to_protobuf_pipeline(imported.project, input);
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_project = &pipeline.stages[1].project;
    let options = final_project.target_options.protobuf.as_ref().unwrap();
    let layout = protobuf_layout(options);
    let before_bytes = format_protobuf::to_vec(
        &layout,
        &options.root_message,
        &before.stage("protobuf").unwrap().primary,
    )
    .unwrap();
    assert!(!before_bytes.is_empty());
    if let Some(expected) = expected {
        assert_eq!(before_bytes, expected);
    }
    let before_decoded =
        format_protobuf::from_slice(&layout, &options.root_message, &before_bytes).unwrap();

    let directory = TempDir::new();
    std::fs::copy(input, directory.0.join(input.file_name().unwrap())).unwrap();
    let exported = directory.0.join("protobuf-chain.mfd");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(xml.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(xml.matches("library=\"binary\"").count(), 1);
    assert_eq!(xml.matches("type=\"doc-protobuf\"").count(), 1);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let schemafile = document
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("type") == Some("doc-protobuf"))
        .and_then(|node| node.children().find(|child| child.has_tag_name("document")))
        .and_then(|node| node.attribute("schemafile"))
        .unwrap();
    assert!(schemafile.starts_with("protobuf-chain-stage-2-target"));
    let schema_path = directory.0.join(schemafile);
    assert!(schema_path.is_file());
    assert_eq!(
        std::fs::read_to_string(&schema_path).unwrap(),
        options.schema
    );

    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_project = &reimported.pipeline.stages[1].project;
    let options = final_project.target_options.protobuf.as_ref().unwrap();
    let layout = protobuf_layout(options);
    let after_bytes = format_protobuf::to_vec(
        &layout,
        &options.root_message,
        &after
            .stage(&reimported.pipeline.stages[1].id)
            .unwrap()
            .primary,
    )
    .unwrap();
    assert_eq!(after_bytes, before_bytes);
    let after_decoded =
        format_protobuf::from_slice(&layout, &options.root_message, &after_bytes).unwrap();
    assert_eq!(after_decoded, before_decoded);
}

#[test]
fn protobuf_final_chain_roundtrips_exact_binary_and_generated_schema() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let expected = [
        0x0a, 0x04, b'D', b'e', b'm', b'o', 0x12, 0x0e, 0x08, 0x07, 0x12, 0x03, b'O', b'n', b'e',
        0x18, 0x01, 0x22, 0x03, 0x0a, 0x01, b'A', 0x12, 0x0e, 0x08, 0x09, 0x12, 0x03, b'T', b'w',
        b'o', 0x18, 0x00, 0x22, 0x03, 0x0a, 0x01, b'B',
    ];
    assert_identity_xml_to_protobuf_roundtrip(
        &fixtures.join("protobuf-target.mfd"),
        &fixtures.join("protobuf-target-source.xml"),
        Some(&expected),
    );
}

#[test]
fn local_xml_to_protobuf_mapping_runs_after_an_identity_xml_stage() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("PersonsToProtobuf.mfd");
    let input = samples.join("Altova_Hierarchical.xml");
    if !design.is_file() || !input.is_file() {
        return;
    }
    assert_identity_xml_to_protobuf_roundtrip(&design, &input, None);
}

#[test]
fn xbrl_final_chain_roundtrips_exact_instance_document() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("xbrl-final.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let input = fixtures.join("xbrl-final-source.xml");
    let (pipeline, hosts) = identity_xml_to_xbrl_pipeline(imported.project, &input);
    let before = engine::run_pipeline(&pipeline, &hosts).unwrap();
    assert_eq!(before.stage("copy").unwrap().primary, hosts["input"]);
    let final_project = &pipeline.stages[1].project;
    let options = final_project.target_options.xbrl.as_ref().unwrap();
    let before_xml = format_xbrl::to_string(
        &final_project.target,
        &before.stage("xbrl").unwrap().primary,
        options,
    )
    .unwrap();
    let document = roxmltree::Document::parse(&before_xml).unwrap();
    let facts = document
        .descendants()
        .filter(|node| node.has_tag_name(("urn:ferrule:test:facts", "Label")))
        .collect::<Vec<_>>();
    assert_eq!(facts.len(), 1, "{before_xml}");
    assert_eq!(facts[0].text(), Some("reported"));
    assert!(before_xml.contains("2026-06-30"));
    assert_eq!(
        document
            .descendants()
            .filter(|node| node.has_tag_name(("http://www.xbrl.org/2003/instance", "context")))
            .count(),
        1
    );

    let directory = TempDir::new();
    for file in ["xbrl-final-source.xml", "xbrl-final-taxonomy.xsd"] {
        std::fs::copy(fixtures.join(file), directory.0.join(file)).unwrap();
    }
    let exported = directory.0.join("xbrl-chain.mfd");
    let preflight = mfd::preflight_pipeline_export(&pipeline, &exported).unwrap();
    assert!(preflight.is_native_compatible(), "{preflight:?}");
    assert!(!exported.exists());
    let report =
        mfd::export_pipeline_with_profile(&pipeline, &exported, mfd::ExportProfile::NativeMfd)
            .unwrap();
    assert!(report.is_native_compatible(), "{report:?}");
    let design = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(design.matches("PassThrough=\"1\"").count(), 1);
    assert_eq!(design.matches("library=\"xbrl\"").count(), 1);
    assert!(design.contains("schema=\"xbrl-final-taxonomy.xsd\""));

    let reimported = mfd::import_pipeline(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let PipelineInput::Host { name } = &reimported.pipeline.stages[0].source else {
        panic!("reimported first stage must read a host source");
    };
    let after = engine::run_pipeline(
        &reimported.pipeline,
        &BTreeMap::from([(name.clone(), hosts["input"].clone())]),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            before.stage(&pipeline.stages[index].id).unwrap().primary,
            after
                .stage(&reimported.pipeline.stages[index].id)
                .unwrap()
                .primary,
            "stage {}",
            index + 1
        );
    }
    let final_project = &reimported.pipeline.stages[1].project;
    let options = final_project.target_options.xbrl.as_ref().unwrap();
    let after_xml = format_xbrl::to_string(
        &final_project.target,
        &after
            .stage(&reimported.pipeline.stages[1].id)
            .unwrap()
            .primary,
        options,
    )
    .unwrap();
    assert_eq!(after_xml.as_bytes(), before_xml.as_bytes());
    let after_document = roxmltree::Document::parse(&after_xml).unwrap();
    assert_eq!(
        after_document
            .descendants()
            .find(|node| node.has_tag_name(("urn:ferrule:test:facts", "Label")))
            .and_then(|node| node.text()),
        Some("reported")
    );
}

#[test]
fn xbrl_final_chain_rejects_unsupported_boundaries_before_artifacts() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("xbrl-final.mfd")).unwrap();
    let (pipeline, _) =
        identity_xml_to_xbrl_pipeline(imported.project, &fixtures.join("xbrl-final-source.xml"));
    let directory = TempDir::new();

    let mut intermediate = pipeline.clone();
    intermediate.stages[0].project.target_path = Some("buffer.xbrl".into());
    intermediate.stages[0].project.target_options =
        pipeline.stages[1].project.target_options.clone();
    let destination = directory.0.join("not-created/intermediate.mfd");
    assert!(
        mfd::export_pipeline_with_profile(
            &intermediate,
            &destination,
            mfd::ExportProfile::NativeMfd,
        )
        .is_err()
    );
    assert!(!destination.parent().unwrap().exists());

    let mut named = pipeline.clone();
    let final_project = &mut named.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.xbrl".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: mapping::Scope::default(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    assert!(
        mfd::export_pipeline_with_profile(&named, &destination, mfd::ExportProfile::NativeMfd)
            .is_err()
    );
    assert!(!destination.parent().unwrap().exists());

    let mut presentation = pipeline.clone();
    let options = presentation.stages[1]
        .project
        .target_options
        .xbrl
        .as_ref()
        .unwrap()
        .clone();
    presentation.stages[1].project.target_options.xbrl = Some(
        mapping::XbrlBoundaryOptions::external_target(
            options.taxonomy(),
            Some("presentation/table.sps"),
        )
        .unwrap()
        .with_namespace_bindings(options.namespace_bindings().to_vec())
        .unwrap(),
    );
    let destination = directory.0.join("not-created/presentation.mfd");
    let error = mfd::export_pipeline_with_profile(
        &presentation,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("without presentation or numeric fact metadata"),
        "{error}"
    );
    assert!(!destination.parent().unwrap().exists());

    let mut disconnected = pipeline;
    disconnected.stages[1].project.root = mapping::Scope::default();
    disconnected.stages[1].project.prune_unreachable_nodes();
    let destination = directory.0.join("not-created/disconnected.mfd");
    assert!(
        mfd::export_pipeline_with_profile(
            &disconnected,
            &destination,
            mfd::ExportProfile::NativeMfd,
        )
        .is_err()
    );
    assert!(!destination.parent().unwrap().exists());
}

#[test]
fn protobuf_final_chain_rejects_unsupported_boundaries_before_artifacts() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("protobuf-target.mfd")).unwrap();
    let (pipeline, _) = identity_xml_to_protobuf_pipeline(
        imported.project,
        &fixtures.join("protobuf-target-source.xml"),
    );
    let directory = TempDir::new();

    let mut intermediate = pipeline.clone();
    intermediate.stages[0].project.target_path = Some("buffer.bin".into());
    intermediate.stages[0].project.target_options =
        pipeline.stages[1].project.target_options.clone();
    let destination = directory.0.join("not-created/intermediate.mfd");
    let error = mfd::export_pipeline_with_profile(
        &intermediate,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut named = pipeline.clone();
    let final_project = &mut named.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.bin".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: mapping::Scope::default(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    let error =
        mfd::export_pipeline_with_profile(&named, &destination, mfd::ExportProfile::NativeMfd)
            .unwrap_err()
            .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut malformed = pipeline.clone();
    malformed.stages[1]
        .project
        .target_options
        .protobuf
        .as_mut()
        .unwrap()
        .schema = "not a proto schema".into();
    let destination = directory.0.join("not-created/malformed.mfd");
    let error =
        mfd::export_pipeline_with_profile(&malformed, &destination, mfd::ExportProfile::NativeMfd)
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("embedded protobuf schema is invalid"),
        "{error}"
    );
    assert!(!destination.parent().unwrap().exists());

    let mut disconnected = pipeline;
    disconnected.stages[1].project.root = mapping::Scope::default();
    disconnected.stages[1].project.prune_unreachable_nodes();
    let destination = directory.0.join("not-created/disconnected.mfd");
    let error = mfd::export_pipeline_with_profile(
        &disconnected,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("pipeline export graph"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

fn add_connected_protobuf_target(xml: &str, schemafile: &str, default_output: bool) -> String {
    const EXTRA_KEY: &str = "4294967289";
    let document = roxmltree::Document::parse(xml).unwrap();
    let pass_through = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .unwrap();
    let vertex = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .find(|node| {
            pass_through.descendants().any(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("outkey") == node.attribute("vertexkey")
            })
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let mut with_edge = xml.to_owned();
    with_edge.insert_str(
        edges.range().end - "</edges>".len(),
        &format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
    );
    let output_property = if default_output {
        " XSLTDefaultOutput=\"1\""
    } else {
        ""
    };
    let component = format!(
        "<component name=\"other\" library=\"binary\" kind=\"33\"><properties{output_property}/><data><root><entry name=\"FileInstance\" inpkey=\"{EXTRA_KEY}\"><entry name=\"document\" type=\"doc-protobuf\"><document schemafile=\"{schemafile}\" root=\"{{ferrule.fixture}}Directory\"/><entry name=\"Directory\"/></entry></entry></root><binary outputinstance=\"other.bin\"/></data></component>"
    );
    let children_end = with_edge.rfind("</children>").unwrap();
    with_edge.insert_str(children_end, &component);
    with_edge
}

fn add_connected_xbrl_target(xml: &str, default_output: bool) -> String {
    const EXTRA_KEY: &str = "4294967289";
    let document = roxmltree::Document::parse(xml).unwrap();
    let pass_through = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .unwrap();
    let vertex = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .find(|node| {
            pass_through.descendants().any(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("outkey") == node.attribute("vertexkey")
            })
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let mut with_edge = xml.to_owned();
    with_edge.insert_str(
        edges.range().end - "</edges>".len(),
        &format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
    );
    let output_property = if default_output {
        " XSLTDefaultOutput=\"1\""
    } else {
        ""
    };
    let component = format!(
        "<component name=\"other\" library=\"xbrl\" kind=\"27\"><properties{output_property}/><data><root><entry name=\"FileInstance\"><entry name=\"document\"><entry name=\"xbrl\" inpkey=\"{EXTRA_KEY}\"/></entry></entry></root><xbrl schema=\"xbrl-final-taxonomy.xsd\" outputinstance=\"other.xbrl\"/></data></component>"
    );
    let children_end = with_edge.rfind("</children>").unwrap();
    with_edge.insert_str(children_end, &component);
    with_edge
}

fn add_connected_input_outside_target_payload(xml: &str, library: &str) -> String {
    const EXTRA_KEY: &str = "4294967289";
    let document = roxmltree::Document::parse(xml).unwrap();
    let pass_through = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .unwrap();
    let vertex = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .find(|node| {
            pass_through.descendants().any(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("outkey") == node.attribute("vertexkey")
            })
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let target = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some(library)
                && node.children().any(|child| {
                    child.has_tag_name("properties")
                        && child.attribute("XSLTDefaultOutput") == Some("1")
                })
        })
        .unwrap();
    let root = target
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
        .unwrap();
    let mut insertions = [
        (
            edges.range().end - "</edges>".len(),
            format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
        ),
        (
            root.range().end - "</root>".len(),
            format!("<entry name=\"bogus\" inpkey=\"{EXTRA_KEY}\"/>"),
        ),
    ];
    insertions.sort_by_key(|(position, _)| std::cmp::Reverse(*position));
    let mut mutated = xml.to_owned();
    for (position, insertion) in insertions {
        mutated.insert_str(position, &insertion);
    }
    mutated
}

fn add_connected_input_outside_xml_payload(
    xml: &str,
    component_name: &str,
    upstream_key: &str,
) -> String {
    const EXTRA_KEY: &str = "4294967288";
    let document = roxmltree::Document::parse(xml).unwrap();
    let component = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some(component_name)
        })
        .unwrap();
    let root = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
        .unwrap();
    let vertex = document
        .descendants()
        .find(|node| {
            node.has_tag_name("vertex") && node.attribute("vertexkey") == Some(upstream_key)
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let mut insertions = [
        (
            edges.range().end - "</edges>".len(),
            format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
        ),
        (
            root.range().end - "</root>".len(),
            format!("<entry name=\"bogus\" inpkey=\"{EXTRA_KEY}\"/>"),
        ),
    ];
    insertions.sort_by_key(|(position, _)| std::cmp::Reverse(*position));
    let mut mutated = xml.to_owned();
    for (position, insertion) in insertions {
        mutated.insert_str(position, &insertion);
    }
    mutated
}

#[test]
fn pipeline_import_rejects_unrepresented_intermediate_and_named_xml_inputs() {
    for case in ["first", "middle", "named"] {
        let directory = TempDir::new();
        let (path, component_name, upstream_key) = match case {
            "first" => (make_chain(&directory.0), "buffer", "10"),
            "middle" => (make_four_stage_chain(&directory.0), "buffer-2", "30"),
            "named" => (make_terminal_fanout(&directory.0), "secondary", "30"),
            _ => unreachable!(),
        };
        let original = std::fs::read_to_string(&path).unwrap();
        assert!(mfd::import_pipeline(&path).is_ok(), "{component_name}");
        let malformed =
            add_connected_input_outside_xml_payload(&original, component_name, upstream_key);
        std::fs::write(&path, malformed).unwrap();
        let error = mfd::import_pipeline(&path)
            .err()
            .expect("connected XML input outside the selected payload must reject")
            .to_string();
        assert!(
            error.contains("unrepresented connected input port 4294967288"),
            "{component_name}: {error}"
        );
    }
}

#[test]
fn pipeline_import_rejects_connected_inputs_outside_csv_and_json_payloads() {
    for library in ["text", "json"] {
        let directory = TempDir::new();
        let path = match library {
            "text" => make_csv_final_chain(&directory.0),
            "json" => make_json_final_chain(&directory.0),
            _ => unreachable!(),
        };
        let original = std::fs::read_to_string(&path).unwrap();
        assert!(mfd::import_pipeline(&path).is_ok(), "{library}");
        let malformed = add_connected_input_outside_target_payload(&original, library);
        std::fs::write(&path, malformed).unwrap();
        let error = mfd::import_pipeline(&path)
            .err()
            .expect("connected input outside the selected payload must reject")
            .to_string();
        assert!(
            error.contains("unrepresented connected input port 4294967289"),
            "{library}: {error}"
        );
    }
}

#[test]
fn json_null_alternative_only_skips_a_connected_typed_sibling_in_selected_payload() {
    let directory = TempDir::new();
    let path = make_json_final_chain(&directory.0);
    let original = std::fs::read_to_string(&path).unwrap();
    let original_pipeline = mfd::import_pipeline(&path).unwrap().pipeline;
    let with_null = original.replace(
        "<entry name=\"string\" inpkey=\"40\"/>",
        "<entry name=\"string\" inpkey=\"40\"/><entry name=\"null\" inpkey=\"41\"/>",
    );
    assert_ne!(with_null, original);
    let with_null = with_null.replace(
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "<vertex vertexkey=\"30\"><edges><edge vertexkey=\"40\"/><edge vertexkey=\"41\"/></edges></vertex>",
    );
    assert!(with_null.contains("<edge vertexkey=\"41\"/>"));
    std::fs::write(&path, &with_null).unwrap();
    let with_null_pipeline = mfd::import_pipeline(&path).unwrap().pipeline;
    let before = execute(&original_pipeline);
    let after = execute(&with_null_pipeline);
    for index in 0..2 {
        assert_eq!(
            before.stages[index].outputs.primary,
            after.stages[index].outputs.primary
        );
    }

    let mut outside_payload = with_null.replace(
        "<entry name=\"root\">",
        "<entry name=\"root\" inpkey=\"40\">",
    );
    assert_ne!(outside_payload, with_null);
    let insertion = {
        let document = roxmltree::Document::parse(&outside_payload).unwrap();
        document
            .descendants()
            .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("root"))
            .unwrap()
            .range()
            .end
    };
    outside_payload.insert_str(insertion, "<entry name=\"null\" inpkey=\"42\"/>");
    let outside_payload = outside_payload.replace(
        "<edge vertexkey=\"41\"/></edges></vertex>",
        "<edge vertexkey=\"41\"/><edge vertexkey=\"42\"/></edges></vertex>",
    );
    assert!(outside_payload.contains("<edge vertexkey=\"42\"/>"));
    std::fs::write(&path, outside_payload).unwrap();
    let error = mfd::import_pipeline(&path)
        .err()
        .expect("connected null outside the selected JSON payload must reject")
        .to_string();
    assert!(
        error.contains("unrepresented connected input port 42"),
        "{error}"
    );
}

#[test]
fn protobuf_final_chain_import_rejects_malformed_ambiguous_and_disconnected() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("protobuf-target.mfd")).unwrap();
    let (pipeline, _) = identity_xml_to_protobuf_pipeline(
        imported.project,
        &fixtures.join("protobuf-target-source.xml"),
    );
    let directory = TempDir::new();
    let design = directory.0.join("source-protobuf-chain.mfd");
    mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd).unwrap();
    let original = std::fs::read_to_string(&design).unwrap();
    assert!(mfd::import_pipeline(&design).is_ok());
    let schemafile = "source-protobuf-chain-stage-2-target.proto";

    let ambiguous = add_connected_protobuf_target(&original, schemafile, true);
    std::fs::write(&design, ambiguous).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("ambiguous protobuf final must reject")
        .to_string();
    assert!(error.contains("one connected XML"), "{error}");

    let named = add_connected_protobuf_target(&original, schemafile, false);
    std::fs::write(&design, named).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("named protobuf final must reject")
        .to_string();
    assert!(
        error.contains("Protocol Buffer components only as the final primary target"),
        "{error}"
    );

    let document = roxmltree::Document::parse(&original).unwrap();
    let protobuf = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("library") == Some("binary"))
        .unwrap();
    let mut disconnected = original.clone();
    for key in protobuf
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| entry.attribute("inpkey"))
    {
        disconnected = disconnected.replace(&format!("<edge vertexkey=\"{key}\"/>"), "");
    }
    assert_ne!(disconnected, original);
    std::fs::write(&design, disconnected).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("disconnected protobuf final must reject")
        .to_string();
    assert!(error.contains("connected"), "{error}");

    let wrong_kind = original.replacen("library=\"binary\"", "library=\"other-binary\"", 1);
    assert_ne!(wrong_kind, original);
    std::fs::write(&design, wrong_kind).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("other binary kind must reject")
        .to_string();
    assert!(error.contains("does not yet support"), "{error}");

    let wrong_binary_kind = original.replacen("kind=\"33\"", "kind=\"32\"", 1);
    assert_ne!(wrong_binary_kind, original);
    std::fs::write(&design, wrong_binary_kind).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("wrong binary kind must reject")
        .to_string();
    assert!(error.contains("does not yet support"), "{error}");

    let input_only = original.replacen(
        "outputinstance=\"converted.bin\"",
        "inputinstance=\"converted.bin\"",
        1,
    );
    assert_ne!(input_only, original);
    std::fs::write(&design, input_only).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("input-only binary final must reject")
        .to_string();
    assert!(error.contains("no importable target component"), "{error}");

    let extra_input = add_connected_input_outside_target_payload(&original, "binary");
    std::fs::write(&design, extra_input).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("connected input outside the protobuf message must reject")
        .to_string();
    assert!(error.contains("outside its message boundary"), "{error}");

    let missing_schema = original.replace(
        &format!("schemafile=\"{schemafile}\""),
        "schemafile=\"missing.proto\"",
    );
    assert_ne!(missing_schema, original);
    std::fs::write(&design, missing_schema).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("missing protobuf schema must reject")
        .to_string();
    assert!(error.contains("protobuf"), "{error}");
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn xbrl_final_chain_import_rejects_malformed_ambiguous_and_disconnected() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("xbrl-final.mfd")).unwrap();
    let (pipeline, _) =
        identity_xml_to_xbrl_pipeline(imported.project, &fixtures.join("xbrl-final-source.xml"));
    let directory = TempDir::new();
    std::fs::copy(
        fixtures.join("xbrl-final-taxonomy.xsd"),
        directory.0.join("xbrl-final-taxonomy.xsd"),
    )
    .unwrap();
    let design = directory.0.join("source-xbrl-chain.mfd");
    mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd).unwrap();
    let original = std::fs::read_to_string(&design).unwrap();
    assert!(mfd::import_pipeline(&design).is_ok());

    let ambiguous = add_connected_xbrl_target(&original, true);
    std::fs::write(&design, ambiguous).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("ambiguous XBRL final must reject")
        .to_string();
    assert!(error.contains("one connected XML"), "{error}");

    let named = add_connected_xbrl_target(&original, false);
    std::fs::write(&design, named).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("named XBRL final must reject")
        .to_string();
    assert!(
        error.contains("XBRL components only as the final primary target"),
        "{error}"
    );

    let document = roxmltree::Document::parse(&original).unwrap();
    let target = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("library") == Some("xbrl"))
        .unwrap();
    let mut disconnected = original.clone();
    for key in target
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| entry.attribute("inpkey"))
    {
        disconnected = disconnected.replace(&format!("<edge vertexkey=\"{key}\"/>"), "");
    }
    assert_ne!(disconnected, original);
    std::fs::write(&design, disconnected).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("disconnected XBRL final must reject")
        .to_string();
    assert!(error.contains("connected"), "{error}");

    let wrong_kind = original.replacen("kind=\"27\"", "kind=\"26\"", 1);
    assert_ne!(wrong_kind, original);
    std::fs::write(&design, wrong_kind).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("wrong XBRL kind must reject")
        .to_string();
    assert!(error.contains("does not yet support"), "{error}");

    let presentation = original.replacen(
        "<xbrl schema=\"xbrl-final-taxonomy.xsd\"",
        "<xbrl schema=\"xbrl-final-taxonomy.xsd\" sps=\"presentation/table.sps\"",
        1,
    );
    assert_ne!(presentation, original);
    std::fs::write(&design, presentation).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("presentation-backed XBRL final must reject")
        .to_string();
    assert!(error.contains("does not yet support"), "{error}");

    let extra_input = add_connected_input_outside_target_payload(&original, "xbrl");
    std::fs::write(&design, extra_input).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("connected XBRL input outside the payload must reject")
        .to_string();
    assert!(
        error.contains("unrepresented connected input port"),
        "{error}"
    );
    assert!(!directory.0.join("not-created").exists());
}

#[test]
fn flextext_final_chain_rejects_unsupported_boundaries_before_artifacts() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("flextext-target.mfd")).unwrap();
    let (pipeline, _) = identity_xml_to_flextext_pipeline(
        imported.project,
        &fixtures.join("flextext/target-source.xml"),
    );
    let directory = TempDir::new();

    let mut intermediate = pipeline.clone();
    intermediate.stages[0].project.target_path = Some("buffer.txt".into());
    intermediate.stages[0].project.target_options =
        pipeline.stages[1].project.target_options.clone();
    let destination = directory.0.join("not-created/intermediate.mfd");
    let error = mfd::export_pipeline_with_profile(
        &intermediate,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unsupported file boundary"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut named = pipeline.clone();
    let final_project = &mut named.stages[1].project;
    final_project.extra_targets.push(mapping::NamedTarget {
        name: "secondary".into(),
        path: Some("secondary.txt".into()),
        schema: final_project.target.clone(),
        options: final_project.target_options.clone(),
        root: final_project.root.clone(),
    });
    let destination = directory.0.join("not-created/named.mfd");
    let error =
        mfd::export_pipeline_with_profile(&named, &destination, mfd::ExportProfile::NativeMfd)
            .unwrap_err()
            .to_string();
    assert!(error.contains("non-file-XML named target"), "{error}");
    assert!(!destination.parent().unwrap().exists());

    let mut unsupported = pipeline;
    let layout = unsupported.stages[1]
        .project
        .target_options
        .flextext
        .as_ref()
        .unwrap()
        .clone();
    let mapping::FlexCommand::SplitOnce {
        name,
        first,
        second,
        ..
    } = layout.command()
    else {
        panic!("fixture must have a single split");
    };
    unsupported.stages[1].project.target_options.flextext = Some(
        mapping::FlexTextLayout::new(
            layout.root_name(),
            mapping::FlexCommand::SplitOnce {
                name: name.clone(),
                splitter: mapping::OnceSplitter::LineStartingWith("ITEM".into()),
                first: first.clone(),
                second: second.clone(),
            },
            layout.output_line_ending(),
            layout.write_bom(),
        )
        .unwrap(),
    );
    let destination = directory.0.join("not-created/unsupported.mfd");
    let error = mfd::export_pipeline_with_profile(
        &unsupported,
        &destination,
        mfd::ExportProfile::NativeMfd,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("line-starting single split"), "{error}");
    assert!(!destination.parent().unwrap().exists());
}

fn add_connected_flextext_target(xml: &str, config: &str, default_output: bool) -> String {
    const EXTRA_KEY: &str = "4294967289";
    let document = roxmltree::Document::parse(xml).unwrap();
    let pass_through = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.children().any(|child| {
                    child.has_tag_name("properties") && child.attribute("PassThrough") == Some("1")
                })
        })
        .unwrap();
    let vertex = document
        .descendants()
        .filter(|node| node.has_tag_name("vertex"))
        .find(|node| {
            pass_through.descendants().any(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("outkey") == node.attribute("vertexkey")
            })
        })
        .unwrap();
    let edges = vertex
        .children()
        .find(|node| node.has_tag_name("edges"))
        .unwrap();
    let mut with_edge = xml.to_owned();
    with_edge.insert_str(
        edges.range().end - "</edges>".len(),
        &format!("<edge vertexkey=\"{EXTRA_KEY}\"/>"),
    );
    let output_property = if default_output {
        " XSLTDefaultOutput=\"1\""
    } else {
        ""
    };
    let component = format!(
        "<component name=\"other\" library=\"text\" kind=\"16\"><properties{output_property}/><data><root><entry name=\"FileInstance\"><entry name=\"document\"><entry name=\"Sections\" inpkey=\"{EXTRA_KEY}\"/></entry></entry></root><text type=\"txt\" config=\"{config}\" outputinstance=\"other.txt\"/></data></component>"
    );
    let children_end = with_edge.rfind("</children>").unwrap();
    with_edge.insert_str(children_end, &component);
    with_edge
}

#[test]
fn flextext_final_chain_import_rejects_malformed_ambiguous_and_disconnected() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let imported = mfd::import(&fixtures.join("flextext-target.mfd")).unwrap();
    let (pipeline, _) = identity_xml_to_flextext_pipeline(
        imported.project,
        &fixtures.join("flextext/target-source.xml"),
    );
    let directory = TempDir::new();
    let design = directory.0.join("source-flextext-chain.mfd");
    mfd::export_pipeline_with_profile(&pipeline, &design, mfd::ExportProfile::NativeMfd).unwrap();
    let original = std::fs::read_to_string(&design).unwrap();
    assert!(mfd::import_pipeline(&design).is_ok());
    let config = "source-flextext-chain-stage-2-target.mft";

    let ambiguous = add_connected_flextext_target(&original, config, true);
    std::fs::write(&design, ambiguous).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("ambiguous FlexText final must reject")
        .to_string();
    assert!(
        error.contains("one connected XML, CSV, fixed-width, FlexText, JSON, Protocol Buffers, XBRL, or XLSX final target"),
        "{error}"
    );

    let named = add_connected_flextext_target(&original, config, false);
    std::fs::write(&design, named).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("named FlexText terminal must reject")
        .to_string();
    assert!(
        error.contains(
            "CSV, fixed-width text, and FlexText components only as the final primary target"
        ),
        "{error}"
    );

    let document = roxmltree::Document::parse(&original).unwrap();
    let flextext = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("text")
                && node.descendants().any(|entry| {
                    entry.has_tag_name("text") && entry.attribute("type") == Some("txt")
                })
        })
        .unwrap();
    let mut disconnected = original.clone();
    for key in flextext
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| entry.attribute("inpkey"))
    {
        disconnected = disconnected.replace(&format!("<edge vertexkey=\"{key}\"/>"), "");
    }
    assert_ne!(disconnected, original);
    std::fs::write(&design, disconnected).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("disconnected FlexText final must reject")
        .to_string();
    assert!(error.contains("connected"), "{error}");

    let no_config = original.replace(&format!("config=\"{config}\""), "config=\"\"");
    assert_ne!(no_config, original);
    std::fs::write(&design, no_config).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("empty FlexText configuration path must reject")
        .to_string();
    assert!(error.contains("final target"), "{error}");

    let missing_config =
        original.replace(&format!("config=\"{config}\""), "config=\"missing.mft\"");
    assert_ne!(missing_config, original);
    std::fs::write(&design, missing_config).unwrap();
    let error = mfd::import_pipeline(&design)
        .err()
        .expect("missing FlexText configuration must reject")
        .to_string();
    assert!(error.contains("text/flextext"), "{error}");
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
