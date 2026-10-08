use super::*;
use crate::{
    Graph, Node, Pipeline, PipelineInput, PipelineStage, Project, Scope, pipeline_file,
    project_file,
};
use ir::{FiniteF64, JsonAllowedValue, JsonAllowedValues, ScalarType, SchemaKind, SchemaNode};
use serde::{Deserialize, Serialize};

const PHYSICAL: &str = r#"{
  "source":{"name":"Source","kind":{"kind":"group","children":[]}},
  "target":{"name":"Target","kind":{"kind":"group","children":[{"name":"Amount","kind":{"kind":"scalar","ty":"float"},"numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}}}]}},
  "source_options":{"pdf":{"root_name":"PDF","page_selection":{"kind":"first"},"commands":[{"kind":"capture","name":"text","region":{"left":{"reference":{"kind":"left"},"offset":1e-307},"top":{"reference":{"kind":"top"},"offset":0.0},"right":{"reference":{"kind":"right"},"offset":0.0},"bottom":{"reference":{"kind":"bottom"},"offset":-0.0}}}]}},
  "graph":{"nodes":{"0":{"kind":"const","value":1e-307},"1":{"kind":"value_map","input":0,"table":[[1e-307,1e-307],[1.0000000000000001e-307,1.5]],"default":1e-307},"2":{"kind":"const","value":"FERRULE-F64-BITS:0000000000000000"},"3":{"kind":"const","value":1.0},"4":{"kind":"const","value":-0.0}}},
  "user_functions":{"1":{"library":"L","name":"High","parameters":[],"output_name":"Value","output_type":"float","body":{"nodes":{"0":{"kind":"const","value":1e-307}}},"output":0}},
  "root":{"bindings":[{"target_field":"Amount","node":0}],"children":[]}
}"#;
fn project() -> Project {
    serde_json::from_str(PHYSICAL).unwrap()
}
fn plain_project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Target", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}
fn snapshot<T: Serialize>(model: &T) -> Value {
    tree::to_value(model).unwrap()
}
// Explicit version-2 fixture construction is independent of the encoder's
// ordinary-versus-envelope selection. The document and table retain every
// typed float tag and bit; malformed tests then edit this valid envelope.
fn versioned_fixture<T: Serialize>(model: &T, kind: &str) -> Value {
    let document = snapshot(model);
    let float_bits: serde_json::Map<String, Value> = collect_bits(&document)
        .unwrap()
        .into_iter()
        .map(|(path, bits)| (path, Value::String(format!("{bits:016x}"))))
        .collect();
    serde_json::json!({
        "__ferrule_file": { "kind": kind, "version": 2, "float_bits": float_bits },
        "document": document,
    })
}
fn versioned_text<T: Serialize>(model: &T, kind: &str) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&versioned_fixture(model, kind)).unwrap()
    )
}
fn envelope() -> Value {
    let original = project();
    let value = versioned_fixture(&original, "project");
    let result = project_file::decode_str(&value.to_string());
    println!("ORIGINAL_VALID_VERSIONED_FIXTURE_RESULT {result:?}");
    let decoded = result.unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&decoded)).unwrap());
    assert_eq!(
        collect_bits(&snapshot(&original)).unwrap(),
        collect_bits(&snapshot(&decoded)).unwrap()
    );
    value
}
fn assert_invalid(value: &Value) {
    assert!(project_file::decode_str(&serde_json::to_string(value).unwrap()).is_err());
}

