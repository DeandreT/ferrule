// Shared emitter regressions: each backend supplies `emit_schema_fixture`.
use codegen::{
    ArtifactSet, DynamicSourceProgram, EmbeddedSchemaError, Expression, ExpressionNode,
    MAX_EMBEDDED_JSON_SCHEMA_BYTES, MAX_EMBEDDED_XML_SCHEMA_BYTES, NamedSourceProgram,
    NamedTargetProgram, Program, ScalarFunction, SourceIteration,
};
use ir::{NumericRange, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::Project;

const PHYSICAL_PROJECT: &str = include_str!("fixtures/unstable_float_project.json");

fn physical_program() -> Program {
    let project: Project = serde_json::from_str(PHYSICAL_PROJECT).expect("physical project parses");
    codegen::lower(&project).expect("physical project lowers")
}

fn stable_program() -> Program {
    let mut program = physical_program();
    program.target =
        SchemaNode::group("Target", vec![SchemaNode::scalar("Out", ScalarType::Float)]);
    program
}

fn changed_schema(name: &str) -> SchemaNode {
    let mut schema = physical_program().target;
    schema.name = name.into();
    schema
}

fn padded_schema(name: &str, bytes: usize) -> SchemaNode {
    let mut padding = SchemaNode::scalar("Padding", ScalarType::String);
    padding.fixed = Some(String::new());
    let mut schema = SchemaNode::group(
        name,
        vec![SchemaNode::scalar("Out", ScalarType::Float), padding],
    );
    let overhead = serde_json::to_string(&schema).unwrap().len();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!();
    };
    children[1].fixed = Some("x".repeat(bytes - overhead));
    assert_eq!(serde_json::to_string(&schema).unwrap().len(), bytes);
    schema
}

fn padded_lossless_schema(name: &str, bytes: usize) -> SchemaNode {
    let mut schema = changed_schema(name);
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    let mut padding = SchemaNode::scalar("Padding", ScalarType::String);
    padding.fixed = Some(String::new());
    children.push(padding);
    let overhead = codegen::serialize_embedded_schema(&schema, usize::MAX)
        .unwrap()
        .len();
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    children[1].fixed = Some("x".repeat(bytes - overhead));
    let descriptor = codegen::serialize_embedded_schema(&schema, usize::MAX).unwrap();
    assert!(descriptor.starts_with("FERRULE-EMBEDDED-SCHEMA/2\n"));
    assert_eq!(descriptor.len(), bytes);
    schema
}

#[test]
fn embedded_schema_emission_preserves_physical_constraint_without_decimal_drift() {
    let program = physical_program();
    let Some(NumericRange::Number(range)) = program.target.child("Out").unwrap().numeric_range
    else {
        panic!("physical fixture has a number range");
    };
    let Expression::Const {
        value: Value::Float(value),
    } = program.expressions[0].expression
    else {
        panic!("physical fixture has a floating constant");
    };
    assert!(
        !range.contains(value),
        "interpreter constraint rejects the constant"
    );
    let encoded = serde_json::to_string(&program.target).expect("schema serializes");
    let decoded: SchemaNode = serde_json::from_str(&encoded).expect("encoded schema parses");
    let Some(NumericRange::Number(weakened)) = decoded.child("Out").unwrap().numeric_range else {
        panic!("reparsed fixture has a number range");
    };
    assert!(
        weakened.contains(value),
        "unprotected embedding would weaken the range"
    );
    let descriptor =
        codegen::serialize_embedded_schema(&program.target, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
            .expect("physical metadata encodes losslessly");
    assert!(descriptor.starts_with("FERRULE-EMBEDDED-SCHEMA/2\n"));
    let artifacts = emit_schema_fixture(&program).expect("physical fixture emits losslessly");
    assert_descriptor_is_emitted(&artifacts, &descriptor);
}

fn assert_descriptor_is_emitted(artifacts: &ArtifactSet, descriptor: &str) {
    let literal = serde_json::to_string(descriptor).expect("descriptor escapes");
    assert!(
        artifacts
            .files()
            .iter()
            .any(|file| { String::from_utf8_lossy(&file.contents).contains(&literal) }),
        "generated artifacts contain the exact descriptor"
    );
}

#[test]
fn embedded_schema_emission_preserves_primary_named_dynamic_and_expression_schemas() {
    let mut cases = Vec::new();

    let mut primary = stable_program();
    primary.source = changed_schema("PrimaryInput");
    cases.push((primary, "PrimaryInput"));

    let mut named = stable_program();
    named.extra_sources.push(NamedSourceProgram {
        name: "Named".into(),
        source: changed_schema("NamedInput"),
        dynamic: None,
    });
    cases.push((named, "NamedInput"));

    let mut dynamic = stable_program();
    dynamic.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group("Rows", vec![]).repeating()],
    );
    dynamic.expressions.push(ExpressionNode {
        id: 1,
        expression: Expression::Const {
            value: Value::String("logical.json".into()),
        },
    });
    dynamic.extra_sources.push(NamedSourceProgram {
        name: "Dynamic".into(),
        source: changed_schema("DynamicInput"),
        dynamic: Some(DynamicSourceProgram {
            path: 1,
            driver: SourceIteration::new(vec!["Rows".into()]),
        }),
    });
    cases.push((dynamic, "DynamicInput"));

    let mut output = stable_program();
    output.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: changed_schema("NamedOutput"),
        root: output.root.clone(),
    });
    cases.push((output, "NamedOutput"));

    let mut xml = stable_program();
    xml.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group(
            "Serialized",
            vec![SchemaNode::scalar("Out", ScalarType::Float)],
        )],
    );
    xml.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Out", ScalarType::String)],
    );
    xml.root.bindings[0].target_domain = ScalarType::String.into();
    xml.expressions[0].expression = Expression::XmlSerialize {
        frame: None,
        path: vec!["Serialized".into()],
        schema: Box::new(changed_schema("Serialized")),
        declaration: false,
        indent: false,
        namespace: None,
    };
    cases.push((xml, "Serialized"));

    for (program, name) in cases {
        codegen::validate_program(&program).expect("fixture is valid before emission");
        let descriptor = codegen::serialize_embedded_schema(
            &changed_schema(name),
            MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )
        .expect("unstable schema encodes");
        assert!(descriptor.starts_with("FERRULE-EMBEDDED-SCHEMA/2\n"));
        let artifacts = emit_schema_fixture(&program).expect("valid metadata emits losslessly");
        assert_descriptor_is_emitted(&artifacts, &descriptor);
    }
}

