use super::*;
use crate::{
    FileCodecError, Graph, NamedSource, NamedTarget, Pipeline, PipelineStage, Scope, pipeline_file,
    project_file,
};
use ir::SchemaNode;

const PROFILE: &str = include_str!("../../../docs/design/fixtures/sqlite-read-v1/profile.json");

fn schema() -> SchemaNode {
    SchemaNode::group(
        "orders",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            SchemaNode::scalar("label", ScalarType::String),
            SchemaNode::scalar("score", ScalarType::Int),
            SchemaNode::scalar("active", ScalarType::Bool),
            SchemaNode::scalar("ratio", ScalarType::Float),
        ],
    )
    .repeating()
}

fn profile() -> SqliteReadProfile {
    serde_json::from_str(PROFILE).unwrap()
}

fn edited(edit: impl FnOnce(&mut serde_json::Value)) -> SqliteReadProfile {
    let mut value: serde_json::Value = serde_json::from_str(PROFILE).unwrap();
    edit(&mut value);
    serde_json::from_value(value).unwrap()
}

fn refusal(profile: &SqliteReadProfile) -> (String, QueryProfileReason) {
    let error = validate_sqlite_read_profile(&schema(), profile).unwrap_err();
    (error.path, error.reason)
}

#[test]
fn authored_profile_round_trips_and_admits_in_first_use_order() {
    let profile = profile();
    let original: serde_json::Value = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(serde_json::to_value(&profile).unwrap(), original);

    let plan = validate_sqlite_read_profile(&schema(), &profile).unwrap();
    assert_eq!(plan.table(), "orders");
    assert_eq!(plan.key(), "id");
    let columns: Vec<_> = plan
        .columns()
        .iter()
        .map(|(name, ty)| (name.as_str(), *ty))
        .collect();
    assert_eq!(
        columns,
        [
            ("id", ScalarType::Int),
            ("label", ScalarType::String),
            ("score", ScalarType::Int),
            ("active", ScalarType::Bool),
            ("ratio", ScalarType::Float),
        ]
    );
    assert_eq!(
        plan.predicates()[0].test,
        ValidatedPredicateTest::Compare {
            op: SqliteReadComparisonOp::Ge,
            parameter: 0
        }
    );
    assert_eq!(plan.parameters()[1].runtime_parameter, "label");
}

#[test]
fn closed_wire_refuses_unknown_members_kinds_operators_and_duplicates() {
    let parse = |text: &str| serde_json::from_str::<SqliteReadProfile>(text).is_err();
    let original: serde_json::Value = serde_json::from_str(PROFILE).unwrap();
    let mutate = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut value = original.clone();
        edit(&mut value);
        serde_json::from_value::<SqliteReadProfile>(value).is_err()
    };
    assert!(mutate(&|value| value["sql"] = "SELECT 1".into()));
    assert!(mutate(&|value| value["kind"] = "sqlite_read_v2".into()));
    assert!(mutate(&|value| value["all"][0]["op"] = "like".into()));
    assert!(mutate(&|value| value["all"][0]["value"] = 1.into()));
    assert!(mutate(
        &|value| value["order_by"][0]["collation"] = "nocase".into()
    ));
    assert!(mutate(&|value| value["parameters"][0]["default"] = 1.into()));
    assert!(mutate(&|value| {
        value.as_object_mut().unwrap().remove("parameters");
    }));
    assert!(mutate(&|value| value["all"][0]["type"] = "decimal".into()));
    assert!(parse(&PROFILE.replacen(
        "\"table\": \"orders\",",
        "\"table\": \"orders\", \"table\": \"other\",",
        1
    )));
    assert!(parse(&PROFILE.replacen(
        "\"kind\": \"sqlite_read_v1\",",
        "\"kind\": \"sqlite_read_v1\", \"kind\": \"sqlite_read_v1\",",
        1
    )));
    // Empty arrays are explicit and remain admissible.
    let empty = edited(|value| {
        value["parameters"] = serde_json::json!([]);
        value["all"] = serde_json::json!([]);
    });
    assert!(validate_sqlite_read_profile(&schema(), &empty).is_ok());
}

