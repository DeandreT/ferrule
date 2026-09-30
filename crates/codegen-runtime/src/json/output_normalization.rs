use ir::{Instance, SchemaNode, Value};

use super::serialize_json;

const LOW: u64 = 0x0031_fa18_2c40_c60d;
const HIGH: u64 = 0x0031_fa18_2c40_c60e;

fn scalar(bits: u64) -> Instance {
    Instance::Scalar(Value::Float(f64::from_bits(bits)))
}

fn assert_matches_native(schema_json: &str, instance: &Instance, accepted: bool) {
    let schema: SchemaNode = serde_json::from_str(schema_json).expect("valid embedded schema");
    let native = format_json::to_string(&schema, instance);
    let generated = serialize_json(schema_json, instance);
    assert_eq!(
        native.is_ok(),
        accepted,
        "native output contract differs for {instance:?}: {native:?}"
    );
    assert_eq!(
        generated.is_ok(),
        accepted,
        "generated output contract differs for {instance:?}: {generated:?}"
    );
    if let (Ok(native), Ok(generated)) = (native, generated) {
        assert_eq!(generated, native);
    }
}

#[test]
fn exact_multiple_of_uses_pre_serialization_float_value() {
    // serde_json 1.0.150 writes LOW as `1e-307`, then parses that text as
    // HIGH. It writes HIGH as `1.0000000000000001e-307`, then parses LOW.
    // Output constraints must inspect the normalized value before this lossy
    // text round trip, as the native formatter does.
    let schema = r#"{"name":"Value","json_multiple_of":{"any_of":[[{"coefficient":1,"decimal_exponent":-307}]]},"kind":{"kind":"scalar","ty":"float"}}"#;
    assert_matches_native(schema, &scalar(LOW), true);
    assert_matches_native(schema, &scalar(HIGH), false);
}

#[test]
fn allowed_values_use_pre_serialization_float_value() {
    let schema = r#"{"name":"Value","json_allowed_values":[{"type":"float","value":1e-307},{"type":"float","value":1.5}],"kind":{"kind":"scalar","ty":"float"}}"#;
    // The embedded 1e-307 metadata is parsed as HIGH, so LOW must fail and
    // HIGH must pass even though serde's output-text parse reverses them.
    assert_matches_native(schema, &scalar(LOW), false);
    assert_matches_native(schema, &scalar(HIGH), true);
    assert_matches_native(schema, &scalar(1.5_f64.to_bits()), true);
    assert_matches_native(schema, &scalar(2.0_f64.to_bits()), false);
}

#[test]
fn numeric_range_and_contains_keep_the_same_native_value() {
    let minimum = r#"{"kind":"number","bounds":{"minimum":{"value":1e-307}}}"#;
    let range_schema = format!(
        r#"{{"name":"Value","numeric_range":{minimum},"kind":{{"kind":"scalar","ty":"float"}}}}"#
    );
    assert_matches_native(&range_schema, &scalar(LOW), false);
    assert_matches_native(&range_schema, &scalar(HIGH), true);

    let contains_schema = format!(
        r#"{{"name":"Values","repeating":true,"json_contains":[{{"predicate":{{"kind":"schema","schema":{{"name":"item","numeric_range":{minimum},"kind":{{"kind":"scalar","ty":"float"}}}}}},"range":{{"minimum":1}}}}],"kind":{{"kind":"scalar","ty":"float"}}}}"#
    );
    let repeated = |bits| Instance::Repeated(vec![scalar(bits)]);
    assert_matches_native(&contains_schema, &repeated(LOW), false);
    assert_matches_native(&contains_schema, &repeated(HIGH), true);
}
