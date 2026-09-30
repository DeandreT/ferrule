use codegen_schema::{CodecError, FLOAT_BITS_MARKER_PREFIX, V2_PREFIX, decode, encode};
use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    IntegerRange, ItemCountRange, JsonAllowedValue, JsonAllowedValues, JsonContainsConstraint,
    JsonContainsConstraints, JsonDependentSchemaConstraint, JsonDependentSchemaConstraints,
    JsonSchemaPredicate, NumberBound, NumberRange, NumericRange, ScalarType, SchemaNode,
};

const LIMIT: usize = 1024 * 1024;
const LOW: u64 = 0x0031_fa18_2c40_c60d;
const HIGH: u64 = 0x0031_fa18_2c40_c60e;

fn range_schema(name: &str, bits: u64) -> SchemaNode {
    let finite = FiniteF64::new(f64::from_bits(bits)).expect("test bits are finite");
    let range = NumberRange::new(Some(NumberBound::inclusive(finite)), None)
        .expect("one-sided range is valid");
    SchemaNode::scalar(name, ScalarType::Float)
        .with_numeric_range(NumericRange::Number(range))
        .expect("float range is valid")
}

fn range_bits(schema: &SchemaNode) -> u64 {
    let Some(NumericRange::Number(range)) = schema.numeric_range else {
        panic!("number range is present");
    };
    range
        .minimum()
        .expect("minimum exists")
        .value()
        .get()
        .to_bits()
}

#[test]
fn stable_schema_keeps_exact_v1_json() {
    let schema = range_schema("Value", 1.5_f64.to_bits());
    let old = serde_json::to_string(&schema).expect("schema serializes");
    assert_eq!(encode(&schema, LIMIT).expect("stable schema encodes"), old);
    assert_eq!(decode(&old, LIMIT).expect("v1 schema decodes"), schema);
}

#[test]
fn adjacent_unstable_floats_and_signed_zero_keep_all_bits() {
    for bits in [LOW, HIGH] {
        let schema = range_schema("Value", bits);
        let encoded = encode(&schema, LIMIT).expect("unstable schema encodes");
        assert!(encoded.starts_with(V2_PREFIX));
        assert!(encoded.contains(&format!("{FLOAT_BITS_MARKER_PREFIX}{bits:016x}")));
        let decoded = decode(&encoded, LIMIT).expect("v2 schema decodes");
        assert_eq!(range_bits(&decoded), bits);
        assert_eq!(
            encode(&decoded, LIMIT).expect("canonical re-encode"),
            encoded
        );
    }

    for bits in [0_f64.to_bits(), (-0_f64).to_bits()] {
        let schema = range_schema("Zero", bits);
        let encoded = encode(&schema, LIMIT).expect("signed zero encodes");
        assert_eq!(
            range_bits(&decode(&encoded, LIMIT).expect("signed zero decodes")),
            bits
        );
    }

    let nested = SchemaNode::group(
        "Root",
        vec![
            range_schema("Unstable", LOW),
            range_schema("Zero", (-0_f64).to_bits()),
        ],
    );
    let encoded = encode(&nested, LIMIT).expect("v2 with signed zero encodes");
    assert!(encoded.starts_with(V2_PREFIX));
    assert!(encoded.contains(&format!(
        "{FLOAT_BITS_MARKER_PREFIX}{:016x}",
        (-0_f64).to_bits()
    )));
    let decoded = decode(&encoded, LIMIT).expect("v2 signed zero decodes");
    let ir::SchemaKind::Group { children, .. } = decoded.kind else {
        panic!("root is a group");
    };
    assert_eq!(range_bits(&children[1]), (-0_f64).to_bits());
}