#[test]
fn schema_shape_is_validated_before_the_descriptor() {
    let mut nested = schema();
    if let SchemaKind::Group { children, .. } = &mut nested.kind {
        children[1] = SchemaNode::group("label", vec![]);
    }
    let error = validate_sqlite_read_profile(&nested, &edited(|value| value["table"] = "".into()))
        .unwrap_err();
    assert_eq!(error.path, "schema/label");
    assert_eq!(error.reason, QueryProfileReason::SchemaFieldNotPlainScalar);

    let mut flat = schema();
    flat.repeating = false;
    assert_eq!(
        validate_sqlite_read_profile(&flat, &profile())
            .unwrap_err()
            .reason,
        QueryProfileReason::SchemaNotRepeatingGroup
    );
    let mut generated = schema();
    if let SchemaKind::Group { children, .. } = &mut generated.kind {
        children[0].value_generation = Some(ir::ValueGeneration::MaxNumber);
    }
    assert_eq!(
        validate_sqlite_read_profile(&generated, &profile())
            .unwrap_err()
            .path,
        "schema/id"
    );
}

#[test]
fn descriptor_refusals_report_the_first_stored_member() {
    use QueryProfileReason as R;
    let cases: Vec<(SqliteReadProfile, &str, QueryProfileReason)> = vec![
        (
            edited(|v| v["table"] = "".into()),
            "/table",
            R::EmptyIdentifier,
        ),
        (
            edited(|v| v["key"] = "k\u{0}".into()),
            "/key",
            R::IdentifierContainsNul,
        ),
        (
            edited(|v| v["table"] = "t".repeat(257).into()),
            "/table",
            R::IdentifierTooLong { limit: 256 },
        ),
        (
            edited(|v| v["projection"][1]["field"] = "score".into()),
            "/projection/1/field",
            R::ProjectionFieldOrder {
                expected: "label".into(),
            },
        ),
        (
            edited(|v| v["projection"][2]["column"] = "LABEL".into()),
            "/projection/2/column",
            R::DuplicateColumn,
        ),
        (
            edited(|v| {
                v["projection"].as_array_mut().unwrap().pop();
            }),
            "/projection",
            R::ProjectionCount { expected: 5 },
        ),
        (
            edited(|v| v["parameters"][0]["name"] = "1st".into()),
            "/parameters/0/name",
            R::InvalidParameterName,
        ),
        (
            edited(|v| v["parameters"][1]["name"] = "minimum".into()),
            "/parameters/1/name",
            R::DuplicateParameter,
        ),
        (
            edited(|v| v["all"][0]["parameter"] = "missing".into()),
            "/all/0/parameter",
            R::UnknownParameter,
        ),
        (
            edited(|v| v["all"][0]["type"] = "float".into()),
            "/all/0/parameter",
            R::ParameterTypeMismatch {
                parameter: ScalarType::Int,
            },
        ),
        (
            edited(|v| {
                v["parameters"][0]["type"] = "float".into();
                v["all"][0]["type"] = "float".into();
            }),
            "/all/0/column",
            R::ColumnTypeConflict {
                first: ScalarType::Int,
                found: ScalarType::Float,
            },
        ),
        (
            edited(|v| {
                v["all"].as_array_mut().unwrap().pop();
            }),
            "/parameters/1",
            R::UnreferencedParameter,
        ),
        (
            edited(|v| {
                v["all"][1] = serde_json::json!({"op":"ge","column":"score","type":"int","parameter":"minimum"});
            }),
            "/all/1",
            R::DuplicatePredicate,
        ),
        (
            edited(|v| {
                v["parameters"][1]["type"] = "bool".into();
                v["all"][1] = serde_json::json!({"op":"lt","column":"active","type":"bool","parameter":"label"});
            }),
            "/all/1",
            R::BooleanOrdering,
        ),
        (
            edited(|v| v["order_by"] = serde_json::json!([])),
            "/order_by",
            R::EmptyOrder,
        ),
        (
            edited(|v| {
                v["order_by"].as_array_mut().unwrap().swap(0, 1);
            }),
            "/order_by/0/column",
            R::KeyTermNotLast,
        ),
        (
            edited(|v| v["order_by"][1]["direction"] = "desc".into()),
            "/order_by/1",
            R::KeyTermShape,
        ),
        (
            edited(|v| v["order_by"][1]["nulls"] = "first".into()),
            "/order_by/1",
            R::KeyTermShape,
        ),
        (
            edited(|v| v["order_by"][0]["column"] = "ID".into()),
            "/order_by/0/column",
            R::KeyTermNotLast,
        ),
    ];
    for (profile, path, reason) in cases {
        assert_eq!(refusal(&profile), (path.to_owned(), reason));
    }
}