#[test]
fn physical_project_preserves_schema_graph_value_map_function_and_pdf_floats() {
    let mut original = project();
    let high = serde_json::from_str::<f64>("1e-307").unwrap();
    let low = serde_json::from_str::<f64>("1.0000000000000001e-307").unwrap();
    assert_eq!(high.to_bits(), 0x0031_fa18_2c40_c60d);
    assert_eq!(low.to_bits(), 0x0031_fa18_2c40_c60e);
    assert_ne!(high.to_bits(), low.to_bits());
    let mut enumerated = SchemaNode::scalar("Enum", ScalarType::Float);
    enumerated.json_allowed_values = Some(
        JsonAllowedValues::new([
            JsonAllowedValue::Float(FiniteF64::new(low).unwrap()),
            JsonAllowedValue::Float(FiniteF64::new(high).unwrap()),
        ])
        .unwrap(),
    );
    if let SchemaKind::Group { children, .. } = &mut original.target.kind {
        children.push(enumerated);
    }
    let legacy = serde_json::to_string_pretty(&original).unwrap();
    let legacy_result = serde_json::from_str::<Project>(&legacy);
    println!("ORIGINAL_ORDINARY_PHYSICAL_PROJECT_RESULT {legacy_result:?}");
    let legacy_decoded = legacy_result.unwrap();
    assert!(same_model(&snapshot(&legacy_decoded), &snapshot(&original)).unwrap());
    assert_eq!(
        collect_bits(&snapshot(&legacy_decoded)).unwrap(),
        collect_bits(&snapshot(&original)).unwrap()
    );
    let ordinary_encoded = project_file::encode_pretty(&original).unwrap();
    assert_eq!(ordinary_encoded, format!("{legacy}\n"));
    assert!(!ordinary_encoded.contains("\"__ferrule_file\""));
    let ordinary_result = project_file::decode_bytes(ordinary_encoded.as_bytes());
    println!("ORIGINAL_ORDINARY_PHYSICAL_FILE_RESULT {ordinary_result:?}");
    let ordinary_decoded = ordinary_result.unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&ordinary_decoded)).unwrap());
    assert_eq!(
        collect_bits(&snapshot(&ordinary_decoded)).unwrap(),
        collect_bits(&snapshot(&original)).unwrap()
    );
    assert_eq!(
        project_file::encode_pretty(&ordinary_decoded).unwrap(),
        ordinary_encoded
    );
    let encoded = versioned_text(&original, "project");
    assert!(encoded.ends_with('\n'));
    let root: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(root["__ferrule_file"]["kind"], "project");
    assert_eq!(root["__ferrule_file"]["version"], 2);
    let result = project_file::decode_bytes(encoded.as_bytes());
    println!("ORIGINAL_VERSIONED_PHYSICAL_FILE_RESULT {result:?}");
    let decoded = result.unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&decoded)).unwrap());
    assert_eq!(
        collect_bits(&snapshot(&decoded)).unwrap(),
        collect_bits(&snapshot(&original)).unwrap()
    );
    assert_eq!(
        project_file::encode_pretty(&decoded).unwrap(),
        ordinary_encoded
    );
    assert_eq!(versioned_text(&decoded, "project"), encoded);
    assert_eq!(
        root["__ferrule_file"]["float_bits"]["/graph/nodes/0/value"],
        "0031fa182c40c60d"
    );
    assert!(
        serde_json::from_str::<Project>(&encoded).is_err(),
        "legacy reader must reject the envelope"
    );
    assert_eq!(
        root["document"]["graph"]["nodes"]["2"]["value"],
        "FERRULE-F64-BITS:0000000000000000"
    );
    assert!(
        root["__ferrule_file"]["float_bits"]
            .get("/graph/nodes/3/value")
            .is_some(),
        "v2 includes stable float tags too"
    );
}