#[test]
fn nested_ranges_allowed_values_alternatives_and_predicates_roundtrip() {
    let low = FiniteF64::new(f64::from_bits(LOW)).expect("finite");
    let allowed = JsonAllowedValues::new([JsonAllowedValue::Int(0), JsonAllowedValue::Float(low)])
        .expect("canonical allowed values");
    let allowed_child = SchemaNode::scalar("Allowed", ScalarType::Float)
        .with_json_allowed_values(allowed)
        .expect("float allowed values");

    let alternative = SchemaNode::group(
        "Alternative",
        vec![SchemaNode::scalar("Out", ScalarType::Float)],
    )
    .with_alternatives(vec![GroupAlternative {
        name: "Selected".into(),
        members: vec!["Out".into()],
        required: vec!["Out".into()],
        constraints: vec![GroupAlternativeConstraint {
            member: "Out".into(),
            value: GroupAlternativeConstraintValue::Float(low),
        }],
    }])
    .expect("float alternative");

    let contains = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonSchemaPredicate::schema(range_schema("Item", LOW)),
        ItemCountRange::new(1, None).expect("positive count"),
    )])
    .expect("contains predicate");
    let items = SchemaNode::scalar("Items", ScalarType::Float)
        .repeating()
        .with_json_contains(contains)
        .expect("float contains is valid");

    let dependent = JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
        "Trigger",
        JsonSchemaPredicate::schema(SchemaNode::group(
            "Root",
            vec![range_schema("Nested", HIGH)],
        )),
    )])
    .expect("dependent predicate");
    let root = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Trigger", ScalarType::Bool),
            allowed_child,
            alternative,
            items,
        ],
    )
    .with_dynamic_fields(range_schema("Dynamic", LOW))
    .expect("dynamic schema")
    .with_json_dependent_schemas(dependent)
    .expect("dependent schema");

    let encoded = encode(&root, LIMIT).expect("all metadata encodes");
    assert!(encoded.starts_with(V2_PREFIX));
    let low_marker = format!("{FLOAT_BITS_MARKER_PREFIX}{LOW:016x}");
    let high_marker = format!("{FLOAT_BITS_MARKER_PREFIX}{HIGH:016x}");
    assert!(
        encoded.matches(&low_marker).count() >= 4,
        "all nested low values marked"
    );
    assert!(encoded.contains(&high_marker));
    let decoded = decode(&encoded, LIMIT).expect("all metadata decodes");
    assert_eq!(
        encode(&decoded, LIMIT).expect("canonical re-encode"),
        encoded
    );
    assert_eq!(decoded, root);
}

#[test]
fn marker_validation_and_string_domain_do_not_conflict() {
    let schema = range_schema("Value", LOW);
    let descriptor = encode(&schema, LIMIT).expect("v2 schema");
    let valid = format!("{FLOAT_BITS_MARKER_PREFIX}{LOW:016x}");
    for invalid in [
        "FERRULE-F64-BITS:0031FA182C40C60D",
        "FERRULE-F64-BITS:0031fa182c40c60",
        "FERRULE-F64-BITS:0031fa182c40c60g",
        "FERRULE-F64-BITS:7ff0000000000000",
        "FERRULE-F64-BITS:7ff8000000000000",
    ] {
        let malformed = descriptor.replace(&valid, invalid);
        assert!(matches!(
            decode(&malformed, LIMIT),
            Err(CodecError::InvalidFloatMarker(_))
        ));
    }
    let missing = descriptor.replace(&format!("\"{valid}\""), "1e-307");
    assert!(matches!(
        decode(&missing, LIMIT),
        Err(CodecError::InvalidFloatMarker(_))
    ));
    assert_eq!(
        decode("FERRULE-EMBEDDED-SCHEMA/3\n{}", LIMIT),
        Err(CodecError::UnsupportedVersion)
    );

    let mut named = schema.clone();
    named.name = valid.clone();
    let encoded = encode(&named, LIMIT).expect("literal marker name is valid");
    assert_eq!(
        decode(&encoded, LIMIT)
            .expect("literal marker name survives")
            .name,
        valid
    );

    let allowed = JsonAllowedValues::new([
        JsonAllowedValue::String("ordinary".into()),
        JsonAllowedValue::String(valid.clone()),
    ])
    .expect("marker text is an ordinary allowed string");
    let literal = SchemaNode::group(
        "Root",
        vec![
            range_schema("Unstable", LOW),
            SchemaNode::scalar("Text", ScalarType::String)
                .with_json_allowed_values(allowed)
                .expect("string allowed values are valid"),
        ],
    );
    let literal_encoded = encode(&literal, LIMIT).expect("v2 with literal string marker");
    assert_eq!(
        decode(&literal_encoded, LIMIT).expect("literal string decodes"),
        literal
    );

    let foreign = descriptor.replacen(
        "\"name\":",
        "\"extra\":\"FERRULE-F64-BITS:0000000000000000\",\"name\":",
        1,
    );
    assert_eq!(
        decode(&foreign, LIMIT).expect("ordinary extra field is ignored"),
        schema
    );

    let integer = SchemaNode::scalar("Count", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(
            IntegerRange::new(Some(1), None).expect("integer range"),
        ))
        .expect("integer metadata");
    let mut integer_value = serde_json::to_value(integer).expect("integer schema serializes");
    integer_value["numeric_range"]["bounds"]["minimum"] = serde_json::Value::String(valid);
    let wrong_typed_slot = format!(
        "{V2_PREFIX}{}",
        serde_json::to_string(&integer_value).expect("malformed payload serializes")
    );
    assert!(matches!(
        decode(&wrong_typed_slot, LIMIT),
        Err(CodecError::Deserialization(_))
    ));
}

