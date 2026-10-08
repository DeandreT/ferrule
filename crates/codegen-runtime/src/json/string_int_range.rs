use std::error::Error;
use std::fmt::Debug;

use ir::{Instance, SchemaNode, Value};

use super::{
    JsonBoundaryError, parse_json, parse_json_bytes, serialize_json, serialize_json_bytes,
};

fn descriptor(profile: &str) -> &'static str {
    match profile {
        r###"bounded"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"optional-nullable"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}]}}"###
        }
        r###"required-nullable"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"wide"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9007199254740993,"maximum":9007199254740995}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"min-singleton"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":-9223372036854775808,"maximum":-9223372036854775808}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"max-singleton"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9223372036854775807,"maximum":9223372036854775807}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"lower-only"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"upper-only"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"anyOf-nullable"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"oneOf-nullable"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"finite-enum"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"json_allowed_values":[{"type":"json_null"},{"type":"int","value":2},{"type":"int","value":6},{"type":"int","value":100},{"type":"string","value":"1"},{"type":"string","value":"100"},{"type":"string","value":"6"}]}],"required":["value"]}}"###
        }
        r###"finite-composition-filter"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"kind":{"kind":"scalar_union","types":["string","int"]},"json_allowed_values":[{"type":"json_null"},{"type":"int","value":2},{"type":"int","value":3},{"type":"string","value":"1"},{"type":"string","value":"100"},{"type":"string","value":"6"}]}],"required":["value"]}}"###
        }
        r###"array"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"values","repeating":true,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"container_nullable":true}],"required":["values"]}}"###
        }
        r###"old-unconstrained"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###
        }
        r###"independent-string-assertions"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"string_length_range":{"minimum":2,"maximum":3},"json_patterns":{"any_of":[["^x+$"]]},"json_formats":["date"]}],"required":["value"]}}"###
        }
        r###"independent-multiple-of"### => {
            r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":8}},"kind":{"kind":"scalar_union","types":["string","int"]},"json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]}}],"required":["value"]}}"###
        }
        _ => panic!("undeclared fixture profile"),
    }
}

// Capture is deliberately independent of the expected shape. Unexpected tags,
// instance kinds, metadata and public causes are retained without asserting.
fn instance_original(instance: &Instance) -> serde_json::Value {
    use serde_json::json;
    match instance {
        Instance::Scalar(value) => {
            let payload = match value {
                Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => serde_json::Value::Null,
                Value::String(value) => json!(value),
                Value::Int(value) => json!(value),
                Value::Bool(value) => json!(value),
                Value::Float(value) => {
                    json!({"bits":format!("{:016x}",value.to_bits()),"display":value.to_string()})
                }
            };
            json!({"kind":"Scalar","tag":value.type_name(),"payload":payload,"debug":format!("{value:#?}")})
        }
        Instance::Group(group) => {
            json!({"kind":"Group","xml_type_origin":format!("{:?}",group.xml_type_origin()),"fields":group.iter().map(|(name,value)| json!({"name":name,"value":instance_original(value)})).collect::<Vec<_>>()})
        }
        Instance::Repeated(items) => {
            json!({"kind":"Repeated","items":items.iter().map(instance_original).collect::<Vec<_>>()})
        }
        Instance::MappedSequence(items) => {
            json!({"kind":"MappedSequence","items":items.iter().map(instance_original).collect::<Vec<_>>()})
        }
        Instance::DocumentSet(documents) => {
            json!({"kind":"DocumentSet","documents":documents.iter().map(|document| json!({"path":document.path(),"source_path":document.source_path(),"value":instance_original(document.value()),"debug":format!("{document:#?}")})).collect::<Vec<_>>()})
        }
    }
}