#[test]
fn nested_schema_metadata_and_embedded_boundaries_preserve_bits() {
    use ir::{
        GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
        ItemCountRange, JsonContainsConstraint, JsonContainsConstraints,
        JsonDependentSchemaConstraint, JsonDependentSchemaConstraints, JsonSchemaPredicate,
    };
    let high = FiniteF64::new(serde_json::from_str("1e-307").unwrap()).unwrap();
    let leaf = project().target.child("Amount").unwrap().clone();
    let mut alternatives = SchemaNode::group("Choice", vec![leaf.clone()]);
    if let SchemaKind::Group {
        alternatives: choices,
        ..
    } = &mut alternatives.kind
    {
        choices.push(GroupAlternative {
            name: "High".into(),
            members: vec!["Amount".into()],
            required: vec!["Amount".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "Amount".into(),
                value: GroupAlternativeConstraintValue::Float(high),
            }],
        });
    }
    let mut repeated = SchemaNode::scalar("Values", ScalarType::Float).repeating();
    repeated.json_contains = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonSchemaPredicate::schema(leaf.clone()),
        ItemCountRange::new(1, None).unwrap(),
    )]);
    let mut nested = SchemaNode::group("Nested", vec![alternatives, repeated]);
    nested.json_dependent_schemas =
        JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
            "Choice",
            JsonSchemaPredicate::schema(SchemaNode::group("Predicate", vec![leaf.clone()])),
        )]);
    if let SchemaKind::Group { dynamic, .. } = &mut nested.kind {
        *dynamic = Some(Box::new(leaf));
    }
    let mut original = plain_project();
    original.source = nested.clone();
    original.target = nested.clone();
    original.extra_sources.push(crate::NamedSource {
        name: "named".into(),
        path: "source.json".into(),
        schema: nested.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    original.extra_targets.push(crate::NamedTarget {
        name: "named".into(),
        path: None,
        schema: nested.clone(),
        options: Default::default(),
        root: Scope::default(),
    });
    original.graph.nodes.insert(
        0,
        Node::XmlSerialize {
            path: vec![],
            frame: None,
            schema: Box::new(nested.clone()),
            declaration: false,
            indent: true,
            namespace: None,
        },
    );
    original.source_options.external_source = Some(
        crate::ExternalSourceOptions::http_post(
            crate::ExternalHttpMode::Manual,
            Default::default(),
            Some(crate::ExternalPayloadFormat::Json),
            Some(nested),
            crate::ExternalPayloadFormat::Json,
            vec![],
        )
        .unwrap(),
    );
    let encoded = project_file::encode_pretty(&original).unwrap();
    let decoded = project_file::decode_str(&encoded).unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&decoded)).unwrap());
    let bits = collect_bits(&snapshot(&decoded)).unwrap();
    for prefix in [
        "/source/json_dependent_schemas/",
        "/source/kind/children/0/kind/alternatives/",
        "/source/kind/children/1/json_contains/",
        "/source/kind/dynamic/",
        "/extra_sources/0/schema/",
        "/extra_targets/0/schema/",
        "/graph/nodes/0/schema/",
        "/source_options/external_source/origin/request_schema/",
    ] {
        assert!(
            bits.keys().any(|path| path.starts_with(prefix)),
            "missing {prefix}"
        );
    }
}

#[test]
fn stable_files_keep_ordinary_json_and_signed_zero_tags() {
    let mut original = plain_project();
    for (id, value) in [
        ir::Value::Float(1.0),
        ir::Value::Float(-0.0),
        ir::Value::Int(1),
        ir::Value::Null,
        ir::Value::JsonNull(ir::JsonNull),
        ir::Value::XmlNil(ir::XmlNil),
    ]
    .into_iter()
    .enumerate()
    {
        original
            .graph
            .nodes
            .insert(id as u32, Node::Const { value });
    }
    let expected = format!("{}\n", serde_json::to_string_pretty(&original).unwrap());
    let encoded = project_file::encode_pretty(&original).unwrap();
    assert_eq!(encoded, expected);
    assert!(!encoded.contains("__ferrule_file"));
    let decoded = project_file::decode_str(&encoded).unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&decoded)).unwrap());
}

#[test]
fn pipeline_embedded_projects_keep_all_float_bits() {
    let original = Pipeline {
        main_mapping_path: Some("main.mfd".into()),
        stages: vec![
            PipelineStage {
                id: "first".into(),
                mapping_path: None,
                project: project(),
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: vec![],
            },
            PipelineStage {
                id: "last".into(),
                mapping_path: None,
                project: project(),
                source: PipelineInput::StageTarget {
                    stage: "first".into(),
                    target: None,
                },
                extra_sources: vec![],
            },
        ],
    };
    let ordinary_encoded = pipeline_file::encode_pretty(&original).unwrap();
    assert_eq!(
        ordinary_encoded,
        format!("{}\n", serde_json::to_string_pretty(&original).unwrap())
    );
    assert!(!ordinary_encoded.contains("\"__ferrule_file\""));
    let ordinary_result = pipeline_file::decode_bytes(ordinary_encoded.as_bytes());
    println!("ORIGINAL_ORDINARY_PIPELINE_FILE_RESULT {ordinary_result:?}");
    let ordinary_decoded = ordinary_result.unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&ordinary_decoded)).unwrap());
    assert_eq!(
        collect_bits(&snapshot(&ordinary_decoded)).unwrap(),
        collect_bits(&snapshot(&original)).unwrap()
    );
    assert_eq!(
        pipeline_file::encode_pretty(&ordinary_decoded).unwrap(),
        ordinary_encoded
    );
    let encoded = versioned_text(&original, "pipeline");
    let root: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(root["__ferrule_file"]["kind"], "pipeline");
    assert!(
        root["__ferrule_file"]["float_bits"]
            .get("/stages/1/project/graph/nodes/0/value")
            .is_some()
    );
    let result = pipeline_file::decode_bytes(encoded.as_bytes());
    println!("ORIGINAL_VERSIONED_PIPELINE_FILE_RESULT {result:?}");
    let decoded = result.unwrap();
    assert!(same_model(&snapshot(&original), &snapshot(&decoded)).unwrap());
    assert_eq!(
        collect_bits(&snapshot(&decoded)).unwrap(),
        collect_bits(&snapshot(&original)).unwrap()
    );
    assert_eq!(
        root["__ferrule_file"]["float_bits"]["/stages/1/project/graph/nodes/0/value"],
        "0031fa182c40c60d"
    );
    assert_eq!(
        pipeline_file::encode_pretty(&decoded).unwrap(),
        ordinary_encoded
    );
    assert_eq!(versioned_text(&decoded, "pipeline"), encoded);
    assert!(serde_json::from_str::<Pipeline>(&encoded).is_err());
    assert!(matches!(
        project_file::decode_str(&encoded),
        Err(FileCodecError::WrongKind { .. })
    ));
}

