use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DocumentProject, Graph, JsonSourceShape, Node, PrimarySourcePlan, Project, Scope,
};

fn document(shape: &JsonSourceShape) -> DocumentProject {
    DocumentProject::new(
        Project {
            source: shape.document_set_schema().unwrap(),
            target: SchemaNode::group("Target", vec![SchemaNode::scalar("ok", ScalarType::Bool)]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: BTreeMap::new(),
            graph: Graph {
                nodes: BTreeMap::from([
                    (
                        0,
                        Node::RuntimeParameter {
                            name: "InputPath".into(),
                            ty: ScalarType::String,
                            preview: Some("never-open-preview.json".into()),
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::Bool(true),
                        },
                    ),
                ]),
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "ok".into(),
                    node: 1,
                }],
                ..Default::default()
            },
        },
        PrimarySourcePlan { filename: 0 },
    )
    .with_json_source_projection(shape.clone())
    .unwrap()
}

#[test]
fn both_backends_emit_the_explicit_lazy_projection_companion() {
    let shape = JsonSourceShape::Number;
    let document = document(&shape);
    let program = codegen::lower_document(&document).unwrap();
    let rust = codegen_rust::emit_with_primary_json_projection(
        &program,
        &codegen_rust::Options {
            package_name: "json_projection_control".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
        },
        &shape,
    )
    .unwrap();
    let csharp = codegen_csharp::emit_with_primary_json_projection(&program, &shape).unwrap();
    let rust_source = String::from_utf8(
        rust.into_files()
            .into_iter()
            .find(|f| f.path.as_str() == "src/lib.rs")
            .unwrap()
            .contents,
    )
    .unwrap();
    let csharp_source = String::from_utf8(
        csharp
            .into_files()
            .into_iter()
            .find(|f| f.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert!(rust_source.contains("pub fn acquire_primary_json"));
    assert!(rust_source.contains("move || evaluate_primary_filename(execution)"));
    assert!(csharp_source.contains("AcquirePrimaryJson("));
    assert!(csharp_source.contains("() => EvaluatePrimaryFilename(execution)"));
    assert!(codegen::prepare_json_source_projection(&program, &JsonSourceShape::String).is_err());
    let restored: DocumentProject =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    assert_eq!(restored.json_source_projection(), Some(&shape));
    assert!(
        codegen::prepare_json_source_projection(
            &codegen::lower_document(&restored).unwrap(),
            &shape
        )
        .is_ok()
    );
}

#[test]
fn source_import_preserves_typed_paths_and_never_uses_preview_bytes() {
    let root = std::env::temp_dir().join(format!(
        "json-projection-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.json"), r#"{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object","properties":{"n":{"type":"number"},"unused":{"type":"string"}}}}}}"#).unwrap();
    let text = r#"<mapping version="36"><component name="map" uid="1"><structure><children>
      <component name="InputPath" library="core" uid="2" kind="6"><sources><datapoint/></sources><targets><datapoint pos="0" key="1"/></targets><data><input datatype="string" previewvalue="must-not-open.json" usepreviewvalue="1"/><parameter usageKind="input" name="InputPath"/></data></component>
      <component name="Source" library="json" kind="31" uid="3"><properties DynamicFilenames="1" XSLTTargetEncoding="UTF-8"/><data><root><entry name="FileInstance" inpkey="50" outkey="51"><entry name="document"><entry name="root"><entry name="object"><entry name="rows" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="10"><entry name="n" type="json-property"><entry name="number" outkey="11"/></entry></entry></entry></entry></entry></entry></entry></entry></entry></root><json schema="source.json"/></data></component>
      </children><graph><vertices><vertex vertexkey="1"><edges><edge vertexkey="50"/></edges></vertex></vertices></graph></structure></component></mapping>"#;
    let mapping = root.join("source.mfd");
    std::fs::write(&mapping, text).unwrap();
    let result =
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .unwrap();
    assert_eq!(result.filename_parameter, "InputPath");
    assert_eq!(result.filename_output_port, Some(51));
    assert_eq!(
        result.filename_preview.as_deref(),
        Some("must-not-open.json")
    );
    assert_eq!(
        result.ports[&11],
        ["Documents", "Root", "rows", "Items", "n"]
    );
    assert!(
        result
            .source_schema
            .child("rows")
            .unwrap()
            .child("unused")
            .is_some()
    );
    assert_eq!(std::fs::read_to_string(&mapping).unwrap(), text);
    let no_filename_output = text.replace(" inpkey=\"50\" outkey=\"51\"", " inpkey=\"50\"");
    assert_ne!(no_filename_output, text);
    std::fs::write(&mapping, &no_filename_output).unwrap();
    let without_output =
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .unwrap();
    assert_eq!(without_output.filename_output_port, None);
    assert_eq!(
        without_output.filename_input_port,
        result.filename_input_port
    );
    assert_eq!(without_output.shape, result.shape);
    assert_eq!(without_output.ports, result.ports);
    let original_schema = std::fs::read_to_string(root.join("source.json")).unwrap();
    for (entry, schema_types, selected) in [
        (
            "number",
            r#"["number","string","null"]"#,
            JsonSourceShape::Number,
        ),
        (
            "string",
            r#"["number","string","null"]"#,
            JsonSourceShape::String,
        ),
        (
            "boolean",
            r#"["boolean","string","null"]"#,
            JsonSourceShape::Boolean,
        ),
    ] {
        let schema =
            original_schema.replace(r#""type":"number""#, &format!(r#""type":{schema_types}"#));
        std::fs::write(root.join("source.json"), &schema).unwrap();
        let mapping_text = text.replace(r#"name="number""#, &format!(r#"name="{entry}""#));
        std::fs::write(&mapping, &mapping_text).unwrap();
        let projected = mfd::import_json_source_projection_with_options(
            &mapping,
            &mfd::ImportOptions::default(),
        )
        .unwrap();
        assert_eq!(projected.ports, result.ports);
        let JsonSourceShape::Object { properties } = &projected.shape else {
            panic!("object root");
        };
        let JsonSourceShape::Array { item } = &properties[0].shape else {
            panic!("array");
        };
        let JsonSourceShape::Object { properties } = item.as_ref() else {
            panic!("object item");
        };
        assert_eq!(properties[0].shape, selected);
        assert!(matches!(
            projected
                .source_schema
                .child("rows")
                .unwrap()
                .child("n")
                .unwrap()
                .kind,
            ir::SchemaKind::ScalarUnion { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(root.join("source.json")).unwrap(),
            schema
        );
        assert_eq!(std::fs::read_to_string(&mapping).unwrap(), mapping_text);
    }
    std::fs::write(
        root.join("source.json"),
        original_schema.replace(r#""type":"number""#, r#""type":["boolean","string"]"#),
    )
    .unwrap();
    std::fs::write(&mapping, text).unwrap();
    assert!(
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .is_err()
    );
    std::fs::write(root.join("source.json"), &original_schema).unwrap();
    std::fs::write(&mapping, text.replace("outkey=\"51\"", "outkey=\"\"")).unwrap();
    assert!(
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .is_err()
    );
    std::fs::write(
        &mapping,
        text.replace("<json schema=", "<json json5=\"1\" schema="),
    )
    .unwrap();
    assert!(
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .is_err()
    );
    std::fs::write(&mapping, text.replace("name=\"number\"", "name=\"string\"")).unwrap();
    assert!(
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .is_err()
    );
    std::fs::write(
        &mapping,
        text.replace("name=\"number\"", "name=\"number\" default=\"17\""),
    )
    .unwrap();
    assert!(
        mfd::import_json_source_projection_with_options(&mapping, &mfd::ImportOptions::default())
            .is_err()
    );
    for file in ["source.mfd", "source.json"] {
        std::fs::remove_file(root.join(file)).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}