#[test]
fn embedded_schema_emission_accepts_stable_boundaries_and_preserves_projection_text() {
    let mut program = stable_program();
    let mut stable = changed_schema("Stable");
    let SchemaKind::Group { children, .. } = &mut stable.kind else {
        panic!("fixture is a group");
    };
    children[0].numeric_range = Some(
        serde_json::from_str(r#"{"kind":"number","bounds":{"minimum":{"value":1.5}}}"#)
            .expect("stable range parses"),
    );
    program.source = stable.clone();
    program.extra_sources.push(NamedSourceProgram {
        name: "Named".into(),
        source: stable.clone(),
        dynamic: None,
    });
    program.target = stable.clone();
    program.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: stable,
        root: program.root.clone(),
    });
    assert!(emit_schema_fixture(&program).is_ok());

    let mut projection = stable_program();
    let descriptor = r#"{"name":"Parsed","kind":{"kind":"group","children":[{"name":"Out","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}},"kind":{"kind":"scalar","ty":"float"}}]}}"#;
    projection.expressions = vec![
        ExpressionNode {
            id: 0,
            expression: Expression::Const {
                value: Value::String(r#"{"Out":1e-307}"#.into()),
            },
        },
        ExpressionNode {
            id: 1,
            expression: Expression::Const {
                value: Value::String(descriptor.into()),
            },
        },
        ExpressionNode {
            id: 2,
            expression: Expression::Const {
                value: Value::String(r#"["Out"]"#.into()),
            },
        },
        ExpressionNode {
            id: 3,
            expression: Expression::Call {
                function: ScalarFunction::JsonParseField,
                args: vec![0, 1, 2],
            },
        },
    ];
    projection.root.bindings[0].expression = 3;
    let artifacts =
        emit_schema_fixture(&projection).expect("raw projection descriptor is preserved");
    let descriptor_literal = serde_json::to_string(descriptor).expect("descriptor escapes");
    assert!(
        artifacts
            .files()
            .iter()
            .any(|file| String::from_utf8_lossy(&file.contents).contains(&descriptor_literal))
    );
    assert!(
        matches!(&projection.expressions[1].expression, Expression::Const { value: Value::String(text) } if text == descriptor)
    );
}

#[test]
fn embedded_schema_emission_enforces_each_json_boundary_cap_before_artifacts() {
    let max = MAX_EMBEDDED_JSON_SCHEMA_BYTES;
    let mut accepted = stable_program();
    accepted.source = padded_schema("Source", max);
    assert!(emit_schema_fixture(&accepted).is_ok());

    let mut cases = Vec::new();
    let mut primary = stable_program();
    primary.source = padded_schema("PrimaryInput", max + 1);
    cases.push((primary, "PrimaryInput"));
    let mut primary = stable_program();
    primary.target = padded_schema("PrimaryOutput", max + 1);
    cases.push((primary, "PrimaryOutput"));

    let mut named = stable_program();
    named.extra_sources.push(NamedSourceProgram {
        name: "Named".into(),
        source: padded_schema("NamedInput", max + 1),
        dynamic: None,
    });
    cases.push((named, "NamedInput"));

    let mut dynamic = stable_program();
    dynamic.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group("Rows", vec![]).repeating()],
    );
    dynamic.expressions.push(ExpressionNode {
        id: 1,
        expression: Expression::Const {
            value: Value::String("logical.json".into()),
        },
    });
    dynamic.extra_sources.push(NamedSourceProgram {
        name: "Dynamic".into(),
        source: padded_schema("DynamicInput", max + 1),
        dynamic: Some(DynamicSourceProgram {
            path: 1,
            driver: SourceIteration::new(vec!["Rows".into()]),
        }),
    });
    cases.push((dynamic, "DynamicInput"));

    let mut named = stable_program();
    named.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: padded_schema("NamedOutput", max + 1),
        root: named.root.clone(),
    });
    cases.push((named, "NamedOutput"));

    for (program, name) in cases {
        codegen::validate_program(&program).expect("oversize schema is otherwise valid");
        assert_eq!(
            emit_schema_fixture(&program),
            Err(EmbeddedSchemaError::TooLarge {
                schema: name.into(),
                bytes: max + 1,
                max
            })
        );
    }
}