#[test]
fn exact_byte_limits_and_ordinary_json_flexibility() {
    let schema = range_schema("Value", LOW);
    let descriptor = encode(&schema, LIMIT).expect("v2 schema");
    assert!(decode(&descriptor, descriptor.len()).is_ok());
    assert_eq!(
        decode(&descriptor, descriptor.len() - 1),
        Err(CodecError::TooLarge {
            bytes: descriptor.len(),
            max: descriptor.len() - 1
        })
    );
    assert_eq!(
        encode(&schema, descriptor.len() - 1),
        Err(CodecError::TooLarge {
            bytes: descriptor.len(),
            max: descriptor.len() - 1
        })
    );
    let plain_unstable = serde_json::to_string(&schema).expect("plain schema serializes");
    assert_eq!(
        encode(&schema, plain_unstable.len() - 1),
        Err(CodecError::TooLarge {
            bytes: plain_unstable.len(),
            max: plain_unstable.len() - 1,
        })
    );
    let spaced = descriptor.replacen("\"name\":", "\"name\" :", 1);
    assert_eq!(
        decode(&spaced, LIMIT).expect("JSON whitespace is accepted"),
        schema
    );
    let mut payload: serde_json::Value =
        serde_json::from_str(descriptor.strip_prefix(V2_PREFIX).expect("v2 prefix"))
            .expect("v2 JSON parses");
    let fields = payload.as_object_mut().expect("schema is an object");
    let name = fields.remove("name").expect("schema has a name");
    fields.insert("name".into(), name);
    let reordered = format!(
        "{V2_PREFIX}{}",
        serde_json::to_string(&payload).expect("reordered schema serializes")
    );
    assert_ne!(reordered, descriptor);
    assert_eq!(
        decode(&reordered, LIMIT).expect("field order is accepted"),
        schema
    );
    let stable = serde_json::to_string(&range_schema("é", 1.5_f64.to_bits())).expect("v1");
    assert_eq!(
        decode(&stable, stable.len() - 1),
        Err(CodecError::TooLarge {
            bytes: stable.len(),
            max: stable.len() - 1
        })
    );
}

#[test]
fn v2_accepts_explicit_null_optional_metadata_but_not_required_slots() {
    let low = FiniteF64::new(f64::from_bits(LOW)).expect("finite lower edge");
    let zero = FiniteF64::new(0.0).expect("finite zero");
    let upper = SchemaNode::scalar("Upper", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(zero)),
                Some(NumberBound::inclusive(low)),
            )
            .expect("ordered range"),
        ))
        .expect("float metadata");
    let schema = SchemaNode::group("Root", vec![upper, range_schema("Lower", LOW)]);
    let descriptor = encode(&schema, LIMIT).expect("unstable metadata encodes");
    let mut payload: serde_json::Value =
        serde_json::from_str(descriptor.strip_prefix(V2_PREFIX).expect("v2 descriptor"))
            .expect("payload parses");
    for optional in [
        "numeric_range",
        "json_allowed_values",
        "json_contains",
        "json_dependent_schemas",
    ] {
        payload[optional] = serde_json::Value::Null;
    }
    payload["kind"]["dynamic"] = serde_json::Value::Null;
    payload["kind"]["children"][0]["numeric_range"]["bounds"]["minimum"] = serde_json::Value::Null;
    payload["kind"]["children"][1]["numeric_range"]["bounds"]["maximum"] = serde_json::Value::Null;

    let with_nulls = format!(
        "{V2_PREFIX}{}",
        serde_json::to_string(&payload).expect("null metadata serializes")
    );
    let decoded = decode(&with_nulls, LIMIT).expect("optional nulls decode");
    assert!(decoded.numeric_range.is_none());
    assert!(decoded.json_allowed_values.is_none());
    assert!(decoded.json_contains.is_none());
    assert!(decoded.json_dependent_schemas.is_none());
    let ir::SchemaKind::Group {
        children, dynamic, ..
    } = decoded.kind
    else {
        panic!("root remains a group");
    };
    assert!(dynamic.is_none());
    let Some(NumericRange::Number(upper_range)) = children[0].numeric_range else {
        panic!("upper number range remains present");
    };
    assert!(upper_range.minimum().is_none());
    assert_eq!(
        upper_range
            .maximum()
            .expect("upper bound")
            .value()
            .get()
            .to_bits(),
        LOW
    );
    let Some(NumericRange::Number(lower_range)) = children[1].numeric_range else {
        panic!("lower number range remains present");
    };
    assert_eq!(
        lower_range
            .minimum()
            .expect("lower bound")
            .value()
            .get()
            .to_bits(),
        LOW
    );
    assert!(lower_range.maximum().is_none());

    let ordinary = serde_json::to_string(&payload)
        .expect("ordinary payload serializes")
        .replace(
            &format!("\"{FLOAT_BITS_MARKER_PREFIX}{LOW:016x}\""),
            "1e-307",
        );
    assert!(
        serde_json::from_str::<SchemaNode>(&ordinary).is_ok(),
        "ordinary typed schema JSON accepts the same optional nulls"
    );

    let mut missing_children = payload.clone();
    missing_children["kind"]["children"] = serde_json::Value::Null;
    assert!(matches!(
        decode(&format!("{V2_PREFIX}{missing_children}"), LIMIT),
        Err(CodecError::InvalidShape(_))
    ));
    let mut missing_bounds = payload;
    missing_bounds["kind"]["children"][0]["numeric_range"]["bounds"] = serde_json::Value::Null;
    assert!(matches!(
        decode(&format!("{V2_PREFIX}{missing_bounds}"), LIMIT),
        Err(CodecError::InvalidShape(_))
    ));
}

