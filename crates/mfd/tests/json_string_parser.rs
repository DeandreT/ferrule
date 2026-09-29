use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_string_parser_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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
fn imports_executes_and_round_trips_a_connected_json_string_parser() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Payload" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("payload.schema.json"),
        r#"{"type":"object","properties":{"Shares":{"type":"integer"},"Leaves":{"type":"object","properties":{"Total":{"type":"number"}}}}}"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Report"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Shares" type="xs:integer"/><xs:element name="Total" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Row" outkey="10"><entry name="Payload" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="payload" library="json" kind="31"><data><root><entry name="FileInstance" inpkey="20"><entry name="document"><entry name="root"><entry name="object"><entry name="Shares" type="json-property"><entry name="number" outkey="21"/></entry><entry name="Leaves" type="json-property"><entry name="object"><entry name="Total" type="json-property"><entry name="number" outkey="22"/></entry></entry></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="payload.schema.json" inputinstance="missing-design-time.json"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Report"><entry name="Row" inpkey="30"><entry name="Shares" inpkey="31"/><entry name="Total" inpkey="32"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.source_path.as_deref(), Some("source.xml"));
    assert!(imported.project.extra_sources.is_empty());
    assert!(engine::validate(&imported.project).is_empty());

    let input = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "Payload".into(),
            Instance::Scalar(Value::String(
                r#"{"Shares":7,"Leaves":{"Total":3.5}}"#.into(),
            )),
        )])]),
    )]);
    let output = engine::run(&imported.project, &input).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].field("Shares").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        rows[0].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(3.5))
    );

    let roundtrip_path = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let design = std::fs::read_to_string(&roundtrip_path).unwrap();
    assert_eq!(design.matches("usageKind=\"stringparse\"").count(), 1);
    assert!(design.contains("library=\"json\""));
    assert!(design.contains("roundtrip-json-parser-"));
    assert!(!design.contains("name=\"json_parse_field\""));
    assert!(
        std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("roundtrip-json-parser-") && name.ends_with(".schema.json")
                })
            })
    );

    let roundtrip = mfd::import(&roundtrip_path).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(engine::run(&roundtrip.project, &input).unwrap(), output);

    let mut unsupported = imported.project;
    let Some((_, Node::Call { args, .. })) = unsupported.graph.nodes.iter().find(|(_, node)| {
        matches!(
            node,
            Node::Call { function, .. } if function == "json_parse_field"
        )
    }) else {
        panic!("imported project must contain a parser field call");
    };
    let descriptor = args[2];
    unsupported.graph.nodes.insert(
        descriptor,
        Node::SourceField {
            path: vec!["Payload".into()],
            frame: Some(vec!["Row".into()]),
        },
    );
    let unsupported_path = dir.0.join("unsupported.mfd");
    let warnings = mfd::export(&unsupported, &unsupported_path).unwrap();
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("JSON string parser")
                && warning.contains("field path descriptor")
                && warning.contains("not a string literal")
        }),
        "{warnings:?}"
    );
}