fn original<T: Debug>(
    result: &Result<T, JsonBoundaryError>,
    value: impl Fn(&T) -> serde_json::Value,
) -> serde_json::Value {
    match result {
        Ok(actual) => {
            serde_json::json!({"status":"ok","value":value(actual),"debug":format!("{actual:#?}")})
        }
        Err(error) => {
            let typed = match error {
                JsonBoundaryError::InvalidInput { message } => {
                    serde_json::json!({"variant":"InvalidInput","message":message})
                }
                JsonBoundaryError::InvalidOutput { message } => {
                    serde_json::json!({"variant":"InvalidOutput","message":message})
                }
                _ => serde_json::json!({"variant":"unexpected","debug":format!("{error:#?}")}),
            };
            let mut sources = Vec::new();
            let mut next = error.source();
            while let Some(source) = next {
                sources.push(serde_json::json!({"display":source.to_string(),"debug":format!("{source:#?}")}));
                next = source.source();
            }
            serde_json::json!({"status":"error","typed":typed,"display":error.to_string(),"debug":format!("{error:#?}"),"sources":sources})
        }
    }
}

struct InputCase {
    name: &'static str,
    schema: &'static str,
    document: &'static str,
    expected: Result<Instance, JsonBoundaryError>,
}

struct OutputCase {
    name: &'static str,
    schema: &'static str,
    instance: Instance,
    expected: Result<&'static str, JsonBoundaryError>,
}