#[test]
fn legacy_parser_retains_ignored_any_and_duplicate_field_behavior() {
    let ordinary = serde_json::to_string(&plain_project()).unwrap();
    for ignored in [
        "1e400".into(),
        r#""\ud800""#.into(),
        format!("{}0{}", "[".repeat(140), "]".repeat(140)),
    ] {
        let text = format!("{{\"ignored\":{ignored},{}", &ordinary[1..]);
        let legacy: Project = serde_json::from_str(&text).unwrap();
        let decoded = project_file::decode_str(&text).unwrap();
        assert!(same_model(&snapshot(&legacy), &snapshot(&decoded)).unwrap());
    }
    let duplicated = format!(
        "{{\"source\":{},{}",
        serde_json::to_string(&plain_project().source).unwrap(),
        &ordinary[1..]
    );
    let legacy = serde_json::from_str::<Project>(&duplicated)
        .unwrap_err()
        .to_string();
    let Err(FileCodecError::Deserialization(decoded)) = project_file::decode_str(&duplicated)
    else {
        panic!("legacy error category")
    };
    assert_eq!(decoded.to_string(), legacy);
}

#[test]
fn sidecar_rejects_stale_payload_and_wrong_numeric_targets() {
    let mut value = envelope();
    value["document"]["graph"]["nodes"]["0"]["value"] = serde_json::json!(3.5);
    assert!(matches!(
        project_file::decode_str(&value.to_string()),
        Err(FileCodecError::InvalidFloatMetadata { .. })
    ));
    let mut value = envelope();
    value["__ferrule_file"]["float_bits"]["/root/bindings/0/node"] =
        Value::String("0000000000000000".into());
    assert_invalid(&value);
    let mut value = envelope();
    value["__ferrule_file"]["float_bits"]
        .as_object_mut()
        .unwrap()
        .remove("/graph/nodes/0/value");
    assert_invalid(&value);
    let mut value = envelope();
    value["__ferrule_file"]["float_bits"]["/ignored"] = Value::String("0000000000000000".into());
    value["document"]["ignored"] = serde_json::json!(0.0);
    assert_invalid(&value);
}

#[test]
fn versioned_metadata_is_strict_and_rejects_truncated_nonfinite_and_invalid_paths() {
    for hex in [
        "0031fa182c40c60",
        "0031fa182c40c60E",
        "zz31fa182c40c60e",
        "7ff0000000000000",
        "7ff8000000000000",
    ] {
        let mut value = envelope();
        value["__ferrule_file"]["float_bits"]["/graph/nodes/0/value"] = Value::String(hex.into());
        assert_invalid(&value);
    }
    for path in [
        "",
        "graph/nodes/0/value",
        "/graph/~2/value",
        "/graph/nodes/01/value",
        "/absent",
        "/graph/nodes/2/value",
    ] {
        let mut value = envelope();
        value["__ferrule_file"]["float_bits"][path] = Value::String("0031fa182c40c60e".into());
        assert_invalid(&value);
    }
    let mut value = envelope();
    value["__ferrule_file"]["version"] = serde_json::json!(3);
    assert!(matches!(
        project_file::decode_str(&value.to_string()),
        Err(FileCodecError::UnsupportedVersion { version: 3 })
    ));
    let mut value = envelope();
    value["__ferrule_file"]["ignored"] = Value::Bool(true);
    assert_invalid(&value);
    let mut value = envelope();
    value["ignored"] = Value::Bool(true);
    assert_invalid(&value);
    assert!(matches!(
        project_file::decode_bytes(&[0xff]),
        Err(FileCodecError::InvalidUtf8(_))
    ));
    let text = versioned_text(&project(), "project");
    assert!(project_file::decode_str(&text[..text.len() / 2]).is_err());
    let ordinary = project_file::encode_pretty(&project()).unwrap();
    assert!(project_file::decode_str(&ordinary[..ordinary.len() / 2]).is_err());
}