#[test]
fn missing_string_parser_schema_provenance_survives_without_changing_execution() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Payload" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Report"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("missing.mfd"),
        r#"<mapping><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Payload" outkey="11"/></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="payload" library="json" kind="31"><data><root><entry name="FileInstance" inpkey="20"><entry name="document"><entry name="root"><entry name="object"><entry name="Value" type="json-property"><entry name="string" outkey="21"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="missing&amp;parser.schema.json"/></data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Report"><entry name="Value" inpkey="31"/></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("missing.mfd")).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("missing&parser.schema.json")
            && warning.contains("falling back to the entry tree")
    }));
    let original = &imported.project;
    let serialized = serde_json::to_string(original).unwrap();
    let stored: mapping::Project = serde_json::from_str(&serialized).unwrap();
    let schema_text = stored
        .graph
        .nodes
        .values()
        .find_map(|node| {
            let Node::Call { function, args } = node else {
                return None;
            };
            if function != "json_parse_field" {
                return None;
            }
            let Node::Const {
                value: Value::String(schema),
            } = stored.graph.nodes.get(&args[1])?
            else {
                return None;
            };
            Some(schema)
        })
        .expect("parser schema descriptor");
    let mut descriptor: serde_json::Value = serde_json::from_str(schema_text).unwrap();
    assert_eq!(descriptor["ferrule:json-parser-recipe"]["version"], 1);
    assert_eq!(
        descriptor["ferrule:json-parser-recipe"]["unresolved_schema_reference"],
        "missing&parser.schema.json"
    );
    let runtime_schema: ir::SchemaNode = serde_json::from_str(schema_text).unwrap();
    descriptor
        .as_object_mut()
        .unwrap()
        .remove("ferrule:json-parser-recipe");
    let bare_schema: ir::SchemaNode = serde_json::from_value(descriptor).unwrap();
    assert_eq!(runtime_schema, bare_schema);

    let input = Instance::Group(vec![(
        "Payload".into(),
        Instance::Scalar(Value::String(r#"{"Value":"kept"}"#.into())),
    )]);
    let expected = engine::run(original, &input).unwrap();
    assert_eq!(engine::run(&stored, &input).unwrap(), expected);
    let lowered = codegen::lower(&stored).unwrap();
    codegen_rust::emit(
        &lowered,
        &codegen_rust::Options {
            package_name: "parser-provenance".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .unwrap();
    codegen_csharp::emit(&lowered).unwrap();

    let destination = dir.0.join("roundtrip.mfd");
    let report = mfd::preflight_export(&stored, &destination).unwrap();
    assert_eq!(report.compatibility, mfd::ExportCompatibility::Incomplete);
    assert!(report.issues.iter().any(|issue| {
        issue.feature == mfd::ExportCompatibilityFeature::UnresolvedJsonSchema
            && issue.component == "payload"
    }));
    assert!(report.warnings.iter().any(|warning| {
        warning.contains("JSON string parser") && warning.contains("missing&parser.schema.json")
    }));
    assert!(matches!(
        mfd::export_with_profile(&stored, &destination, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!destination.exists());
    assert_eq!(mfd::export(&stored, &destination).unwrap(), report.warnings);
    let encoded = std::fs::read_to_string(&destination).unwrap();
    assert!(encoded.contains("ferrule-unresolved-json-schema=\"missing&amp;parser.schema.json\""));
    let reimported = mfd::import(&destination).unwrap();
    assert!(reimported.warnings.iter().any(|warning| {
        warning.contains("missing&parser.schema.json") && warning.contains("entry-tree fallback")
    }));
    assert_eq!(engine::run(&reimported.project, &input).unwrap(), expected);

    for (field, replacement, expected_error) in [
        (
            "version",
            serde_json::json!(2),
            "metadata version is unsupported",
        ),
        (
            "version",
            serde_json::json!("bad"),
            "recipe metadata is invalid",
        ),
        (
            "unresolved_schema_reference",
            serde_json::json!(42),
            "recipe metadata is invalid",
        ),
        (
            "unresolved_schema_reference",
            serde_json::json!("x".repeat(4097)),
            "1 to 4096 bytes",
        ),
    ] {
        let mut invalid = stored.clone();
        let Some(Node::Const {
            value: Value::String(schema),
        }) = invalid.graph.nodes.values_mut().find(|node| {
            matches!(node, Node::Const { value: Value::String(text) } if text.contains("ferrule:json-parser-recipe"))
        }) else {
            panic!("parser schema descriptor");
        };
        let mut value: serde_json::Value = serde_json::from_str(schema).unwrap();
        value["ferrule:json-parser-recipe"][field] = replacement;
        *schema = serde_json::to_string(&value).unwrap();
        assert!(
            mfd::preflight_export(&invalid, &dir.0.join("invalid.mfd"))
                .unwrap_err()
                .to_string()
                .contains(expected_error)
        );
    }
}
