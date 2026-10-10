use std::any::Any;
use std::error::Error;
use std::fmt::Debug;

use format_json::{JsonFormatError, from_str, json_schema, to_string, to_value};
use ir::{Instance, SchemaNode, Value};

// Complete literal schemas and metadata come from independently authored
// interval/tag/presence oracles, not from an application or runtime output.
const PROFILES: &[(&str, &str, &str, &str)] = &[
    (
        r###"bounded"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2,"maximum":5}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2,"maximum":5}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"optional-nullable"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5}},"required":[],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5}},"additionalProperties":false}"###,
    ),
    (
        r###"required-nullable"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"wide"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":9007199254740993,"maximum":9007199254740995}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9007199254740993,"maximum":9007199254740995}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":9007199254740993,"maximum":9007199254740995}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"min-singleton"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":-9223372036854775808,"maximum":-9223372036854775808}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":-9223372036854775808,"maximum":-9223372036854775808}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":-9223372036854775808,"maximum":-9223372036854775808}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"max-singleton"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":9223372036854775807,"maximum":9223372036854775807}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9223372036854775807,"maximum":9223372036854775807}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":9223372036854775807,"maximum":9223372036854775807}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"lower-only"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"upper-only"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"maximum":5}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"maximum":5}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"anyOf-nullable"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"anyOf":[{"type":"string"},{"type":"integer","minimum":2,"maximum":5},{"type":"null"}]}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"oneOf-nullable"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"oneOf":[{"type":"string"},{"type":"integer","minimum":2,"maximum":5},{"type":"null"}]}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"finite-enum"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5,"enum":["1","6","100",2,6,100,null]}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"json_allowed_values":[{"type":"json_null"},{"type":"int","value":2},{"type":"int","value":6},{"type":"int","value":100},{"type":"string","value":"1"},{"type":"string","value":"100"},{"type":"string","value":"6"}]}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"minimum":2,"maximum":5,"enum":[null,2,6,100,"1","100","6"]}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"finite-composition-filter"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"anyOf":[{"type":["string","integer","null"],"minimum":2,"maximum":5,"enum":["1","6","100",2,6,100,null]},{"type":"integer","enum":[3]}]}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"kind":{"kind":"scalar_union","types":["string","int"]},"json_allowed_values":[{"type":"json_null"},{"type":"int","value":2},{"type":"int","value":3},{"type":"string","value":"1"},{"type":"string","value":"100"},{"type":"string","value":"6"}]}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","null"],"enum":[null,2,3,"1","100","6"]}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"array"###,
        r###"{"title":"Record","type":"object","properties":{"values":{"type":["array","null"],"items":{"type":["string","integer","null"],"minimum":2,"maximum":5}}},"required":["values"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"values","repeating":true,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"container_nullable":true}],"required":["values"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"values":{"anyOf":[{"type":"array","items":{"type":["string","integer","null"],"minimum":2,"maximum":5}},{"type":"null"}]}},"required":["values"],"additionalProperties":false}"###,
    ),
    (
        r###"old-unconstrained"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"]}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"]}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"independent-string-assertions"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2,"maximum":5,"minLength":2,"maxLength":3,"pattern":"^x+$","format":"date"}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"string_length_range":{"minimum":2,"maximum":3},"json_patterns":{"any_of":[["^x+$"]]},"json_formats":["date"]}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2,"maximum":5,"minLength":2,"maxLength":3,"pattern":"^x+$","format":"date"}},"required":["value"],"additionalProperties":false}"###,
    ),
    (
        r###"independent-multiple-of"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2,"maximum":8,"multipleOf":2}},"required":["value"],"additionalProperties":false}"###,
        r###"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":8}},"kind":{"kind":"scalar_union","types":["string","int"]},"json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]}}],"required":["value"]}}"###,
        r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2,"maximum":8,"multipleOf":2}},"required":["value"],"additionalProperties":false}"###,
    ),
];

fn error_fields(error: &JsonFormatError) -> serde_json::Value {
    use serde_json::json;
    let mut fields = match error {
        JsonFormatError::RangeMismatch { name, range, got } => {
            json!({"variant":"RangeMismatch","name":name,"range":range,"got":got})
        }
        JsonFormatError::Shape {
            name,
            expected,
            got,
        } => json!({"variant":"Shape","name":name,"expected":expected,"got":got}),
        JsonFormatError::MissingRequiredProperty { object, property } => {
            json!({"variant":"MissingRequiredProperty","object":object,"property":property})
        }
        JsonFormatError::StringLengthMismatch { name, range, got } => {
            json!({"variant":"StringLengthMismatch","name":name,"range":range,"got":got})
        }
        JsonFormatError::PatternMismatch { name } => {
            json!({"variant":"PatternMismatch","name":name})
        }
        JsonFormatError::AllowedValueMismatch { name, got } => {
            json!({"variant":"AllowedValueMismatch","name":name,"got":got})
        }
        JsonFormatError::MultipleOfMismatch {
            name,
            divisors,
            got,
        } => json!({"variant":"MultipleOfMismatch","name":name,"divisors":divisors,"got":got}),
        JsonFormatError::UnsupportedSchemaUnion { name, reason } => {
            json!({"variant":"UnsupportedSchemaUnion","name":name,"reason":reason})
        }
        _ => json!({"variant":"unexpected","debug":format!("{error:#?}")}),
    };
    fields["display"] = error.to_string().into();
    fields
}

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

