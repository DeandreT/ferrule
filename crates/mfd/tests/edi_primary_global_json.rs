use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, SchemaKind, SchemaNode, Value};
use mapping::{Node, Project, Scope};

#[path = "fixtures/edi-primary-global-json/project_oracle.rs"]
mod project_oracle;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edi-primary-global-json")
        .join(name)
}

// These literals describe typed EDI values, including Null for empty elements.
// They are not an ordinary JSON input schema or a document-set X12 adapter.
fn typed(value: &serde_json::Value, schema: &SchemaNode) -> Instance {
    if let Some(values) = value.get("typed_root_sequence") {
        assert_eq!(value.as_object().unwrap().len(), 1);
        return Instance::Repeated(
            values
                .as_array()
                .unwrap()
                .iter()
                .map(|value| typed(value, schema))
                .collect(),
        );
    }
    if schema.repeating {
        let mut item = schema.clone();
        item.repeating = false;
        return Instance::Repeated(
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|value| typed(value, &item))
                .collect(),
        );
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => {
            let object = value.as_object().unwrap();
            assert_eq!(object.len(), children.len());
            Instance::Group(
                children
                    .iter()
                    .map(|child| (child.name.clone(), typed(&object[&child.name], child)))
                    .collect::<Vec<_>>()
                    .into(),
            )
        }
        SchemaKind::Scalar { .. } => Instance::Scalar(match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::String(value) => Value::String(value.clone()),
            serde_json::Value::Number(value) => Value::Int(value.as_i64().unwrap()),
            serde_json::Value::Bool(value) => Value::Bool(*value),
            _ => panic!("unsupported authored scalar"),
        }),
        _ => panic!("unsupported authored EDI schema"),
    }
}

fn verify(project: &Project, source: &Instance, case: &serde_json::Value) {
    let original_source = source.clone();
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().unwrap() {
        let value = match value {
            serde_json::Value::String(value) => Value::String(value.clone()),
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(value) => Value::Bool(*value),
            serde_json::Value::Number(value) => Value::Int(value.as_i64().unwrap()),
            _ => panic!("unsupported authored parameter: {case}"),
        };
        parameters.insert(name, value).unwrap();
    }
    let context = engine::ExecutionContext::new(Path::new("edi-primary-global-json.mfd"))
        .with_parameters(&parameters);
    let result = engine::run_with_context(project, source, &context);
    if let Some(expected) = case.get("expected_error") {
        match expected["kind"].as_str() {
            Some("MissingRuntimeParameter") => assert!(
                matches!(result,
                Err(engine::EngineError::MissingRuntimeParameter { node, name })
                if node == expected["node"].as_u64().unwrap() as u32 && name == expected["name"].as_str().unwrap()),
                "{case}"
            ),
            Some("MappingException") => assert!(
                matches!(result,
                Err(engine::EngineError::MappingException { node, message })
                if node == expected["node"].as_u64().unwrap() as u32 && message.as_deref() == expected["message"].as_str()),
                "{case}"
            ),
            None => assert!(
                matches!(result,
                Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, message }))
                if function == expected["function"].as_str().unwrap() && message == expected["message"].as_str().unwrap()),
                "{case}"
            ),
            _ => panic!("unsupported authored error: {case}"),
        }
    } else {
        let result =
            result.unwrap_or_else(|error| panic!("unexpected runtime error for {case}: {error:?}"));
        if let Some(field) = case.get("expected_typed_null_field") {
            assert_eq!(
                result.field(field.as_str().unwrap()),
                Some(&Instance::Scalar(Value::Null)),
                "typed absence changed before JSON serialization: {case}"
            );
        }
        assert_eq!(
            format_json::to_value(&project.target, &result).unwrap(),
            case["expected_complete_json"],
            "{case}"
        );
    }
    assert_eq!(source, &original_source, "caller source changed: {case}");
}

#[test]
fn required_global_parser_keeps_whole_projects_and_typed_x12_values() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture("cases.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 36);
    for design in ["root", "nested", "rows", "scalar"] {
        let imported = mfd::import(&fixture(&format!("required-edi-{design}.mfd"))).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(project_oracle::expected(design)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let encoded = mapping::project_file::encode_pretty(&imported.project).unwrap();
        let restored = mapping::project_file::decode_str(&encoded).unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&imported.project).unwrap()
        );
        for case in cases.iter().filter(|case| case["design"] == design) {
            let source = typed(&case["primary_typed_value"], &imported.project.source);
            verify(&imported.project, &source, case);
            if let Some(document) = case.get("input_x12") {
                let document = document.as_str().unwrap();
                let text = format_edi::x12::from_str(
                    document,
                    &imported.project.source,
                    imported.project.source_options.lenient_segments,
                )
                .unwrap();
                let bytes = document.as_bytes();
                let utf8 = std::str::from_utf8(bytes).unwrap();
                let byte_source = format_edi::x12::from_str(
                    utf8,
                    &imported.project.source,
                    imported.project.source_options.lenient_segments,
                )
                .unwrap();
                assert_eq!(text, source, "complete X12 typed tree: {case}");
                assert_eq!(byte_source, source, "complete X12 UTF-8 typed tree: {case}");
                verify(&imported.project, &text, case);
                verify(&imported.project, &byte_source, case);
            }
        }
    }
}