#[test]
fn integer_union_public_text_and_byte_boundaries_preserve_complete_typed_contract() {
    let inputs = [
    InputCase { name: r###"bounded-refuse-1"###, schema: descriptor(r###"bounded"###), document: r###"{"value":1}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [2, 5], got 1"###.to_string() }) },
    InputCase { name: r###"bounded-refuse-6"###, schema: descriptor(r###"bounded"###), document: r###"{"value":6}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [2, 5], got 6"###.to_string() }) },
    InputCase { name: r###"wide-refuse-9007199254740992"###, schema: descriptor(r###"wide"###), document: r###"{"value":9007199254740992}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [9007199254740993, 9007199254740995], got 9007199254740992"###.to_string() }) },
    InputCase { name: r###"wide-refuse-9007199254740996"###, schema: descriptor(r###"wide"###), document: r###"{"value":9007199254740996}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [9007199254740993, 9007199254740995], got 9007199254740996"###.to_string() }) },
    InputCase { name: r###"min-neighbor"###, schema: descriptor(r###"min-singleton"###), document: r###"{"value":-9223372036854775807}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [-9223372036854775808, -9223372036854775808], got -9223372036854775807"###.to_string() }) },
    InputCase { name: r###"max-neighbor"###, schema: descriptor(r###"max-singleton"###), document: r###"{"value":9223372036854775806}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [9223372036854775807, 9223372036854775807], got 9223372036854775806"###.to_string() }) },
    InputCase { name: r###"array-refuse-int"###, schema: descriptor(r###"array"###), document: r###"{"values":[6]}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`values` requires numeric range [2, 5], got 6"###.to_string() }) },
    InputCase { name: r###"input-shape-fraction-lexeme"###, schema: descriptor(r###"bounded"###), document: r###"{"value":1.0}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value`: expected declared scalar union, got number"###.to_string() }) },
    InputCase { name: r###"input-shape-exponent-lexeme"###, schema: descriptor(r###"bounded"###), document: r###"{"value":2e0}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value`: expected declared scalar union, got number"###.to_string() }) },
    InputCase { name: r###"input-shape-unsigned-overflow"###, schema: descriptor(r###"bounded"###), document: r###"{"value":9223372036854775808}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value`: expected declared scalar union, got number"###.to_string() }) },
    InputCase { name: r###"input-shape-null"###, schema: descriptor(r###"bounded"###), document: r###"{"value":null}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value`: expected declared scalar union, got null"###.to_string() }) },
    InputCase { name: r###"required-missing"###, schema: descriptor(r###"required-nullable"###), document: r###"{}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"object `Record` requires property `value`"###.to_string() }) },
    InputCase { name: r###"enum-int100-bounded"###, schema: descriptor(r###"finite-enum"###), document: r###"{"value":100}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires numeric range [2, 5], got 100"###.to_string() }) },
    InputCase { name: r###"independent-string-assertions-short"###, schema: descriptor(r###"independent-string-assertions"###), document: r###"{"value":"x"}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` requires string length between 2 and 3 Unicode scalar values, got 1 Unicode scalar values"###.to_string() }) },
    InputCase { name: r###"independent-string-assertions-pattern"###, schema: descriptor(r###"independent-string-assertions"###), document: r###"{"value":"yy"}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` does not match its JSON Schema pattern constraints"###.to_string() }) },
    InputCase { name: r###"multiple-int3"###, schema: descriptor(r###"independent-multiple-of"###), document: r###"{"value":3}"###, expected: Err(JsonBoundaryError::InvalidInput { message: r###"`value` is not an exact multiple of its JSON Schema multipleOf constraints"###.to_string() }) },
    InputCase { name: r###"string-empty"###, schema: descriptor(r###"bounded"###), document: r###"{"value":""}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###""###.to_string())))].into())) },
    InputCase { name: r###"string-word"###, schema: descriptor(r###"bounded"###), document: r###"{"value":"abc"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"abc"###.to_string())))].into())) },
    InputCase { name: r###"string-below"###, schema: descriptor(r###"bounded"###), document: r###"{"value":"1"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"1"###.to_string())))].into())) },
    InputCase { name: r###"string-above"###, schema: descriptor(r###"bounded"###), document: r###"{"value":"6"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"6"###.to_string())))].into())) },
    InputCase { name: r###"string-fraction"###, schema: descriptor(r###"bounded"###), document: r###"{"value":"1.0"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"1.0"###.to_string())))].into())) },
    InputCase { name: r###"string-spaces"###, schema: descriptor(r###"bounded"###), document: r###"{"value":" 6 "}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###" 6 "###.to_string())))].into())) },
    InputCase { name: r###"string-overflow"###, schema: descriptor(r###"bounded"###), document: r###"{"value":"9223372036854775808"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"9223372036854775808"###.to_string())))].into())) },
    InputCase { name: r###"int-min"###, schema: descriptor(r###"bounded"###), document: r###"{"value":2}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(2)))].into())) },
    InputCase { name: r###"int-max"###, schema: descriptor(r###"bounded"###), document: r###"{"value":5}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(5)))].into())) },
    InputCase { name: r###"optional-nullable-null"###, schema: descriptor(r###"optional-nullable"###), document: r###"{"value":null}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into())) },
    InputCase { name: r###"required-nullable-null"###, schema: descriptor(r###"required-nullable"###), document: r###"{"value":null}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into())) },
    InputCase { name: r###"optional-missing"###, schema: descriptor(r###"optional-nullable"###), document: r###"{}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Null))].into())) },
    InputCase { name: r###"wide-9007199254740993"###, schema: descriptor(r###"wide"###), document: r###"{"value":9007199254740993}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9007199254740993)))].into())) },
    InputCase { name: r###"wide-9007199254740995"###, schema: descriptor(r###"wide"###), document: r###"{"value":9007199254740995}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9007199254740995)))].into())) },
    InputCase { name: r###"exact-i64-min"###, schema: descriptor(r###"min-singleton"###), document: r###"{"value":-9223372036854775808}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(-9223372036854775808)))].into())) },
    InputCase { name: r###"exact-i64-max"###, schema: descriptor(r###"max-singleton"###), document: r###"{"value":9223372036854775807}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9223372036854775807)))].into())) },
    InputCase { name: r###"lower-only-max"###, schema: descriptor(r###"lower-only"###), document: r###"{"value":9223372036854775807}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9223372036854775807)))].into())) },
    InputCase { name: r###"upper-only-min"###, schema: descriptor(r###"upper-only"###), document: r###"{"value":-9223372036854775808}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(-9223372036854775808)))].into())) },
    InputCase { name: r###"finite-enum-string100"###, schema: descriptor(r###"finite-enum"###), document: r###"{"value":"100"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"100"###.to_string())))].into())) },
    InputCase { name: r###"finite-composition-filter-string100"###, schema: descriptor(r###"finite-composition-filter"###), document: r###"{"value":"100"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"100"###.to_string())))].into())) },
    InputCase { name: r###"finite-composition-filter-null"###, schema: descriptor(r###"finite-composition-filter"###), document: r###"{"value":null}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into())) },
    InputCase { name: r###"finite-extra-int"###, schema: descriptor(r###"finite-composition-filter"###), document: r###"{"value":3}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(3)))].into())) },
    InputCase { name: r###"old-unbounded-int"###, schema: descriptor(r###"old-unconstrained"###), document: r###"{"value":1}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(1)))].into())) },
    InputCase { name: r###"old-text-tag"###, schema: descriptor(r###"old-unconstrained"###), document: r###"{"value":"1"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"1"###.to_string())))].into())) },
    InputCase { name: r###"array-items"###, schema: descriptor(r###"array"###), document: r###"{"values":[2,"6",null,5]}"###, expected: Ok(Instance::Group(vec![(r###"values"###.to_string(),Instance::Repeated(vec![Instance::Scalar(Value::Int(2)),Instance::Scalar(Value::String(r###"6"###.to_string())),Instance::Scalar(Value::json_null()),Instance::Scalar(Value::Int(5))]))].into())) },
    InputCase { name: r###"array-wrapper-null"###, schema: descriptor(r###"array"###), document: r###"{"values":null}"###, expected: Ok(Instance::Group(vec![(r###"values"###.to_string(),Instance::Scalar(Value::json_null()))].into())) },
    InputCase { name: r###"array-empty"###, schema: descriptor(r###"array"###), document: r###"{"values":[]}"###, expected: Ok(Instance::Group(vec![(r###"values"###.to_string(),Instance::Repeated(vec![]))].into())) },
    InputCase { name: r###"independent-string-assertions-string"###, schema: descriptor(r###"independent-string-assertions"###), document: r###"{"value":"xx"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"xx"###.to_string())))].into())) },
    InputCase { name: r###"independent-string-assertions-int"###, schema: descriptor(r###"independent-string-assertions"###), document: r###"{"value":2}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(2)))].into())) },
    InputCase { name: r###"multiple-string3"###, schema: descriptor(r###"independent-multiple-of"###), document: r###"{"value":"3"}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"3"###.to_string())))].into())) },
    InputCase { name: r###"multiple-int2"###, schema: descriptor(r###"independent-multiple-of"###), document: r###"{"value":2}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(2)))].into())) },
    InputCase { name: r###"late-good-control"###, schema: descriptor(r###"bounded"###), document: r###"{"value":3}"###, expected: Ok(Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(3)))].into())) },
    InputCase { name: "old-root-int", schema: r###"{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}"###, document: "1", expected: Ok(Instance::Scalar(Value::Int(1))) },
    InputCase { name: "root-string100", schema: r###"{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}"###, document: r#""100""#, expected: Ok(Instance::Scalar(Value::String("100".to_string()))) },
    ];
    let outputs = [
    OutputCase { name: r###"bounded-refuse-1"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(1)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [2, 5], got 1"###.to_string() }) },
    OutputCase { name: r###"bounded-refuse-6"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(6)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [2, 5], got 6"###.to_string() }) },
    OutputCase { name: r###"wide-refuse-9007199254740992"###, schema: descriptor(r###"wide"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9007199254740992)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [9007199254740993, 9007199254740995], got 9007199254740992"###.to_string() }) },
    OutputCase { name: r###"wide-refuse-9007199254740996"###, schema: descriptor(r###"wide"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9007199254740996)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [9007199254740993, 9007199254740995], got 9007199254740996"###.to_string() }) },
    OutputCase { name: r###"min-neighbor"###, schema: descriptor(r###"min-singleton"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(-9223372036854775807)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [-9223372036854775808, -9223372036854775808], got -9223372036854775807"###.to_string() }) },
    OutputCase { name: r###"max-neighbor"###, schema: descriptor(r###"max-singleton"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9223372036854775806)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [9223372036854775807, 9223372036854775807], got 9223372036854775806"###.to_string() }) },
    OutputCase { name: r###"array-refuse-int"###, schema: descriptor(r###"array"###), instance: Instance::Group(vec![(r###"values"###.to_string(),Instance::Repeated(vec![Instance::Scalar(Value::Int(6))]))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`values` requires numeric range [2, 5], got 6"###.to_string() }) },
    OutputCase { name: r###"output-shape-float"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Float(1.0)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value`: expected declared scalar union, got float"###.to_string() }) },
    OutputCase { name: r###"output-shape-null"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value`: expected declared scalar union, got json null"###.to_string() }) },
    OutputCase { name: r###"output-shape-xml-nil"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::xml_nil()))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value`: expected declared scalar union, got xml nil"###.to_string() }) },
    OutputCase { name: r###"required-missing"###, schema: descriptor(r###"required-nullable"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Null))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"object `Record` requires property `value`"###.to_string() }) },
    OutputCase { name: r###"enum-int100-bounded"###, schema: descriptor(r###"finite-enum"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(100)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires numeric range [2, 5], got 100"###.to_string() }) },
    OutputCase { name: r###"independent-string-assertions-short"###, schema: descriptor(r###"independent-string-assertions"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"x"###.to_string())))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` requires string length between 2 and 3 Unicode scalar values, got 1 Unicode scalar values"###.to_string() }) },
    OutputCase { name: r###"independent-string-assertions-pattern"###, schema: descriptor(r###"independent-string-assertions"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"yy"###.to_string())))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` does not match its JSON Schema pattern constraints"###.to_string() }) },
    OutputCase { name: r###"multiple-int3"###, schema: descriptor(r###"independent-multiple-of"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(3)))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`value` is not an exact multiple of its JSON Schema multipleOf constraints"###.to_string() }) },
    OutputCase { name: r###"array-refuse-absence-item"###, schema: descriptor(r###"array"###), instance: Instance::Group(vec![(r###"values"###.to_string(),Instance::Repeated(vec![Instance::Scalar(Value::Null)]))].into()), expected: Err(JsonBoundaryError::InvalidOutput { message: r###"`values`: expected declared scalar union, got null"###.to_string() }) },
    OutputCase { name: r###"string-empty"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###""###.to_string())))].into()), expected: Ok(r###"{
  "value": ""
}
"###) },
    OutputCase { name: r###"string-word"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"abc"###.to_string())))].into()), expected: Ok(r###"{
  "value": "abc"
}
"###) },
    OutputCase { name: r###"string-below"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"1"###.to_string())))].into()), expected: Ok(r###"{
  "value": "1"
}
"###) },
    OutputCase { name: r###"string-above"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"6"###.to_string())))].into()), expected: Ok(r###"{
  "value": "6"
}
"###) },
    OutputCase { name: r###"string-fraction"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"1.0"###.to_string())))].into()), expected: Ok(r###"{
  "value": "1.0"
}
"###) },
    OutputCase { name: r###"string-spaces"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###" 6 "###.to_string())))].into()), expected: Ok(r###"{
  "value": " 6 "
}
"###) },
    OutputCase { name: r###"string-overflow"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"9223372036854775808"###.to_string())))].into()), expected: Ok(r###"{
  "value": "9223372036854775808"
}
"###) },
    OutputCase { name: r###"int-min"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(2)))].into()), expected: Ok(r###"{
  "value": 2
}
"###) },
    OutputCase { name: r###"int-max"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(5)))].into()), expected: Ok(r###"{
  "value": 5
}
"###) },
    OutputCase { name: r###"optional-nullable-null"###, schema: descriptor(r###"optional-nullable"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into()), expected: Ok(r###"{
  "value": null
}
"###) },
    OutputCase { name: r###"required-nullable-null"###, schema: descriptor(r###"required-nullable"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into()), expected: Ok(r###"{
  "value": null
}
"###) },
    OutputCase { name: r###"optional-missing"###, schema: descriptor(r###"optional-nullable"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Null))].into()), expected: Ok(r###"{}
"###) },
    OutputCase { name: r###"wide-9007199254740993"###, schema: descriptor(r###"wide"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9007199254740993)))].into()), expected: Ok(r###"{
  "value": 9007199254740993
}
"###) },
    OutputCase { name: r###"wide-9007199254740995"###, schema: descriptor(r###"wide"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9007199254740995)))].into()), expected: Ok(r###"{
  "value": 9007199254740995
}
"###) },
    OutputCase { name: r###"exact-i64-min"###, schema: descriptor(r###"min-singleton"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(-9223372036854775808)))].into()), expected: Ok(r###"{
  "value": -9223372036854775808
}
"###) },
    OutputCase { name: r###"exact-i64-max"###, schema: descriptor(r###"max-singleton"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9223372036854775807)))].into()), expected: Ok(r###"{
  "value": 9223372036854775807
}
"###) },
    OutputCase { name: r###"lower-only-max"###, schema: descriptor(r###"lower-only"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(9223372036854775807)))].into()), expected: Ok(r###"{
  "value": 9223372036854775807
}
"###) },
    OutputCase { name: r###"upper-only-min"###, schema: descriptor(r###"upper-only"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(-9223372036854775808)))].into()), expected: Ok(r###"{
  "value": -9223372036854775808
}
"###) },
    OutputCase { name: r###"finite-enum-string100"###, schema: descriptor(r###"finite-enum"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"100"###.to_string())))].into()), expected: Ok(r###"{
  "value": "100"
}
"###) },
    OutputCase { name: r###"finite-composition-filter-string100"###, schema: descriptor(r###"finite-composition-filter"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"100"###.to_string())))].into()), expected: Ok(r###"{
  "value": "100"
}
"###) },
    OutputCase { name: r###"finite-composition-filter-null"###, schema: descriptor(r###"finite-composition-filter"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::json_null()))].into()), expected: Ok(r###"{
  "value": null
}
"###) },
    OutputCase { name: r###"finite-extra-int"###, schema: descriptor(r###"finite-composition-filter"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(3)))].into()), expected: Ok(r###"{
  "value": 3
}
"###) },
    OutputCase { name: r###"old-unbounded-int"###, schema: descriptor(r###"old-unconstrained"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(1)))].into()), expected: Ok(r###"{
  "value": 1
}
"###) },
    OutputCase { name: r###"old-text-tag"###, schema: descriptor(r###"old-unconstrained"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"1"###.to_string())))].into()), expected: Ok(r###"{
  "value": "1"
}
"###) },
    OutputCase { name: r###"array-items"###, schema: descriptor(r###"array"###), instance: Instance::Group(vec![(r###"values"###.to_string(),Instance::Repeated(vec![Instance::Scalar(Value::Int(2)),Instance::Scalar(Value::String(r###"6"###.to_string())),Instance::Scalar(Value::json_null()),Instance::Scalar(Value::Int(5))]))].into()), expected: Ok(r###"{
  "values": [
    2,
    "6",
    null,
    5
  ]
}
"###) },
    OutputCase { name: r###"array-wrapper-null"###, schema: descriptor(r###"array"###), instance: Instance::Group(vec![(r###"values"###.to_string(),Instance::Scalar(Value::json_null()))].into()), expected: Ok(r###"{
  "values": null
}
"###) },
    OutputCase { name: r###"array-empty"###, schema: descriptor(r###"array"###), instance: Instance::Group(vec![(r###"values"###.to_string(),Instance::Repeated(vec![]))].into()), expected: Ok(r###"{
  "values": []
}
"###) },
    OutputCase { name: r###"independent-string-assertions-string"###, schema: descriptor(r###"independent-string-assertions"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"xx"###.to_string())))].into()), expected: Ok(r###"{
  "value": "xx"
}
"###) },
    OutputCase { name: r###"independent-string-assertions-int"###, schema: descriptor(r###"independent-string-assertions"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(2)))].into()), expected: Ok(r###"{
  "value": 2
}
"###) },
    OutputCase { name: r###"multiple-string3"###, schema: descriptor(r###"independent-multiple-of"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::String(r###"3"###.to_string())))].into()), expected: Ok(r###"{
  "value": "3"
}
"###) },
    OutputCase { name: r###"multiple-int2"###, schema: descriptor(r###"independent-multiple-of"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(2)))].into()), expected: Ok(r###"{
  "value": 2
}
"###) },
    OutputCase { name: r###"late-good-control"###, schema: descriptor(r###"bounded"###), instance: Instance::Group(vec![(r###"value"###.to_string(),Instance::Scalar(Value::Int(3)))].into()), expected: Ok(r###"{
  "value": 3
}
"###) },
    OutputCase { name: "old-root-string", schema: r###"{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}"###, instance: Instance::Scalar(Value::String("1".to_string())), expected: Ok("\"1\"\n") },
    OutputCase { name: "root-string100", schema: r###"{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}"###, instance: Instance::Scalar(Value::String("100".to_string())), expected: Ok("\"100\"\n") },
    OutputCase { name: "old-int-monotype-coercion", schema: r###"{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar","ty":"int"}}"###, instance: Instance::Scalar(Value::String("3".to_string())), expected: Ok("3\n") },
    ];
    let input_results: Vec<_> = inputs
        .iter()
        .map(|case| {
            (
                parse_json(case.schema, case.document),
                parse_json_bytes(case.schema, case.document.as_bytes()),
            )
        })
        .collect();
    let output_results: Vec<_> = outputs
        .iter()
        .map(|case| {
            (
                serialize_json(case.schema, &case.instance),
                serialize_json_bytes(case.schema, &case.instance),
            )
        })
        .collect();
    let input_originals: Vec<_> = inputs.iter().zip(&input_results).map(|(case,(text,bytes))| serde_json::json!({
        "case":case.name,"schema_text":case.schema,"input":case.document,"utf8":case.document.as_bytes(),"expected_debug":format!("{:#?}",case.expected),
        "parse_json":original(text,instance_original),"parse_json_bytes":original(bytes,instance_original),
    })).collect();
    let output_originals: Vec<_> = outputs.iter().zip(&output_results).map(|(case,(text,bytes))| serde_json::json!({
        "case":case.name,"schema_text":case.schema,"input_instance":instance_original(&case.instance),"expected_debug":format!("{:#?}",case.expected),
        "serialize_json":original(text,|text| serde_json::json!({"text":text,"utf8":text.as_bytes()})),
        "serialize_json_bytes":original(bytes,|bytes| serde_json::json!({"utf8":bytes})),
    })).collect();
    // Keep and publish the complete cohort before any semantic comparison.
    eprintln!(
        "STRING_INT101_RUST_DIRECT_ORIGINALS {}",
        serde_json::json!({"inputs":input_originals,"outputs":output_originals})
    );
    for (case, (text, bytes)) in inputs.iter().zip(&input_results) {
        assert_eq!(text, &case.expected, "{} text", case.name);
        assert_eq!(bytes, &case.expected, "{} bytes", case.name);
        for error in [text.as_ref().err(), bytes.as_ref().err()]
            .into_iter()
            .flatten()
        {
            assert!(error.source().is_none(), "{}: {error:#?}", case.name);
        }
    }
    for (case, (text, bytes)) in outputs.iter().zip(&output_results) {
        assert_eq!(
            text.as_deref(),
            case.expected.as_ref().map(|value| *value),
            "{} text",
            case.name
        );
        assert_eq!(
            bytes.as_ref().map(Vec::as_slice),
            case.expected.as_ref().map(|value| value.as_bytes()),
            "{} bytes",
            case.name
        );
        for error in [text.as_ref().err(), bytes.as_ref().err()]
            .into_iter()
            .flatten()
        {
            assert!(error.source().is_none(), "{}: {error:#?}", case.name);
        }
    }
}

#[test]
fn old_unconstrained_integer_union_descriptor_remains_byte_exact() -> Result<(), Box<dyn Error>> {
    const OLD: &str = r###"{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}"###;
    let decoded = codegen_schema::decode(OLD, super::MAX_EMBEDDED_JSON_SCHEMA_BYTES);
    let reencoded = decoded.as_ref().ok().map(serde_json::to_string);
    eprintln!(
        "STRING_INT101_RUST_OLD_SERDE_ORIGINALS {}",
        serde_json::json!({"input":OLD,"decoded_debug":format!("{decoded:#?}"),"reencoded_debug":format!("{reencoded:#?}"),"expected":OLD})
    );
    let decoded: SchemaNode = decoded?;
    assert!(decoded.numeric_range.is_none());
    assert_eq!(serde_json::to_string(&decoded)?, OLD);
    Ok(())
}