#[test]
fn near_finite_edge_values_keep_their_bits() {
    for bits in [
        1_u64,
        f64::MIN_POSITIVE.to_bits(),
        f64::MAX.to_bits(),
        (-f64::MAX).to_bits(),
        (-f64::MIN_POSITIVE).to_bits(),
    ] {
        let schema = range_schema("Edge", bits);
        let encoded = encode(&schema, LIMIT).expect("finite edge encodes");
        let decoded = decode(&encoded, LIMIT).expect("finite edge decodes");
        assert_eq!(range_bits(&decoded), bits);
    }
}

#[test]
fn broad_finite_binary64_metadata_matches_legacy_stability_and_keeps_exact_bits() {
    let edges = [
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
        LOW,
        HIGH,
        0x3fef_ffff_ffff_fc19,
        0x3fef_ffff_ffff_fc1a,
    ];
    let mut stable = 0_usize;
    let mut unstable = 0_usize;
    let mut check = |bits: u64| {
        let schema = range_schema("Value", bits);
        let plain = serde_json::to_string(&schema).expect("plain metadata serializes");
        let legacy_preserves_bits =
            serde_json::from_str::<SchemaNode>(&plain)
                .ok()
                .is_some_and(|decoded| {
                    range_bits(&decoded) == bits
                        && serde_json::to_string(&decoded).expect("parsed metadata serializes")
                            == plain
                });
        let descriptor = encode(&schema, LIMIT).expect("finite metadata encodes");
        if legacy_preserves_bits {
            stable += 1;
            assert_eq!(descriptor, plain, "stable bits {bits:016x} retain v1 bytes");
        } else {
            unstable += 1;
            assert!(
                descriptor.starts_with(V2_PREFIX),
                "unstable bits {bits:016x} use v2"
            );
            assert!(
                descriptor.contains(&format!("{FLOAT_BITS_MARKER_PREFIX}{bits:016x}")),
                "unstable bits {bits:016x} appear as exact marker"
            );
        }
        let decoded = decode(&descriptor, LIMIT).expect("finite metadata decodes");
        assert_eq!(
            range_bits(&decoded),
            bits,
            "finite bits {bits:016x} survive"
        );
    };
    for bits in edges {
        check(bits);
    }

    // The fixed LCG seed and constants make this a reproducible sweep across
    // signs, exponents, and significands rather than a flaky random test.
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let mut finite_checked = 0_usize;
    while finite_checked < 4096 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        if f64::from_bits(state).is_finite() {
            check(state);
            finite_checked += 1;
        }
    }
    assert!(
        stable > 0 && unstable > 0,
        "both descriptor versions are exercised"
    );
}

#[test]
fn v2_keeps_the_ordinary_json_depth_limit() {
    let mut schema = range_schema("Leaf", LOW);
    let mut deepest_supported = None;
    for level in 0..64 {
        let plain = serde_json::to_string(&schema).expect("nested schema serializes");
        if serde_json::from_str::<serde_json::Value>(&plain).is_err() {
            break;
        }
        deepest_supported = Some(schema.clone());
        schema = SchemaNode::group(format!("Level{level}"), vec![schema]);
    }
    let schema = deepest_supported.expect("at least the leaf parses");
    let encoded = encode(&schema, LIMIT).expect("v2 adds no JSON containers");
    assert!(encoded.starts_with(V2_PREFIX));
    let decoded = decode(&encoded, LIMIT).expect("same parser depth supports v2");
    assert_eq!(encode(&decoded, LIMIT).expect("deep re-encode"), encoded);
}

#[test]
fn constructed_overdeep_schema_fails_before_recursive_serialization() {
    let mut schema = range_schema("Leaf", LOW);
    for level in 0..128 {
        schema = SchemaNode::group(format!("Level{level}"), vec![schema]);
    }
    assert_eq!(
        encode(&schema, LIMIT),
        Err(CodecError::DepthLimit {
            depth: 129,
            max: 128,
        })
    );
}