#[test]
fn versioned_duplicate_members_are_rejected_before_collapse() {
    let text = versioned_text(&project(), "project");
    for (needle, duplicate) in [
        ("\"version\": 2", "\"version\": 2, \"version\": 2"),
        (
            "\"kind\": \"project\"",
            "\"kind\": \"project\", \"kind\": \"project\"",
        ),
        (
            "\"/graph/nodes/0/value\": \"0031fa182c40c60d\"",
            "\"/graph/nodes/0/value\": \"0031fa182c40c60d\", \"/graph/nodes/0/value\": \"0031fa182c40c60d\"",
        ),
    ] {
        assert!(text.contains(needle));
        assert!(project_file::decode_str(&text.replacen(needle, duplicate, 1)).is_err());
    }
    let duplicate_document = text.replace("\"value\": 1e-307", "\"value\": 1e-307, \"value\": 1.0");
    assert_ne!(duplicate_document, text);
    assert!(project_file::decode_str(&duplicate_document).is_err());
    let duplicate_root = format!("{{\"document\": {{}},{}", &text[1..]);
    assert!(project_file::decode_str(&duplicate_root).is_err());
}

#[test]
fn insignificant_versioned_formatting_and_key_order_are_accepted() {
    let pretty = versioned_text(&project(), "project");
    let mut quoted = false;
    let mut escaped = false;
    let compact: String = pretty
        .chars()
        .filter(|&ch| {
            if quoted {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quoted = false;
                }
                true
            } else if ch == '"' {
                quoted = true;
                true
            } else {
                !ch.is_whitespace()
            }
        })
        .collect();
    let decoded = project_file::decode_str(&compact).unwrap();
    assert!(same_model(&snapshot(&decoded), &snapshot(&project())).unwrap());
    let split = compact.find(",\"document\":").unwrap();
    let header = &compact["{\"__ferrule_file\":".len()..split];
    let document = &compact[split + ",\"document\":".len()..compact.len() - 1];
    let reordered = format!("{{\"document\":{document},\"__ferrule_file\":{header}}}");
    assert!(project_file::decode_str(&reordered).is_ok());
}

#[test]
fn nonfinite_graph_locations_reject_before_null_serialization() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut original = plain_project();
        original.graph.nodes.insert(
            0,
            Node::Const {
                value: ir::Value::Float(value),
            },
        );
        assert!(
            matches!(project_file::encode_pretty(&original), Err(FileCodecError::NonFiniteValue { path }) if path == "/graph/nodes/0/value")
        );
        for (table, default) in [
            (
                vec![(ir::Value::Float(value), ir::Value::Int(1))],
                ir::Value::Null,
            ),
            (
                vec![(ir::Value::Int(1), ir::Value::Float(value))],
                ir::Value::Null,
            ),
            (vec![], ir::Value::Float(value)),
        ] {
            original.graph.nodes.insert(
                0,
                Node::ValueMap {
                    input: 1,
                    input_type: None,
                    table,
                    default: Some(default),
                },
            );
            assert!(matches!(
                project_file::encode_pretty(&original),
                Err(FileCodecError::NonFiniteValue { .. })
            ));
        }
        let mut original = project();
        original
            .user_functions
            .values_mut()
            .next()
            .unwrap()
            .body
            .nodes
            .insert(
                0,
                Node::Const {
                    value: ir::Value::Float(value),
                },
            );
        assert!(
            matches!(project_file::encode_pretty(&original), Err(FileCodecError::NonFiniteValue { path }) if path.starts_with("/user_functions/1/body/nodes/0"))
        );
        assert!(
            matches!(tree::to_value(&crate::PdfMetricMatch { value, deviation: 0.0 }), Err(FileCodecError::NonFiniteValue { path }) if path == "/value")
        );
    }
}