#[test]
fn optional_global_parser_with_edi_primary_keeps_complete_root_demand_and_values() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture("optional-root-cases.json")).unwrap())
            .unwrap();
    assert_eq!(cases.len(), 14);
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["design"] == "optional-root")
            .count(),
        12
    );
    for design in ["optional-root", "root"] {
        let file = if design == "optional-root" {
            "optional-edi-root.mfd"
        } else {
            "required-edi-root.mfd"
        };
        let imported = mfd::import(&fixture(file)).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(project_oracle::expected(design)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let encoded = mapping::project_file::encode_pretty(&imported.project).unwrap();
        let restored = mapping::project_file::decode_str(&encoded).unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&imported.project).unwrap()
        );
        if design == "optional-root" {
            assert_eq!(
                imported
                    .project
                    .source_options
                    .mfd_optional_parser_input_names,
                std::collections::BTreeMap::from([(3, "payload".into())])
            );
            assert_eq!(
                imported
                    .project
                    .graph
                    .nodes
                    .values()
                    .filter(|node| matches!(node, Node::RuntimeParameterDefault { .. }))
                    .count(),
                1
            );
            for node in imported.project.graph.nodes.values() {
                if let Node::Call { function, args } = node
                    && matches!(
                        function.as_str(),
                        "json_parse_field" | "json_parse_object_exists"
                    )
                {
                    assert_eq!(
                        args[0], 3,
                        "structural and scalar parser consumers share only their parser-local adapter"
                    );
                }
            }
        }
        for case in cases.iter().filter(|case| case["design"] == design) {
            let source = typed(&case["primary_typed_value"], &imported.project.source);
            verify(&imported.project, &source, case);
            if let Some(document) = case.get("input_x12") {
                let document = document.as_str().unwrap();
                for text in [document, std::str::from_utf8(document.as_bytes()).unwrap()] {
                    let input = format_edi::x12::from_str(
                        text,
                        &imported.project.source,
                        imported.project.source_options.lenient_segments,
                    )
                    .unwrap();
                    assert_eq!(input, source, "whole authored X12 tree: {case}");
                    verify(&imported.project, &input, case);
                }
            }
        }
    }
}

#[test]
fn optional_edi_root_absence_metadata_survives_codec_and_refuses_before_export_artifacts() {
    let original = std::fs::read_to_string(fixture("optional-edi-root.mfd")).unwrap();
    for replacement in [
        "",
        "<sources/>",
        "<sources><datapoint pos=\"0\"/></sources>",
        "<sources><datapoint/></sources>",
    ] {
        let text = original.replace(
            "<sources><datapoint pos=\"0\" key=\"400\"/></sources>",
            replacement,
        );
        assert_ne!(text, original);
        let imported = TemporaryFixture::new(&text).import();
        assert!(
            imported.warnings.is_empty(),
            "{replacement}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(project_oracle::expected("optional-root")).unwrap()
        );
    }
    let imported = mfd::import(&fixture("optional-edi-root.mfd")).unwrap();
    let project = mapping::project_file::decode_str(
        &mapping::project_file::encode_pretty(&imported.project).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&project).unwrap(),
        serde_json::to_value(project_oracle::expected("optional-root")).unwrap()
    );
    let temp = TemporaryFixture::new(&original);
    let output = temp.0.join("uncreated/absent.mfd");
    for profile in [mfd::ExportProfile::default(), mfd::ExportProfile::NativeMfd] {
        let error = mfd::export_with_profile(&project, &output, profile).unwrap_err();
        assert!(
            matches!(error, mfd::MfdError::Unsupported(ref reason) if reason == "optional parser input absence provenance cannot be preserved in .mfd export")
        );
        assert!(!output.parent().unwrap().exists());
    }
    assert!(mfd::preflight_export(&project, &output).is_err());
    assert!(!output.parent().unwrap().exists());
}