fn original<T: Debug + serde::Serialize + 'static>(
    result: &Result<T, JsonFormatError>,
) -> serde_json::Value {
    match result {
        Ok(value) => {
            serde_json::json!({"status":"ok","value":value,"instance":(value as &dyn Any).downcast_ref::<Instance>().map(instance_original),"debug":format!("{value:#?}")})
        }
        Err(error) => {
            let mut sources = Vec::new();
            let mut next = error.source();
            while let Some(source) = next {
                sources.push(serde_json::json!({"display":source.to_string(),"debug":format!("{source:#?}")}));
                next = source.source();
            }
            serde_json::json!({"status":"error","typed":error_fields(error),"debug":format!("{error:#?}"),"sources":sources})
        }
    }
}

#[test]
fn complete_integer_union_metadata_exports_and_reimports() -> Result<(), Box<dyn Error>> {
    let actuals: Vec<_> = PROFILES
        .iter()
        .map(|(_, text, _, _)| {
            let imported = json_schema::import_str(text);
            let exported = imported.as_ref().ok().map(json_schema::export);
            let reimported = exported
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(|text| json_schema::import_str(text));
            (imported, exported, reimported)
        })
        .collect();
    let originals: Vec<_> = PROFILES.iter().zip(&actuals).map(|((name, text, metadata, export), (imported, exported, reimported))| serde_json::json!({
        "case":name,"source_schema_text":text,"expected_complete_metadata":metadata,"expected_complete_export":export,
        "import":original(imported),"export":exported.as_ref().map(original),"reimport":reimported.as_ref().map(original),
    })).collect();
    eprintln!(
        "STRING_INT101_NATIVE_ROUNDTRIP_ORIGINALS {}",
        serde_json::json!({"cases":originals})
    );
    for ((name, _, metadata, export), (imported, exported, reimported)) in
        PROFILES.iter().zip(&actuals)
    {
        let imported = imported
            .as_ref()
            .map_err(|error| format!("{name}: {error:#?}"))?;
        assert_eq!(
            serde_json::to_value(imported)?,
            serde_json::from_str::<serde_json::Value>(metadata)?,
            "{name}"
        );
        let exported = exported
            .as_ref()
            .ok_or("export was reached")?
            .as_ref()
            .map_err(|error| error.to_string())?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(exported)?,
            serde_json::from_str::<serde_json::Value>(export)?,
            "{name}"
        );
        assert_eq!(
            reimported.as_ref().and_then(|result| result.as_ref().ok()),
            Some(imported),
            "{name}: {reimported:#?}"
        );
    }
    Ok(())
}

struct Success {
    name: &'static str,
    profile: &'static str,
    document: &'static str,
    instance: Instance,
    output: &'static str,
}

struct Negative {
    name: &'static str,
    profile: &'static str,
    document: Option<&'static str>,
    instance: Option<Instance>,
    error: &'static str,
}

#[test]
fn native_integer_union_input_and_normalized_output_keep_tags() -> Result<(), Box<dyn Error>> {
    let successes = [
        Success {
            name: r###"string-empty"###,
            profile: r###"bounded"###,
            document: r###"{"value":""}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###""###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": ""
}
"###,
        },
        Success {
            name: r###"string-word"###,
            profile: r###"bounded"###,
            document: r###"{"value":"abc"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"abc"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "abc"
}
"###,
        },
        Success {
            name: r###"string-below"###,
            profile: r###"bounded"###,
            document: r###"{"value":"1"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"1"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "1"
}
"###,
        },
        Success {
            name: r###"string-above"###,
            profile: r###"bounded"###,
            document: r###"{"value":"6"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"6"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "6"
}
"###,
        },
        Success {
            name: r###"string-fraction"###,
            profile: r###"bounded"###,
            document: r###"{"value":"1.0"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"1.0"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "1.0"
}
"###,
        },
        Success {
            name: r###"string-spaces"###,
            profile: r###"bounded"###,
            document: r###"{"value":" 6 "}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###" 6 "###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": " 6 "
}
"###,
        },
        Success {
            name: r###"string-overflow"###,
            profile: r###"bounded"###,
            document: r###"{"value":"9223372036854775808"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"9223372036854775808"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "9223372036854775808"
}
"###,
        },
        Success {
            name: r###"int-min"###,
            profile: r###"bounded"###,
            document: r###"{"value":2}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(2)))].into(),
            ),
            output: r###"{
  "value": 2
}
"###,
        },
        Success {
            name: r###"int-max"###,
            profile: r###"bounded"###,
            document: r###"{"value":5}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(5)))].into(),
            ),
            output: r###"{
  "value": 5
}
"###,
        },
        Success {
            name: r###"optional-nullable-null"###,
            profile: r###"optional-nullable"###,
            document: r###"{"value":null}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::json_null()),
                )]
                .into(),
            ),
            output: r###"{
  "value": null
}
"###,
        },
        Success {
            name: r###"required-nullable-null"###,
            profile: r###"required-nullable"###,
            document: r###"{"value":null}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::json_null()),
                )]
                .into(),
            ),
            output: r###"{
  "value": null
}
"###,
        },
        Success {
            name: r###"optional-missing"###,
            profile: r###"optional-nullable"###,
            document: r###"{}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Null))].into(),
            ),
            output: r###"{}