#[derive(Serialize, Deserialize)]
struct PointerFixture {
    #[serde(rename = "a/b~c")]
    floats: Vec<f64>,
    text: String,
}
#[test]
fn escaped_pointer_and_signed_zero_are_unambiguous() {
    let original = PointerFixture {
        floats: vec![serde_json::from_str("1e-307").unwrap(), -0.0, 1.0],
        text: "__ferrule_file FERRULE-F64-BITS:deadbeef".into(),
    };
    let ordinary_encoded = encode_pretty(&original, "project").unwrap();
    assert_eq!(
        ordinary_encoded,
        format!("{}\n", serde_json::to_string_pretty(&original).unwrap())
    );
    assert!(!ordinary_encoded.contains("\"__ferrule_file\":"));
    let ordinary_result = decode_str::<PointerFixture>(&ordinary_encoded, "project");
    println!(
        "ORIGINAL_ORDINARY_POINTER_RESULT {:?}",
        ordinary_result
            .as_ref()
            .map(|value| (&value.floats, &value.text))
    );
    let ordinary_decoded = ordinary_result.unwrap();
    assert_eq!(
        ordinary_decoded
            .floats
            .iter()
            .map(|f| f.to_bits())
            .collect::<Vec<_>>(),
        vec![
            0x0031_fa18_2c40_c60d,
            0x8000_0000_0000_0000,
            0x3ff0_0000_0000_0000
        ]
    );
    assert_eq!(ordinary_decoded.text, original.text);
    let encoded = versioned_text(&original, "project");
    assert!(encoded.contains("/a~1b~0c/0"));
    let result = decode_str::<PointerFixture>(&encoded, "project");
    println!(
        "ORIGINAL_VERSIONED_POINTER_RESULT {:?}",
        result.as_ref().map(|value| (&value.floats, &value.text))
    );
    let decoded = result.unwrap();
    assert_eq!(
        decoded
            .floats
            .iter()
            .map(|f| f.to_bits())
            .collect::<Vec<_>>(),
        original
            .floats
            .iter()
            .map(|f| f.to_bits())
            .collect::<Vec<_>>()
    );
    assert_eq!(decoded.text, original.text);
    let mut root: Value = serde_json::from_str(&encoded).unwrap();
    root["document"]["a/b~c"][1] = serde_json::json!(0.0);
    assert!(matches!(
        decode_str::<PointerFixture>(&root.to_string(), "project"),
        Err(FileCodecError::InvalidFloatMetadata { .. })
    ));
}

#[test]
fn constructed_depth_and_envelope_extra_depth_are_bounded() {
    let mut value = serde_json::json!(serde_json::from_str::<f64>("1e-307").unwrap());
    for _ in 0..128 {
        value = Value::Array(vec![value]);
    }
    assert!(matches!(
        encode_pretty(&value, "project"),
        Err(FileCodecError::DepthLimit { .. })
    ));
    let mut depth126 = serde_json::json!(serde_json::from_str::<f64>("1e-307").unwrap());
    for _ in 0..126 {
        depth126 = Value::Array(vec![depth126]);
    }
    let ordinary126 = encode_pretty(&depth126, "project").unwrap();
    assert!(
        same_model(
            &decode_str::<Value>(&ordinary126, "project").unwrap(),
            &depth126
        )
        .unwrap()
    );
    let versioned126 = versioned_text(&depth126, "project");
    let decoded126 = decode_str::<Value>(&versioned126, "project");
    println!("ORIGINAL_VERSIONED_DEPTH126_RESULT {decoded126:?}");
    assert!(same_model(&decoded126.unwrap(), &depth126).unwrap());
    let mut depth127 = serde_json::json!(serde_json::from_str::<f64>("1e-307").unwrap());
    for _ in 0..127 {
        depth127 = Value::Array(vec![depth127]);
    }
    // A root scalar/array is not a Project. The private codec demonstrates
    // ordinary depth 127 and the real versioned decoder's extra root cost.
    let ordinary127 = encode_pretty(&depth127, "project").unwrap();
    let decoded127 = decode_str::<Value>(&ordinary127, "project");
    println!("ORIGINAL_ORDINARY_DEPTH127_RESULT {decoded127:?}");
    assert!(same_model(&decoded127.unwrap(), &depth127).unwrap());
    assert!(matches!(
        check_depth(&depth127, 1),
        Err(FileCodecError::DepthLimit {
            depth: 128,
            max: 127
        })
    ));
    let versioned127 = versioned_text(&depth127, "project");
    let decoded_versioned127 = decode_str::<Value>(&versioned127, "project");
    println!("ORIGINAL_VERSIONED_DEPTH127_RESULT {decoded_versioned127:?}");
    assert!(matches!(
        decoded_versioned127,
        Err(FileCodecError::InvalidEnvelope { .. })
    ));
}

