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

// Decoder fixtures select v2 explicitly, independently of encode's version
// choice. This is the literal wire shape of one Float minimum, not an encoder
// walk copied into the test. The input bits are the independent oracle.
const V2_LOW_VALUE: &str = concat!(
    "FERRULE-EMBEDDED-SCHEMA/2\n",
    r#"{"name":"Value","repeating":false,"numeric_range":{"kind":"number","bounds":{"minimum":{"value":"FERRULE-F64-BITS:0031fa182c40c60d"}}},"kind":{"kind":"scalar","ty":"float"}}"#,
);

fn v2_range_payload(name: &str, bits: u64) -> serde_json::Value {
    let mut payload: serde_json::Value = serde_json::from_str(
        V2_LOW_VALUE
            .strip_prefix(V2_PREFIX)
            .expect("literal v2 fixture"),
    )
    .expect("literal range JSON");
    payload["name"] = serde_json::Value::String(name.into());
    payload["numeric_range"]["bounds"]["minimum"]["value"] =
        serde_json::Value::String(format!("{FLOAT_BITS_MARKER_PREFIX}{bits:016x}"));
    payload
}

fn v2_descriptor(payload: &serde_json::Value) -> String {
    format!(
        "{V2_PREFIX}{}",
        serde_json::to_string(payload).expect("explicit fixture JSON serializes")
    )
}

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
    for bits in [LOW, HIGH, 0_f64.to_bits(), (-0_f64).to_bits()] {
        let schema = range_schema("Value", bits);
        let plain = serde_json::to_string(&schema).expect("ordinary schema JSON");
        let encoded = encode(&schema, LIMIT).expect("finite schema encodes");
        assert_eq!(encoded, plain, "ordinary JSON preserves bits {bits:016x}");
        let decoded = decode(&encoded, LIMIT).expect("ordinary schema decodes");
        assert_eq!(range_bits(&decoded), bits);
        assert_eq!(
            encode(&decoded, LIMIT).expect("canonical re-encode"),
            encoded
        );

        let forced_v2 = v2_descriptor(&v2_range_payload("Value", bits));
        let decoded_v2 = decode(&forced_v2, LIMIT).expect("explicit v2 schema decodes");
        assert_eq!(range_bits(&decoded_v2), bits);
        assert_eq!(decoded_v2, schema);
        assert_eq!(
            encode(&decoded_v2, LIMIT).expect("v2 canonicalizes to ordinary JSON"),
            plain
        );
    }

    let nested = SchemaNode::group(
        "Root",
        vec![
            range_schema("Unstable", LOW),
            range_schema("Zero", (-0_f64).to_bits()),
        ],
    );
    let plain = serde_json::to_string(&nested).expect("nested ordinary JSON");
    let encoded = encode(&nested, LIMIT).expect("nested signed zero encodes");
    assert_eq!(encoded, plain);
    let forced_v2 = v2_descriptor(&serde_json::json!({
        "name": "Root", "repeating": false,
        "kind": {"kind": "group", "children": [
            v2_range_payload("Unstable", LOW),
            v2_range_payload("Zero", (-0_f64).to_bits()),
        ]},
    }));
    for descriptor in [&encoded, &forced_v2] {
        let decoded = decode(descriptor, LIMIT).expect("nested signed zero decodes");
        assert_eq!(decoded, nested);
        let ir::SchemaKind::Group { children, .. } = &decoded.kind else {
            panic!("root is a group");
        };
        assert_eq!(range_bits(&children[0]), LOW);
        assert_eq!(range_bits(&children[1]), (-0_f64).to_bits());
        assert_eq!(
            encode(&decoded, LIMIT).expect("nested canonical re-encode"),
            plain
        );
    }
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

    let plain = serde_json::to_string(&root).expect("ordinary metadata JSON");
    let encoded = encode(&root, LIMIT).expect("all metadata encodes");
    assert_eq!(encoded, plain);
    let forced_v2 = concat!(
        "FERRULE-EMBEDDED-SCHEMA/2\n",
        r#"{
          "name":"Root",
          "json_dependent_schemas":[{"trigger":"Trigger","predicate":{"kind":"schema","schema":{
            "name":"Root","kind":{"kind":"group","children":[{
              "name":"Nested","numeric_range":{"kind":"number","bounds":{"minimum":{"value":"FERRULE-F64-BITS:0031fa182c40c60e"}}},"kind":{"kind":"scalar","ty":"float"}
            }]}
          }}}],
          "kind":{"kind":"group","children":[
            {"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},
            {"name":"Allowed","json_allowed_values":[{"type":"int","value":0},{"type":"float","value":"FERRULE-F64-BITS:0031fa182c40c60d"}],"kind":{"kind":"scalar","ty":"float"}},
            {"name":"Alternative","kind":{"kind":"group","children":[{"name":"Out","kind":{"kind":"scalar","ty":"float"}}],"alternatives":[{"name":"Selected","members":["Out"],"required":["Out"],"constraints":[{"member":"Out","value":{"type":"float","value":"FERRULE-F64-BITS:0031fa182c40c60d"}}]}]}},
            {"name":"Items","repeating":true,"json_contains":[{"predicate":{"kind":"schema","schema":{"name":"Item","numeric_range":{"kind":"number","bounds":{"minimum":{"value":"FERRULE-F64-BITS:0031fa182c40c60d"}}},"kind":{"kind":"scalar","ty":"float"}}},"range":{"minimum":1}}],"kind":{"kind":"scalar","ty":"float"}}
          ],"dynamic":{"name":"Dynamic","numeric_range":{"kind":"number","bounds":{"minimum":{"value":"FERRULE-F64-BITS:0031fa182c40c60d"}}},"kind":{"kind":"scalar","ty":"float"}}}
        }"#,
    );
    let low_marker = format!("{FLOAT_BITS_MARKER_PREFIX}{LOW:016x}");
    let high_marker = format!("{FLOAT_BITS_MARKER_PREFIX}{HIGH:016x}");
    assert_eq!(forced_v2.matches(&low_marker).count(), 4);
    assert_eq!(forced_v2.matches(&high_marker).count(), 1);
    for descriptor in [encoded.as_str(), forced_v2] {
        let decoded = decode(descriptor, LIMIT).expect("all metadata decodes");
        assert_eq!(decoded, root);
        assert_eq!(encode(&decoded, LIMIT).expect("canonical re-encode"), plain);
        let ordinary_decoded = serde_json::to_value(decoded).expect("decoded metadata tree");
        let ordinary_root = serde_json::to_value(&root).expect("original metadata tree");
        assert_eq!(ordinary_decoded, ordinary_root);
    }
}