#[test]
fn limits_refuse_before_their_entries_are_visited() {
    let many = edited(|v| {
        v["all"] = serde_json::Value::Array(
            (0..65)
                .map(|_| serde_json::json!({"op":"is_null","column":"label","type":"string"}))
                .collect(),
        );
    });
    assert_eq!(
        refusal(&many),
        ("/all".into(), QueryProfileReason::TooMany { limit: 64 })
    );
    let parameters = edited(|v| {
        v["parameters"] = serde_json::Value::Array(
            (0..33)
                .map(|i| serde_json::json!({"name":format!("p{i}"),"type":"int","runtime_parameter":"x"}))
                .collect(),
        );
    });
    assert_eq!(
        refusal(&parameters),
        (
            "/parameters".into(),
            QueryProfileReason::TooMany { limit: 32 }
        )
    );
}

fn query_project() -> Project {
    Project {
        source: schema(),
        target: SchemaNode::group("Target", vec![]),
        source_path: Some("orders.db".into()),
        target_path: None,
        source_options: FormatOptions {
            sqlite_read: Some(profile()),
            ..Default::default()
        },
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

#[test]
fn saved_profiles_require_a_capability_envelope_that_older_readers_refuse() {
    let project = query_project();
    let text = project_file::encode_pretty(&project).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["__ferrule_file"]["version"], 3);
    assert_eq!(
        saved["__ferrule_file"]["capabilities"],
        serde_json::json!(["sqlite_read_v1"])
    );
    let reopened = project_file::decode_str(&text).unwrap();
    assert_eq!(reopened.source_options, project.source_options);

    // A reader that predates the envelope misses the project members.
    assert!(serde_json::from_str::<Project>(&text).is_err());
    // A version-2 reader refuses both the version and the unknown header member.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct VersionTwoHeader {
        kind: String,
        version: u64,
        float_bits: serde_json::Value,
    }
    assert!(serde_json::from_value::<VersionTwoHeader>(saved["__ferrule_file"].clone()).is_err());
    assert_ne!(saved["__ferrule_file"]["version"], 2);

    // Current readers refuse the profile outside a matching envelope.
    let unmarked = serde_json::to_string(&project).unwrap();
    assert!(matches!(
        project_file::decode_str(&unmarked),
        Err(FileCodecError::CapabilityRequiresEnvelope {
            capability: "sqlite_read_v1"
        })
    ));
    let mut plain = saved.clone();
    plain["__ferrule_file"]["version"] = 2.into();
    plain["__ferrule_file"]
        .as_object_mut()
        .unwrap()
        .remove("capabilities");
    assert!(matches!(
        project_file::decode_str(&plain.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
    let mut unknown = saved.clone();
    unknown["__ferrule_file"]["capabilities"] = serde_json::json!(["sqlite_write_v1"]);
    assert!(matches!(
        project_file::decode_str(&unknown.to_string()),
        Err(FileCodecError::UnknownCapability { .. })
    ));
    let mut stale = saved;
    stale["document"]["source_options"]
        .as_object_mut()
        .unwrap()
        .remove("sqlite_read");
    assert!(matches!(
        project_file::decode_str(&stale.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
}

#[test]
fn absent_profiles_keep_the_ordinary_wire_and_any_placement_is_gated() {
    let mut project = query_project();
    project.source_options = Default::default();
    let text = project_file::encode_pretty(&project).unwrap();
    assert!(!text.contains("__ferrule_file"));
    assert!(!text.contains("sqlite_read"));

    project.extra_targets.push(NamedTarget {
        name: "copy".into(),
        path: None,
        schema: schema(),
        options: FormatOptions {
            sqlite_read: Some(profile()),
            ..Default::default()
        },
        root: Scope::default(),
    });
    assert!(
        project_file::encode_pretty(&project)
            .unwrap()
            .contains("\"capabilities\"")
    );
    project.extra_targets.clear();
    project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "lookup.db".into(),
        schema: schema(),
        options: FormatOptions {
            sqlite_read: Some(profile()),
            ..Default::default()
        },
        dynamic_path: None,
    });
    let inputs = project.sqlite_read_inputs();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].owner, SqliteReadOwner::Named("lookup"));
}

#[test]
fn pipelines_gate_every_embedded_query_project() {
    let stage = PipelineStage {
        id: "read".into(),
        mapping_path: None,
        project: query_project(),
        source: crate::PipelineInput::Host {
            name: "orders".into(),
        },
        extra_sources: vec![],
    };
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![stage],
    };
    let text = pipeline_file::encode_pretty(&pipeline).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["__ferrule_file"]["version"], 3);
    assert!(pipeline_file::decode_str(&text).is_ok());
    assert!(matches!(
        pipeline_file::decode_str(&serde_json::to_string(&pipeline).unwrap()),
        Err(FileCodecError::CapabilityRequiresEnvelope { .. })
    ));
}