"###,
        },
        Success {
            name: r###"wide-9007199254740993"###,
            profile: r###"wide"###,
            document: r###"{"value":9007199254740993}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9007199254740993)),
                )]
                .into(),
            ),
            output: r###"{
  "value": 9007199254740993
}
"###,
        },
        Success {
            name: r###"wide-9007199254740995"###,
            profile: r###"wide"###,
            document: r###"{"value":9007199254740995}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9007199254740995)),
                )]
                .into(),
            ),
            output: r###"{
  "value": 9007199254740995
}
"###,
        },
        Success {
            name: r###"exact-i64-min"###,
            profile: r###"min-singleton"###,
            document: r###"{"value":-9223372036854775808}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(-9223372036854775808)),
                )]
                .into(),
            ),
            output: r###"{
  "value": -9223372036854775808
}
"###,
        },
        Success {
            name: r###"exact-i64-max"###,
            profile: r###"max-singleton"###,
            document: r###"{"value":9223372036854775807}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9223372036854775807)),
                )]
                .into(),
            ),
            output: r###"{
  "value": 9223372036854775807
}
"###,
        },
        Success {
            name: r###"lower-only-max"###,
            profile: r###"lower-only"###,
            document: r###"{"value":9223372036854775807}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9223372036854775807)),
                )]
                .into(),
            ),
            output: r###"{
  "value": 9223372036854775807
}
"###,
        },
        Success {
            name: r###"upper-only-min"###,
            profile: r###"upper-only"###,
            document: r###"{"value":-9223372036854775808}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(-9223372036854775808)),
                )]
                .into(),
            ),
            output: r###"{
  "value": -9223372036854775808
}
"###,
        },
        Success {
            name: r###"finite-enum-string100"###,
            profile: r###"finite-enum"###,
            document: r###"{"value":"100"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"100"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "100"
}
"###,
        },
        Success {
            name: r###"finite-composition-filter-string100"###,
            profile: r###"finite-composition-filter"###,
            document: r###"{"value":"100"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"100"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "100"
}
"###,
        },
        Success {
            name: r###"finite-composition-filter-null"###,
            profile: r###"finite-composition-filter"###,
            document: r###"{"value":null}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::json_null()),
                )]
                .into(),
            ),
            output: r###"{
  "value": null
}
"###,
        },
        Success {
            name: r###"finite-extra-int"###,
            profile: r###"finite-composition-filter"###,
            document: r###"{"value":3}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(3)))].into(),
            ),
            output: r###"{
  "value": 3
}
"###,
        },
        Success {
            name: r###"old-unbounded-int"###,
            profile: r###"old-unconstrained"###,
            document: r###"{"value":1}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(1)))].into(),
            ),
            output: r###"{
  "value": 1
}
"###,
        },
        Success {
            name: r###"old-text-tag"###,
            profile: r###"old-unconstrained"###,
            document: r###"{"value":"1"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"1"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "1"
}
"###,
        },
        Success {
            name: r###"array-items"###,
            profile: r###"array"###,
            document: r###"{"values":[2,"6",null,5]}"###,
            instance: Instance::Group(
                vec![(
                    r###"values"###.to_string(),
                    Instance::Repeated(vec![
                        Instance::Scalar(Value::Int(2)),
                        Instance::Scalar(Value::String(r###"6"###.to_string())),
                        Instance::Scalar(Value::json_null()),
                        Instance::Scalar(Value::Int(5)),
                    ]),
                )]
                .into(),
            ),
            output: r###"{
  "values": [
    2,
    "6",
    null,
    5
  ]
}
"###,
        },
        Success {
            name: r###"array-wrapper-null"###,
            profile: r###"array"###,
            document: r###"{"values":null}"###,
            instance: Instance::Group(
                vec![(
                    r###"values"###.to_string(),
                    Instance::Scalar(Value::json_null()),
                )]
                .into(),
            ),
            output: r###"{
  "values": null
}
"###,
        },
        Success {
            name: r###"array-empty"###,
            profile: r###"array"###,
            document: r###"{"values":[]}"###,
            instance: Instance::Group(
                vec![(r###"values"###.to_string(), Instance::Repeated(vec![]))].into(),
            ),
            output: r###"{
  "values": []
}
"###,
        },
        Success {
            name: r###"independent-string-assertions-string"###,
            profile: r###"independent-string-assertions"###,
            document: r###"{"value":"xx"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"xx"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "xx"
}
"###,
        },
        Success {
            name: r###"independent-string-assertions-int"###,
            profile: r###"independent-string-assertions"###,
            document: r###"{"value":2}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(2)))].into(),
            ),
            output: r###"{
  "value": 2
}
"###,
        },
        Success {
            name: r###"multiple-string3"###,
            profile: r###"independent-multiple-of"###,
            document: r###"{"value":"3"}"###,
            instance: Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"3"###.to_string())),
                )]
                .into(),
            ),
            output: r###"{
  "value": "3"
}
"###,
        },
        Success {
            name: r###"multiple-int2"###,
            profile: r###"independent-multiple-of"###,
            document: r###"{"value":2}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(2)))].into(),
            ),
            output: r###"{
  "value": 2
}
"###,
        },
        Success {
            name: r###"late-good-control"###,
            profile: r###"bounded"###,
            document: r###"{"value":3}"###,
            instance: Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(3)))].into(),
            ),
            output: r###"{
  "value": 3
}
"###,
        },
    ];
    let negatives = [
        Negative {
            name: r###"bounded-refuse-1"###,
            profile: r###"bounded"###,
            document: Some(r###"{"value":1}"###),
            instance: Some(Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(1)))].into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[2, 5]","got":"1","display":"`value` requires numeric range [2, 5], got 1"}"###,
        },
        Negative {
            name: r###"bounded-refuse-6"###,
            profile: r###"bounded"###,
            document: Some(r###"{"value":6}"###),
            instance: Some(Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(6)))].into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[2, 5]","got":"6","display":"`value` requires numeric range [2, 5], got 6"}"###,
        },
        Negative {
            name: r###"wide-refuse-9007199254740992"###,
            profile: r###"wide"###,
            document: Some(r###"{"value":9007199254740992}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9007199254740992)),
                )]
                .into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[9007199254740993, 9007199254740995]","got":"9007199254740992","display":"`value` requires numeric range [9007199254740993, 9007199254740995], got 9007199254740992"}"###,
        },
        Negative {
            name: r###"wide-refuse-9007199254740996"###,
            profile: r###"wide"###,
            document: Some(r###"{"value":9007199254740996}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9007199254740996)),
                )]
                .into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[9007199254740993, 9007199254740995]","got":"9007199254740996","display":"`value` requires numeric range [9007199254740993, 9007199254740995], got 9007199254740996"}"###,
        },
        Negative {
            name: r###"min-neighbor"###,
            profile: r###"min-singleton"###,
            document: Some(r###"{"value":-9223372036854775807}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(-9223372036854775807)),
                )]
                .into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[-9223372036854775808, -9223372036854775808]","got":"-9223372036854775807","display":"`value` requires numeric range [-9223372036854775808, -9223372036854775808], got -9223372036854775807"}"###,
        },
        Negative {
            name: r###"max-neighbor"###,
            profile: r###"max-singleton"###,
            document: Some(r###"{"value":9223372036854775806}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(9223372036854775806)),
                )]
                .into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[9223372036854775807, 9223372036854775807]","got":"9223372036854775806","display":"`value` requires numeric range [9223372036854775807, 9223372036854775807], got 9223372036854775806"}"###,
        },
        Negative {
            name: r###"array-refuse-int"###,
            profile: r###"array"###,
            document: Some(r###"{"values":[6]}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"values"###.to_string(),
                    Instance::Repeated(vec![Instance::Scalar(Value::Int(6))]),
                )]
                .into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"values","range":"[2, 5]","got":"6","display":"`values` requires numeric range [2, 5], got 6"}"###,
        },
        Negative {
            name: r###"input-shape-fraction-lexeme"###,
            profile: r###"bounded"###,
            document: Some(r###"{"value":1.0}"###),
            instance: None,
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"number","display":"`value`: expected declared scalar union, got number"}"###,
        },
        Negative {
            name: r###"input-shape-exponent-lexeme"###,
            profile: r###"bounded"###,
            document: Some(r###"{"value":2e0}"###),
            instance: None,
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"number","display":"`value`: expected declared scalar union, got number"}"###,
        },
        Negative {
            name: r###"input-shape-unsigned-overflow"###,
            profile: r###"bounded"###,
            document: Some(r###"{"value":9223372036854775808}"###),
            instance: None,
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"number","display":"`value`: expected declared scalar union, got number"}"###,
        },
        Negative {
            name: r###"input-shape-null"###,
            profile: r###"bounded"###,
            document: Some(r###"{"value":null}"###),
            instance: None,
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"null","display":"`value`: expected declared scalar union, got null"}"###,
        },
        Negative {
            name: r###"output-shape-float"###,
            profile: r###"bounded"###,
            document: None,
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Float(1.0)),
                )]
                .into(),
            )),
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"float","display":"`value`: expected declared scalar union, got float"}"###,
        },
        Negative {
            name: r###"output-shape-null"###,
            profile: r###"bounded"###,
            document: None,
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::json_null()),
                )]
                .into(),
            )),
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"json null","display":"`value`: expected declared scalar union, got json null"}"###,
        },
        Negative {
            name: r###"output-shape-xml-nil"###,
            profile: r###"bounded"###,
            document: None,
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::xml_nil()),
                )]
                .into(),
            )),
            error: r###"{"variant":"Shape","name":"value","expected":"declared scalar union","got":"xml nil","display":"`value`: expected declared scalar union, got xml nil"}"###,
        },
        Negative {
            name: r###"required-missing"###,
            profile: r###"required-nullable"###,
            document: Some(r###"{}"###),
            instance: Some(Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Null))].into(),
            )),
            error: r###"{"variant":"MissingRequiredProperty","object":"Record","property":"value","display":"object `Record` requires property `value`"}"###,
        },
        Negative {
            name: r###"enum-int100-bounded"###,
            profile: r###"finite-enum"###,
            document: Some(r###"{"value":100}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::Int(100)),
                )]
                .into(),
            )),
            error: r###"{"variant":"RangeMismatch","name":"value","range":"[2, 5]","got":"100","display":"`value` requires numeric range [2, 5], got 100"}"###,
        },
        Negative {
            name: r###"independent-string-assertions-short"###,
            profile: r###"independent-string-assertions"###,
            document: Some(r###"{"value":"x"}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"x"###.to_string())),
                )]
                .into(),
            )),
            error: r###"{"variant":"StringLengthMismatch","name":"value","range":"between 2 and 3 Unicode scalar values","got":1,"display":"`value` requires string length between 2 and 3 Unicode scalar values, got 1 Unicode scalar values"}"###,
        },
        Negative {
            name: r###"independent-string-assertions-pattern"###,
            profile: r###"independent-string-assertions"###,
            document: Some(r###"{"value":"yy"}"###),
            instance: Some(Instance::Group(
                vec![(
                    r###"value"###.to_string(),
                    Instance::Scalar(Value::String(r###"yy"###.to_string())),
                )]
                .into(),
            )),
            error: r###"{"variant":"PatternMismatch","name":"value","display":"`value` does not match its JSON Schema pattern constraints"}"###,
        },
        Negative {
            name: r###"multiple-int3"###,
            profile: r###"independent-multiple-of"###,
            document: Some(r###"{"value":3}"###),
            instance: Some(Instance::Group(
                vec![(r###"value"###.to_string(), Instance::Scalar(Value::Int(3)))].into(),
            )),
            error: r###"{"variant":"MultipleOfMismatch","name":"value","divisors":"2","got":"3","display":"`value` requires a value divisible by 2, got 3"}"###,
        },
        Negative {
            name: r###"array-refuse-absence-item"###,
            profile: r###"array"###,
            document: None,
            instance: Some(Instance::Group(
                vec![(
                    r###"values"###.to_string(),
                    Instance::Repeated(vec![Instance::Scalar(Value::Null)]),
                )]
                .into(),
            )),
            error: r###"{"variant":"Shape","name":"values","expected":"declared scalar union","got":"null","display":"`values`: expected declared scalar union, got null"}"###,
        },
        Negative {
            name: "finite-filter-int100",
            profile: "finite-composition-filter",
            document: Some(r#"{"value":100}"#),
            instance: Some(Instance::Group(
                vec![("value".to_string(), Instance::Scalar(Value::Int(100)))].into(),
            )),
            error: r#"{"variant":"AllowedValueMismatch","name":"value","got":"Int(100)","display":"`value` is not one of its allowed JSON values, got Int(100)"}"#,
        },
    ];
    // Decode complete independently authored metadata; importing is qualified
    // separately above, so no imported output becomes a boundary oracle.
    let decoded: Vec<Result<SchemaNode, serde_json::Error>> = PROFILES
        .iter()
        .map(|entry| serde_json::from_str(entry.2))
        .collect();
    let setup_originals: Vec<_> = PROFILES.iter().zip(&decoded).map(|(profile,result)| serde_json::json!({
        "profile":profile.0,"schema_text":profile.2,"actual_debug":format!("{result:#?}"),
        "actual_schema":result.as_ref().ok(),"error":result.as_ref().err().map(|error| serde_json::json!({"display":error.to_string(),"debug":format!("{error:#?}"),"category":format!("{:?}",error.classify()),"line":error.line(),"column":error.column()})),
    })).collect();
    eprintln!(
        "STRING_INT101_NATIVE_LITERAL_METADATA_SETUP_ORIGINALS {}",
        serde_json::json!({"cases":setup_originals})
    );
    let schemas: Vec<SchemaNode> = decoded.into_iter().collect::<Result<_, _>>()?;
    let schema = |name: &str| {
        &schemas[PROFILES
            .iter()
            .position(|entry| entry.0 == name)
            .expect("declared profile")]
    };
    let negative_actuals: Vec<_> = negatives
        .iter()
        .map(|case| {
            (
                case.document
                    .map(|text| from_str(text, schema(case.profile))),
                case.instance
                    .as_ref()
                    .map(|instance| to_string(schema(case.profile), instance)),
            )
        })
        .collect();
    let success_actuals: Vec<_> = successes
        .iter()
        .map(|case| {
            (
                from_str(case.document, schema(case.profile)),
                to_string(schema(case.profile), &case.instance),
                to_value(schema(case.profile), &case.instance),
            )
        })
        .collect();
    let originals: Vec<_> = negatives.iter().zip(&negative_actuals).map(|(case,(input,output))| serde_json::json!({
        "case":case.name,"schema":schema(case.profile),"input":case.document,"output_input":case.instance,
        "expected_error":case.error,"input_original":input.as_ref().map(original),"output_original":output.as_ref().map(original),
    })).chain(successes.iter().zip(&success_actuals).map(|(case,(input,output,value))| serde_json::json!({
        "case":case.name,"schema":schema(case.profile),"input":case.document,"utf8":case.document.as_bytes(),
        "expected_instance":case.instance,"expected_output":case.output,"input_original":original(input),
        "output_original":original(output),"value_original":original(value),
    }))).collect();
    // Every original, including unexpected success/error and the later valid
    // control, is retained before the first boundary comparison.
    eprintln!(
        "STRING_INT101_NATIVE_BOUNDARY_ORIGINALS {}",
        serde_json::json!({"cases":originals})
    );
    for (case, (input, output)) in negatives.iter().zip(&negative_actuals) {
        let expected: serde_json::Value = serde_json::from_str(case.error)?;
        if let Some(result) = input {
            assert_eq!(
                result.as_ref().err().map(error_fields),
                Some(expected.clone()),
                "{}: {result:#?}",
                case.name
            );
        }
        if let Some(result) = output {
            assert_eq!(
                result.as_ref().err().map(error_fields),
                Some(expected.clone()),
                "{}: {result:#?}",
                case.name
            );
        }
    }
    for (case, (input, output, value)) in successes.iter().zip(&success_actuals) {
        assert_eq!(
            input.as_ref().ok(),
            Some(&case.instance),
            "{}: {input:#?}",
            case.name
        );
        assert_eq!(
            output.as_deref().ok(),
            Some(case.output),
            "{}: {output:#?}",
            case.name
        );
        assert_eq!(
            value.as_ref().ok(),
            Some(&serde_json::from_str::<serde_json::Value>(case.output)?),
            "{}: {value:#?}",
            case.name
        );
    }
    Ok(())
}

#[test]
fn string_float_single_endpoint_keeps_complete_schema_tags_and_range_errors()
-> Result<(), Box<dyn Error>> {
    const INPUT_SCHEMA: &str = r#"{"title":"Record","type":"object","properties":{"value":{"type":["string","number"],"minimum":2}},"required":["value"],"additionalProperties":false}"#;
    const EXPECTED_MODEL: &str = r#"{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"number","bounds":{"minimum":{"value":2.0}}},"kind":{"kind":"scalar_union","types":["string","float"]}}],"required":["value"]}}"#;
    let imported = json_schema::import_str(INPUT_SCHEMA);
    let expected_model = serde_json::from_str::<SchemaNode>(EXPECTED_MODEL);
    eprintln!(
        "STRING_FLOAT_SINGLE_ENDPOINT_MODEL_ORIGINALS {}",
        serde_json::json!({"schema_text":INPUT_SCHEMA,"expected_ir_text":EXPECTED_MODEL,
            "import_original":original(&imported),"expected_model_original":format!("{expected_model:#?}")})
    );
    let expected_model = expected_model?;
    let exported = json_schema::export(&expected_model);
    let reimported = exported
        .as_ref()
        .ok()
        .map(|text| json_schema::import_str(text));
    let group = |value| Instance::Group(vec![("value".into(), Instance::Scalar(value))].into());
    let cases = [
        (
            "boundary",
            r#"{"value":2}"#,
            group(Value::Float(2.0)),
            "{\n  \"value\": 2.0\n}\n",
        ),
        (
            "numeric-looking-string",
            r#"{"value":"1"}"#,
            group(Value::String("1".into())),
            "{\n  \"value\": \"1\"\n}\n",
        ),
        (
            "ordinary-string",
            r#"{"value":"free"}"#,
            group(Value::String("free".into())),
            "{\n  \"value\": \"free\"\n}\n",
        ),
    ];
    let actuals: Vec<_> = cases
        .iter()
        .map(|(_, input, value, _)| {
            (
                from_str(input, &expected_model),
                to_string(&expected_model, value),
            )
        })
        .collect();
    let negative_input = from_str(r#"{"value":1}"#, &expected_model);
    let negative_instance = group(Value::Float(1.0));
    let negative_output = to_string(&expected_model, &negative_instance);
    const INPUT_ERROR: &str = r#"{"variant":"RangeMismatch","name":"value","range":"[2, inf)","got":"1","display":"`value` requires numeric range [2, inf), got 1"}"#;
    const OUTPUT_ERROR: &str = r#"{"variant":"RangeMismatch","name":"value","range":"[2, inf)","got":"1.0","display":"`value` requires numeric range [2, inf), got 1.0"}"#;
    eprintln!(
        "STRING_FLOAT_SINGLE_ENDPOINT_BOUNDARY_ORIGINALS {}",
        serde_json::json!({"expected_schema":expected_model,"expected_export":INPUT_SCHEMA,
            "export_original":original(&exported),"reimport_original":reimported.as_ref().map(original),
            "positive_cases":cases.iter().zip(&actuals).map(|((name,input,expected,output),(read,write))|
                serde_json::json!({"case":name,"input":input,"expected_instance":instance_original(expected),
                    "expected_output":output,"read_original":original(read),"write_original":original(write)})).collect::<Vec<_>>(),
            "negative_input":"{\"value\":1}","negative_output_input":instance_original(&negative_instance),
            "expected_input_error":INPUT_ERROR,"expected_output_error":OUTPUT_ERROR,
            "negative_input_original":original(&negative_input),"negative_output_original":original(&negative_output)})
    );
    assert_eq!(
        imported.as_ref().ok(),
        Some(&expected_model),
        "{imported:#?}"
    );
    assert_eq!(
        reimported.as_ref().and_then(|value| value.as_ref().ok()),
        Some(&expected_model),
        "{reimported:#?}"
    );
    assert_eq!(
        exported
            .as_ref()
            .ok()
            .map(|text| serde_json::from_str::<serde_json::Value>(text))
            .transpose()?,
        Some(serde_json::from_str(INPUT_SCHEMA)?)
    );
    for ((name, _, expected, output), (read, write)) in cases.iter().zip(&actuals) {
        assert_eq!(read.as_ref().ok(), Some(expected), "{name}: {read:#?}");
        assert_eq!(write.as_deref().ok(), Some(*output), "{name}: {write:#?}");
    }
    assert_eq!(
        negative_input.as_ref().err().map(error_fields),
        Some(serde_json::from_str(INPUT_ERROR)?),
        "{negative_input:#?}"
    );
    assert_eq!(
        negative_output.as_ref().err().map(error_fields),
        Some(serde_json::from_str(OUTPUT_ERROR)?),
        "{negative_output:#?}"
    );
    Ok(())
}

#[test]
fn broader_domains_and_correlated_or_duplicate_branches_stay_refused() -> Result<(), Box<dyn Error>>
{
    let cases = [
        (
            r###"three-domains"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer","number"],"minimum":2}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"numeric ranges on general scalar unions are not yet supported","display":"JSON Schema union `value` is not representable: numeric ranges on general scalar unions are not yet supported"}"###,
        ),
        (
            r###"int-bool"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"type":["integer","boolean"],"minimum":2}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"numeric ranges on general scalar unions are not yet supported","display":"JSON Schema union `value` is not representable: numeric ranges on general scalar unions are not yet supported"}"###,
        ),
        (
            r###"empty-interval"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":5,"maximum":2}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"numeric range has no signed 64-bit integer values","display":"JSON Schema union `value` is not representable: numeric range has no signed 64-bit integer values"}"###,
        ),
        (
            r###"integral-float-endpoint"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":2.0}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"floating-point integer bound is not exactly normalizable to i64; use an integer JSON token for integral endpoints","display":"JSON Schema union `value` is not representable: floating-point integer bound is not exactly normalizable to i64; use an integer JSON token for integral endpoints"}"###,
        ),
        (
            r###"malformed-bound"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":"2"}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"`minimum` must be a number","display":"JSON Schema union `value` is not representable: `minimum` must be a number"}"###,
        ),
        (
            r###"out-of-domain-lower"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"type":["string","integer"],"minimum":9223372036854775808}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"lower bound is above the signed 64-bit domain","display":"JSON Schema union `value` is not representable: lower bound is above the signed 64-bit domain"}"###,
        ),
        (
            r###"disjoint-int-branches"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"anyOf":[{"type":"string"},{"type":"integer","minimum":2,"maximum":3},{"type":"integer","minimum":5,"maximum":6}]}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"scalar anyOf numeric ranges cannot be attached to a heterogeneous scalar union","display":"JSON Schema union `value` is not representable: scalar anyOf numeric ranges cannot be attached to a heterogeneous scalar union"}"###,
        ),
        (
            r###"duplicate-int-branches"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"anyOf":[{"type":"string"},{"type":"integer","minimum":2,"maximum":5},{"type":"integer","minimum":2,"maximum":5}]}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"scalar anyOf numeric ranges cannot be attached to a heterogeneous scalar union","display":"JSON Schema union `value` is not representable: scalar anyOf numeric ranges cannot be attached to a heterogeneous scalar union"}"###,
        ),
        (
            r###"correlated-range-divisor"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"anyOf":[{"type":"string"},{"type":"integer","minimum":2,"maximum":5,"multipleOf":2},{"type":"integer","minimum":7,"maximum":9,"multipleOf":3}]}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"scalar anyOf correlates different numeric-range and multipleOf constraints","display":"JSON Schema union `value` is not representable: scalar anyOf correlates different numeric-range and multipleOf constraints"}"###,
        ),
        (
            r###"one-of-overlap"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"oneOf":[{"type":"string"},{"type":"integer","minimum":2,"maximum":5},{"type":"integer","minimum":2,"maximum":5}]}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"scalar oneOf branches overlap and are not mutually exclusive","display":"JSON Schema union `value` is not representable: scalar oneOf branches overlap and are not mutually exclusive"}"###,
        ),
        (
            r###"ranged-union-all-of-merge"###,
            r###"{"title":"Record","type":"object","properties":{"value":{"allOf":[{"type":["string","integer"],"minimum":2,"maximum":5},{"type":["string","integer"]}]}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"numeric range is incompatible with the intersected scalar type","display":"JSON Schema union `value` is not representable: numeric range is incompatible with the intersected scalar type"}"###,
        ),
        (
            "duplicate-string-branches",
            r###"{"title":"Record","type":"object","properties":{"value":{"anyOf":[{"type":"string"},{"type":"string"},{"type":"integer","minimum":2,"maximum":5}]}},"required":["value"],"additionalProperties":false}"###,
            r###"{"variant":"UnsupportedSchemaUnion","name":"value","reason":"scalar anyOf numeric ranges cannot be attached to a heterogeneous scalar union","display":"JSON Schema union `value` is not representable: scalar anyOf numeric ranges cannot be attached to a heterogeneous scalar union"}"###,
        ),
    ];
    let results = cases.map(|(_, text, _)| json_schema::import_str(text));
    let originals: Vec<_> = cases.iter().zip(&results).map(|((name,text,expected),result)| serde_json::json!({"case":name,"schema_text":text,"expected":expected,"actual":original(result)})).collect();
    eprintln!(
        "STRING_INT101_NATIVE_IMPORT_REFUSAL_ORIGINALS {}",
        serde_json::json!({"cases":originals})
    );
    for ((name, _, expected), result) in cases.iter().zip(&results) {
        assert_eq!(
            result.as_ref().err().map(error_fields),
            Some(serde_json::from_str::<serde_json::Value>(expected)?),
            "{name}: {result:#?}"
        );
    }
    Ok(())
}

#[test]
fn existing_concrete_integer_all_of_and_output_coercion_remain_compatible()
-> Result<(), Box<dyn Error>> {
    let text = r#"{"title":"value","allOf":[{"type":"integer","minimum":2},{"type":"integer","maximum":5}]}"#;
    let imported = json_schema::import_str(text);
    let instance = Instance::Scalar(Value::String("3".to_string()));
    let output = imported
        .as_ref()
        .ok()
        .map(|schema| to_string(schema, &instance));
    eprintln!(
        "STRING_INT101_NATIVE_OLD_MONOTYPE_ORIGINALS {}",
        serde_json::json!({"schema_text":text,"instance":instance,"import":original(&imported),"output":output.as_ref().map(original),"expected_output":"3\n"})
    );
    let expected = serde_json::json!({"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar","ty":"int"}});
    assert_eq!(
        imported
            .as_ref()
            .ok()
            .map(serde_json::to_value)
            .transpose()?,
        Some(expected)
    );
    assert_eq!(
        output
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(String::as_str),
        Some("3\n")
    );
    Ok(())
}
