use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "json-parameter-projection-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("schema.json"),
            r#"{"type":"object","properties":{"key":{"type":["string","number","null"]},"unused":{"type":"string"}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("mapping.mfd"),
            r#"<mapping version="36"><component name="map" uid="1"><structure><children>
<component name="Variables" library="core" uid="2" kind="6"><sources><datapoint/></sources><targets><datapoint pos="0" key="1"/></targets><data><input datatype="string" previewvalue="never-open-preview.json" usepreviewvalue="1"/><parameter usageKind="input" name="Variables"/></data></component>
<component name="Parsed" library="json" uid="3" kind="31"><properties XSLTTargetEncoding="UTF-8"/><data><parameter usageKind="stringparse"/><root><entry name="FileInstance" inpkey="50"><file role="inputinstance" name="never-open-instance.json"/><entry name="document"><entry name="root"><entry name="object" outkey="10"><entry name="key" type="json-property"><entry name="string" outkey="11"/></entry></entry></entry></entry></entry></root><json schema="schema.json"/></data></component>
</children><graph><vertices><vertex vertexkey="1"><edges><edge vertexkey="50"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
        )
        .unwrap();
        Self(root)
    }

    fn change(&self, from: &str, to: &str) {
        let path = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from));
        std::fs::write(path, text.replace(from, to)).unwrap();
    }

    fn import(&self) -> Result<mfd::ImportedJsonParameterProjection, mfd::MfdError> {
        mfd::import_json_parameter_projection_with_options(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for file in ["mapping.mfd", "schema.json"] {
            std::fs::remove_file(self.0.join(file)).unwrap();
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}

#[test]
fn explicit_parameter_projection_keeps_identity_and_does_not_acquire_preview_files() {
    let fixture = Fixture::new();
    let original = std::fs::read(fixture.0.join("mapping.mfd")).unwrap();
    let imported = fixture.import().unwrap();
    assert_eq!(imported.parameter_name, "Variables");
    assert_eq!(imported.input_port, 50);
    assert_eq!(
        imported.parameter_preview.as_deref(),
        Some("never-open-preview.json")
    );
    assert_eq!(imported.ports[&10], ["Documents", "Root"]);
    assert_eq!(imported.ports[&11], ["Documents", "Root", "key"]);
    assert!(imported.source_schema.child("unused").is_some());
    let raw = codegen_runtime::JsonSourceDocument::parse(
        "Variables",
        br#"{"key":"a","key":1,"key":"a","key":"b"}"#,
    )
    .unwrap();
    let projected =
        codegen_runtime::project_json_source_document(&imported.shape, &raw, Default::default())
            .unwrap();
    let roots = projected.field("Root").unwrap().as_repeated().unwrap();
    let values: Vec<_> = roots[0]
        .field("key")
        .unwrap()
        .as_repeated()
        .unwrap()
        .iter()
        .map(|item| item.field("Value").unwrap().clone())
        .collect();
    assert_eq!(
        values,
        [
            ir::Instance::Scalar(ir::Value::String("a".into())),
            ir::Instance::Scalar(ir::Value::String("a".into())),
            ir::Instance::Scalar(ir::Value::String("b".into())),
        ]
    );
    assert_eq!(
        std::fs::read(fixture.0.join("mapping.mfd")).unwrap(),
        original
    );
    assert!(
        mfd::import_json_source_projection_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
        .is_err()
    );
}

#[test]
fn parameter_projection_does_not_mistake_a_physical_source_for_stringparse() {
    let fixture = Fixture::new();
    fixture.change(
        "</children>",
        r#"<component name="Physical" library="json" uid="4" kind="31"><properties DynamicFilenames="1" XSLTTargetEncoding="UTF-8"/><data><root><entry name="FileInstance" inpkey="150"><entry name="document"><entry name="root"><entry name="object" outkey="110"><entry name="key" type="json-property"><entry name="string" outkey="111"/></entry></entry></entry></entry></entry></root><json schema="schema.json"/></data></component></children>"#,
    );
    let parameter = fixture.import().unwrap();
    assert_eq!(parameter.input_port, 50);
    assert!(parameter.ports.contains_key(&11));
    assert!(!parameter.ports.contains_key(&111));
    assert!(
        mfd::import_json_source_projection_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
        .is_err()
    );
}

#[test]
fn parameter_projection_keeps_absence_and_raw_string_selection() {
    let fixture = Fixture::new();
    let imported = fixture.import().unwrap();
    for (text, roots, strings) in [
        ("null", 0, 0),
        ("[]", 0, 0),
        ("42", 0, 0),
        ("true", 0, 0),
        (r#""text""#, 0, 0),
        ("{}", 1, 0),
        (r#"{"Key":"value"}"#, 1, 0),
        (r#"{"key":null}"#, 1, 0),
        (r#"{"key":42}"#, 1, 0),
        (r#"{"key":true}"#, 1, 0),
        (r#"{"key":[]}"#, 1, 0),
        (r#"{"key":{}}"#, 1, 0),
        (r#"{"key":""}"#, 1, 1),
        (r#"{"key":null,"key":"value","key":42}"#, 1, 1),
    ] {
        let raw = codegen_runtime::JsonSourceDocument::parse("Variables", text.as_bytes()).unwrap();
        let projected = codegen_runtime::project_json_source_document(
            &imported.shape,
            &raw,
            Default::default(),
        )
        .unwrap();
        let values = projected.field("Root").unwrap().as_repeated().unwrap();
        assert_eq!(values.len(), roots, "{text}");
        if let Some(object) = values.first() {
            assert_eq!(
                object.field("key").unwrap().as_repeated().unwrap().len(),
                strings,
                "{text}"
            );
        }
    }
}

#[test]
fn parameter_projection_refuses_filename_semantics_and_unqualified_inputs() {
    for (from, to) in [
        (
            r#"<properties XSLTTargetEncoding="UTF-8"/>"#,
            r#"<properties XSLTTargetEncoding="UTF-8" DynamicFilenames="1"/>"#,
        ),
        (
            r#"<entry name="FileInstance" inpkey="50">"#,
            r#"<entry name="FileInstance" inpkey="50" outkey="51">"#,
        ),
        (
            r#"<parameter usageKind="stringparse"/>"#,
            r#"<parameter usageKind="stringparse" unsupported="1"/>"#,
        ),
        (
            r#"<parameter usageKind="stringparse"/>"#,
            r#"<parameter usageKind="stringparse"/><parameter usageKind="stringparse"/>"#,
        ),
        (
            r#"<parameter usageKind="stringparse"/>"#,
            r#"<parameter usageKind="variable"/>"#,
        ),
        (
            r#"<json schema="schema.json"/>"#,
            r#"<json schema="schema.json"/><json schema="schema.json"/>"#,
        ),
        (
            r#"<properties XSLTTargetEncoding="UTF-8"/>"#,
            r#"<properties XSLTTargetEncoding="UTF-8"/><properties/>"#,
        ),
        (
            r#"<parameter usageKind="stringparse"/>"#,
            r#"<parameter usageKind="stringparse"/><unexpected/>"#,
        ),
        (
            r#"datatype="string" previewvalue"#,
            r#"datatype="decimal" previewvalue"#,
        ),
        (r#"<edge vertexkey="50"/>"#, ""),
    ] {
        let fixture = Fixture::new();
        fixture.change(from, to);
        assert!(fixture.import().is_err(), "accepted {to}");
    }
}