#[test]
fn embedded_schema_emission_uses_xml_expression_cap_independently() {
    let make = |bytes| {
        let mut program = stable_program();
        program.source = SchemaNode::group(
            "Source",
            vec![SchemaNode::group(
                "Serialized",
                vec![
                    SchemaNode::scalar("Out", ScalarType::Float),
                    SchemaNode::scalar("Padding", ScalarType::String),
                ],
            )],
        );
        program.target = SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Out", ScalarType::String)],
        );
        program.root.bindings[0].target_domain = ScalarType::String.into();
        program.expressions[0].expression = Expression::XmlSerialize {
            frame: None,
            path: vec!["Serialized".into()],
            schema: Box::new(padded_schema("Serialized", bytes)),
            declaration: false,
            indent: false,
            namespace: None,
        };
        program
    };
    assert!(emit_schema_fixture(&make(MAX_EMBEDDED_JSON_SCHEMA_BYTES + 1)).is_ok());
    assert!(emit_schema_fixture(&make(MAX_EMBEDDED_XML_SCHEMA_BYTES)).is_ok());
    assert_eq!(
        emit_schema_fixture(&make(MAX_EMBEDDED_XML_SCHEMA_BYTES + 1)),
        Err(EmbeddedSchemaError::TooLarge {
            schema: "Serialized".into(),
            bytes: MAX_EMBEDDED_XML_SCHEMA_BYTES + 1,
            max: MAX_EMBEDDED_XML_SCHEMA_BYTES,
        })
    );
}

#[test]
fn embedded_schema_emission_counts_lossless_encoding_before_artifacts() {
    let max = MAX_EMBEDDED_JSON_SCHEMA_BYTES;
    let mut program = stable_program();
    program.source = padded_lossless_schema("Input", max);
    assert!(emit_schema_fixture(&program).is_ok());
    program.source = padded_lossless_schema("Input", max + 1);
    assert_eq!(
        emit_schema_fixture(&program),
        Err(EmbeddedSchemaError::TooLarge {
            schema: "Input".into(),
            bytes: max + 1,
            max,
        })
    );

    let max = MAX_EMBEDDED_XML_SCHEMA_BYTES;
    let mut program = stable_program();
    program.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group(
            "Serialized",
            vec![
                SchemaNode::scalar("Out", ScalarType::Float),
                SchemaNode::scalar("Padding", ScalarType::String),
            ],
        )],
    );
    program.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Out", ScalarType::String)],
    );
    program.root.bindings[0].target_domain = ScalarType::String.into();
    let make = |schema| Expression::XmlSerialize {
        frame: None,
        path: vec!["Serialized".into()],
        schema: Box::new(schema),
        declaration: false,
        indent: false,
        namespace: None,
    };
    program.expressions[0].expression = make(padded_lossless_schema("Serialized", max));
    assert!(emit_schema_fixture(&program).is_ok());
    program.expressions[0].expression = make(padded_lossless_schema("Serialized", max + 1));
    assert_eq!(
        emit_schema_fixture(&program),
        Err(EmbeddedSchemaError::TooLarge {
            schema: "Serialized".into(),
            bytes: max + 1,
            max,
        })
    );
}

include!("structured_xml_transport_emission.rs");