#[test]
fn byte_limit_rejects_new_and_versioned_files_but_not_legacy_unknown_padding() {
    let value = "x".repeat(MAX_DOCUMENT_BYTES + 1);
    assert!(matches!(
        encode_pretty(&value, "project"),
        Err(FileCodecError::TooLarge { .. })
    ));
    let text = format!(
        "{{\"__ferrule_file\":{{\"kind\":\"project\",\"version\":2,\"float_bits\":{{}}}},\"document\":{{}},\"padding\":\"{value}\"}}"
    );
    assert!(matches!(
        project_file::decode_str(&text),
        Err(FileCodecError::TooLarge { .. })
    ));
    let ordinary = serde_json::to_string(&plain_project()).unwrap();
    let legacy = format!("{{\"padding\":\"{value}\",{}", &ordinary[1..]);
    assert!(project_file::decode_str(&legacy).is_ok());
}

#[test]
fn deterministic_finite_binary64_graph_sweep_preserves_every_tag_and_bit() {
    let mut bits = vec![
        0_u64,
        (-0_f64).to_bits(),
        1,
        0x8000_0000_0000_0001,
        f64::MIN_POSITIVE.to_bits() - 1,
        f64::MIN_POSITIVE.to_bits(),
        f64::MIN_POSITIVE.to_bits() + 1,
        1_f64.to_bits() - 1,
        1_f64.to_bits(),
        1_f64.to_bits() + 1,
        (-1_f64).to_bits() - 1,
        (-1_f64).to_bits(),
        (-1_f64).to_bits() + 1,
        f64::MAX.to_bits() - 1,
        f64::MAX.to_bits(),
        (-f64::MAX).to_bits() - 1,
        (-f64::MAX).to_bits(),
        0x0031_fa18_2c40_c60d,
        0x0031_fa18_2c40_c60e,
        0x3fef_ffff_ffff_fc19,
        0x3fef_ffff_ffff_fc1a,
    ];
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    while bits.len() < 4096 + 21 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        if f64::from_bits(state).is_finite() {
            bits.push(state);
        }
    }
    let mut original = plain_project();
    for (index, bits) in bits.iter().enumerate() {
        original.graph.nodes.insert(
            index as u32,
            Node::Const {
                value: ir::Value::Float(f64::from_bits(*bits)),
            },
        );
    }
    let ordinary = project_file::encode_pretty(&original).unwrap();
    assert_eq!(
        ordinary,
        format!("{}\n", serde_json::to_string_pretty(&original).unwrap())
    );
    assert!(!ordinary.contains("\"__ferrule_file\""));
    let versioned = versioned_text(&original, "project");
    assert!(versioned.contains("\"__ferrule_file\""));
    for (route, encoded) in [("ordinary", &ordinary), ("versioned", &versioned)] {
        let result = project_file::decode_str(encoded);
        println!("ORIGINAL_SWEEP_{route}_RESULT {result:?}");
        let decoded = result.unwrap();
        assert_eq!(decoded.graph.nodes.len(), bits.len());
        for (index, expected) in bits.iter().enumerate() {
            let Node::Const {
                value: ir::Value::Float(actual),
            } = decoded.graph.nodes[&(index as u32)]
            else {
                panic!("node {index} lost Float tag on {route}");
            };
            assert_eq!(actual.to_bits(), *expected, "node {index} on {route}");
        }
        assert_eq!(project_file::encode_pretty(&decoded).unwrap(), ordinary);
        assert_eq!(versioned_text(&decoded, "project"), versioned);
    }
}