#[test]
fn marker_validation_and_string_domain_do_not_conflict() {
    let schema = range_schema("Value", LOW);
    let descriptor = V2_LOW_VALUE;
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
        encoded,
        serde_json::to_string(&named).expect("ordinary named schema")
    );
    let named_v2 = v2_descriptor(&v2_range_payload(&valid, LOW));
    for descriptor in [&encoded, &named_v2] {
        let decoded = decode(descriptor, LIMIT).expect("literal marker name survives");
        assert_eq!(decoded.name, valid);
        assert_eq!(range_bits(&decoded), LOW);
    }

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
    let literal_encoded = encode(&literal, LIMIT).expect("ordinary literal string marker");
    assert_eq!(
        literal_encoded,
        serde_json::to_string(&literal).expect("ordinary string schema")
    );
    let literal_v2 = v2_descriptor(&serde_json::json!({
        "name": "Root", "kind": {"kind": "group", "children": [
            v2_range_payload("Unstable", LOW),
            {"name": "Text", "json_allowed_values": [
                {"type": "string", "value": valid.clone()},
                {"type": "string", "value": "ordinary"},
            ], "kind": {"kind": "scalar", "ty": "string"}},
        ]},
    }));
    for descriptor in [&literal_encoded, &literal_v2] {
        assert_eq!(
            decode(descriptor, LIMIT).expect("literal string decodes"),
            literal
        );
    }

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
    let plain = serde_json::to_string(&schema).expect("ordinary schema JSON");
    let encoded = encode(&schema, LIMIT).expect("ordinary schema encodes");
    assert_eq!(encoded, plain);
    let descriptor = V2_LOW_VALUE;
    for document in [encoded.as_str(), descriptor] {
        assert_eq!(
            decode(document, document.len()).expect("exact byte limit"),
            schema
        );
        assert_eq!(
            decode(document, document.len() - 1),
            Err(CodecError::TooLarge {
                bytes: document.len(),
                max: document.len() - 1
            })
        );
        let spaced = document.replacen("\"name\":", "\"name\" :", 1);
        assert_eq!(
            decode(&spaced, LIMIT).expect("JSON whitespace is accepted"),
            schema
        );
    }
    assert_eq!(
        encode(&schema, plain.len()).expect("exact ordinary encoder limit"),
        plain
    );
    assert_eq!(
        encode(&schema, plain.len() - 1),
        Err(CodecError::TooLarge {
            bytes: plain.len(),
            max: plain.len() - 1
        })
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
    let unicode_v2 = v2_descriptor(&v2_range_payload("é", LOW));
    assert_eq!(
        decode(&unicode_v2, unicode_v2.len()).expect("exact v2 UTF-8 byte limit"),
        range_schema("é", LOW)
    );
    assert_eq!(
        decode(&unicode_v2, unicode_v2.len() - 1),
        Err(CodecError::TooLarge {
            bytes: unicode_v2.len(),
            max: unicode_v2.len() - 1
        })
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
    let plain = serde_json::to_string(&schema).expect("ordinary bounded schema");
    assert_eq!(
        encode(&schema, LIMIT).expect("ordinary metadata encodes"),
        plain
    );
    let ordinary_decoded = decode(&plain, LIMIT).expect("ordinary metadata decodes");
    assert_eq!(ordinary_decoded, schema);
    let ir::SchemaKind::Group { children, .. } = &ordinary_decoded.kind else {
        panic!("ordinary root remains a group");
    };
    assert_eq!(range_bits(&children[0]), 0_f64.to_bits());
    assert_eq!(range_bits(&children[1]), LOW);
    let mut upper_payload = v2_range_payload("Upper", 0_f64.to_bits());
    upper_payload["numeric_range"]["bounds"]["maximum"] = serde_json::json!({
        "value": "FERRULE-F64-BITS:0031fa182c40c60d",
    });
    let mut payload = serde_json::json!({
        "name": "Root", "kind": {"kind": "group", "children": [
            upper_payload, v2_range_payload("Lower", LOW),
        ]},
    });
    assert_eq!(
        decode(&v2_descriptor(&payload), LIMIT).expect("explicit v2 metadata"),
        schema
    );
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
    let mut ordinary_checked = 0_usize;
    let mut v2_checked = 0_usize;
    let mut check = |bits: u64| {
        let schema = range_schema("Value", bits);
        let plain = serde_json::to_string(&schema).expect("plain metadata serializes");
        let ordinary = serde_json::from_str::<SchemaNode>(&plain).expect("finite JSON is exact");
        assert_eq!(
            range_bits(&ordinary),
            bits,
            "ordinary parser bits {bits:016x}"
        );
        assert_eq!(
            serde_json::to_string(&ordinary).expect("ordinary canonical bytes"),
            plain
        );
        let descriptor = encode(&schema, LIMIT).expect("finite metadata encodes");
        assert_eq!(
            descriptor, plain,
            "finite bits {bits:016x} retain ordinary bytes"
        );
        let decoded = decode(&descriptor, LIMIT).expect("ordinary finite metadata decodes");
        assert_eq!(
            range_bits(&decoded),
            bits,
            "ordinary descriptor bits {bits:016x}"
        );
        ordinary_checked += 1;

        let forced_v2 = v2_descriptor(&v2_range_payload("Value", bits));
        let decoded_v2 = decode(&forced_v2, LIMIT).expect("explicit finite v2 metadata decodes");
        assert_eq!(
            range_bits(&decoded_v2),
            bits,
            "v2 descriptor bits {bits:016x}"
        );
        assert_eq!(decoded_v2, schema);
        assert_eq!(
            encode(&decoded_v2, LIMIT).expect("v2 canonical ordinary bytes"),
            plain
        );
        v2_checked += 1;
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
    assert_eq!(finite_checked, 4096);
    assert_eq!(ordinary_checked, edges.len() + 4096);
    assert_eq!(
        v2_checked,
        edges.len() + 4096,
        "both versions checked for every finite input"
    );
}

#[test]
fn v2_keeps_the_ordinary_json_depth_limit() {
    let mut schema = range_schema("Leaf", LOW);
    let mut payload = v2_range_payload("Leaf", LOW);
    let mut deepest_supported = None;
    let mut refused_depth = false;
    for level in 0..64 {
        let plain = serde_json::to_string(&schema).expect("nested schema serializes");
        let forced_v2 = v2_descriptor(&payload);
        let ordinary_parses = serde_json::from_str::<serde_json::Value>(&plain).is_ok();
        let v2_parses = serde_json::from_str::<serde_json::Value>(
            forced_v2
                .strip_prefix(V2_PREFIX)
                .expect("explicit v2 prefix"),
        )
        .is_ok();
        assert_eq!(v2_parses, ordinary_parses, "v2 adds no JSON containers");
        if !ordinary_parses {
            assert!(matches!(
                decode(&forced_v2, LIMIT),
                Err(CodecError::Deserialization(_))
            ));
            refused_depth = true;
            break;
        }
        deepest_supported = Some((schema.clone(), forced_v2));
        let name = format!("Level{level}");
        schema = SchemaNode::group(&name, vec![schema]);
        payload = serde_json::json!({
            "name": name, "repeating": false,
            "kind": {"kind": "group", "children": [payload]},
        });
    }
    assert!(
        refused_depth,
        "the next container level reaches the ordinary JSON parser limit"
    );
    let (schema, forced_v2) = deepest_supported.expect("at least the leaf parses");
    let plain = serde_json::to_string(&schema).expect("deep ordinary schema JSON");
    let encoded = encode(&schema, LIMIT).expect("deep ordinary metadata encodes");
    assert_eq!(encoded, plain);
    for descriptor in [&encoded, &forced_v2] {
        let decoded = decode(descriptor, LIMIT).expect("same parser depth supports both versions");
        assert_eq!(decoded, schema);
        assert_eq!(
            encode(&decoded, LIMIT).expect("deep canonical re-encode"),
            plain
        );
    }
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