#[test]
fn absent_optional_root_does_not_bypass_required_target_property_validation() {
    let original = std::fs::read_to_string(fixture("optional-edi-root.mfd")).unwrap();
    let temp = TemporaryFixture::new(&original);
    let path = temp.0.join("root-target.schema.json");
    let mut schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    schema["required"] = serde_json::json!(["Label"]);
    std::fs::write(&path, serde_json::to_vec(&schema).unwrap()).unwrap();
    let imported = temp.import();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut expected = project_oracle::expected("optional-root");
    expected.target = expected
        .target
        .with_required_fields(vec!["Label".into()])
        .unwrap();
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture("optional-root-cases.json")).unwrap())
            .unwrap();
    let case = &cases[0];
    let source = typed(&case["primary_typed_value"], &imported.project.source);
    let before = source.clone();
    let output = engine::run_with_context(
        &imported.project,
        &source,
        &engine::ExecutionContext::new(Path::new("optional-edi-root.mfd")),
    )
    .unwrap();
    assert_eq!(
        output,
        Instance::Group(Vec::<(String, Instance)>::new().into())
    );
    assert!(
        matches!(format_json::to_value(&imported.project.target, &output), Err(format_json::JsonFormatError::MissingRequiredProperty { ref object, ref property }) if object == "Report" && property == "Label")
    );
    assert_eq!(source, before);
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct TemporaryFixture(PathBuf);
impl TemporaryFixture {
    fn new(text: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrule_edi_parser_primary_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        for name in ["payload.schema.json", "root-target.schema.json"] {
            std::fs::copy(fixture(name), root.join(name)).unwrap();
        }
        std::fs::write(root.join("root.mfd"), text).unwrap();
        Self(root)
    }
    fn import(&self) -> mfd::Imported {
        mfd::import(&self.0.join("root.mfd")).unwrap()
    }
}
impl Drop for TemporaryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unsupported_primary_and_raw_parser_controls_preserve_complete_refusals() {
    let original = std::fs::read_to_string(fixture("required-edi-root.mfd")).unwrap();
    let warning = "JSON parser object presence into `` requires one plain object connection and a uniquely declared global String input without a connected default; skipped";
    for (name, text) in [
        ("other root", original.replace("name=\"Envelope\" ferrule-kind=\"group\"", "name=\"Other\" ferrule-kind=\"group\"")),
        ("non-X12", original.replace("kind=\"EDIX12\"", "kind=\"EDIFACT\"")),
        ("invalid optional setting", original.replace("usageKind=\"input\" name=\"payload\"", "usageKind=\"input\" name=\"payload\" optional=\"2\"")),
        ("connected default", original.replace("</vertices>", "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"400\"/></edges></vertex></vertices>")),
        ("shadow", original.replace("</children>", "<component name=\"shadow\" library=\"unknown\" kind=\"31\"><data><root><entry name=\"unlowered\" outkey=\"420\"/></root></data></component></children>")),
    ] {
        assert_ne!(text, original, "{name}");
        let imported = TemporaryFixture::new(&text).import();
        let expected_warning = if name == "invalid optional setting" {
            "JSON string parser `parsed` is unsupported: output port `420` is structural; object presence requires a declared global String input without a connected default"
        } else {
            warning
        };
        assert!(imported.warnings.iter().any(|value| value == expected_warning), "{name}: {:?}", imported.warnings);
        let mut expected = project_oracle::expected("root");
        expected.root = Scope::default();
        expected.graph.nodes.clear();
        let unresolved = "unsupported:required-input:db:substitute-null:2";
        if name == "invalid optional setting" {
            expected.root.bindings.push(mapping::Binding {
                target_field: "Label".into(),
                node: 0,
            });
            expected.graph.nodes = [
                (0, Node::Call { function: unresolved.into(), args: vec![1] }),
                (1, Node::SourceField { path: vec!["NTE".into(), "Text".into()], frame: None }),
            ].into();
        }
        if name == "other root" { expected.source.name = "Other".into(); }
        if name == "non-X12" { expected.source_options.edi_kind = Some(mapping::EdiBoundaryKind::Edifact); }
        assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(expected).unwrap(), "{name}");
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::RuntimeParameter { .. } | Node::RuntimeParameterDefault { .. })));
        if name == "invalid optional setting" {
            let message = format!("unknown function `{unresolved}`");
            let issues = engine::validate(&imported.project);
            assert_eq!(issues.len(), 1, "{issues:?}");
            assert_eq!(issues[0].location, "graph node 0");
            assert_eq!(issues[0].message, message);
            let error = codegen::lower(&imported.project).unwrap_err();
            assert_eq!(error.diagnostics(), &[codegen::Diagnostic::Validation {
                location: "graph node 0".into(),
                message,
            }]);
            let cases: Vec<serde_json::Value> = serde_json::from_slice(
                &std::fs::read(fixture("cases.json")).unwrap(),
            ).unwrap();
            let source = typed(&cases[0]["primary_typed_value"], &imported.project.source);
            let before = source.clone();
            let result = engine::run_with_context(
                &imported.project,
                &source,
                &engine::ExecutionContext::new(Path::new("unresolved-parser.mfd")),
            );
            assert!(matches!(result, Err(engine::EngineError::Function(functions::FunctionError::UnknownFunction(ref function))) if function == unresolved));
            assert_eq!(source, before);
        } else {
            assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::Call { .. })));
        }
    }
}
