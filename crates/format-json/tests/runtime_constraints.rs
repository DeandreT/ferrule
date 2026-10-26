//! Fixed behavior controls for moving runtime checks independently of schema compilation.

use format_json::{JsonFormatError, PreparedSchema, from_str, json_schema, to_string, to_value};
use ir::{Instance, Value};

fn error_kind(error: &JsonFormatError) -> &'static str {
    match error {
        JsonFormatError::Json(_) => "syntax",
        JsonFormatError::Shape { .. } => "shape",
        JsonFormatError::ConstantMismatch { .. } => "constant",
        JsonFormatError::AllowedValueMismatch { .. } => "enum",
        JsonFormatError::RangeMismatch { .. } => "range",
        JsonFormatError::MultipleOfMismatch { .. } => "multiple",
        JsonFormatError::StringLengthMismatch { .. } => "length",
        JsonFormatError::PatternMismatch { .. } => "pattern",
        JsonFormatError::ItemCountMismatch { .. } => "count",
        JsonFormatError::UniqueItemsMismatch { .. } => "unique",
        JsonFormatError::ContainsCountMismatch { .. } => "contains",
        _ => "other",
    }
}

#[test]
fn prepared_and_unprepared_keep_exact_values_and_first_failures() {
    // Every rejection has an independently selected first error, not merely
    // agreement between two callers of the same implementation.
    let cases = [
        (
            r#"{"type":"integer","const":9007199254740993}"#,
            "9007199254740993",
            None,
        ),
        (
            r#"{"type":"integer","const":9007199254740993}"#,
            "9007199254740992",
            Some("constant"),
        ),
        (
            r#"{"type":"number","minimum":9007199254740992}"#,
            "9007199254740993",
            Some("shape"),
        ),
        (r#"{"type":"number","multipleOf":0.1}"#, "0.3", None),
        (
            r#"{"type":"number","multipleOf":0.1}"#,
            "0.30000000000000004",
            Some("multiple"),
        ),
        (
            r#"{"type":"integer","minimum":5,"multipleOf":2}"#,
            "3",
            Some("range"),
        ),
        (
            r#"{"type":"string","minLength":2,"pattern":"^z"}"#,
            r#""a""#,
            Some("length"),
        ),
        (
            r#"{"type":"string","minLength":2,"pattern":"^z"}"#,
            r#""ab""#,
            Some("pattern"),
        ),
        (
            r#"{"type":"array","items":{"type":"integer"},"minItems":3,"uniqueItems":true}"#,
            "[1,1]",
            Some("unique"),
        ),
        (
            r#"{"type":"array","items":{"type":"integer"},"minItems":3}"#,
            r#"["bad"]"#,
            Some("count"),
        ),
        (
            r#"{"type":"array","items":{"type":"integer"},"contains":{"minimum":5,"type":"integer"}}"#,
            "[1,2]",
            Some("contains"),
        ),
        (
            r#"{"type":"array","items":{"type":"integer"},"contains":{"minimum":5,"type":"integer"}}"#,
            "[1,5]",
            None,
        ),
        (r#"{"type":"integer","minimum":5}"#, "{", Some("syntax")),
    ];
    for (schema, input, expected_error) in cases {
        let schema = json_schema::import_str(schema).unwrap();
        let prepared = PreparedSchema::new(schema.clone());
        let result = from_str(input, &schema);
        assert_eq!(
            result.as_ref().err().map(error_kind),
            expected_error,
            "{input}"
        );
        for _ in 0..2 {
            assert_eq!(
                prepared
                    .from_str(input)
                    .map_err(|error| format!("{error:?}")),
                from_str(input, &schema).map_err(|error| format!("{error:?}")),
                "{input}"
            );
        }
        if let Ok(instance) = result {
            let output = to_value(&schema, &instance).unwrap();
            assert_eq!(prepared.to_value(&instance).unwrap(), output);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&to_string(&schema, &instance).unwrap())
                    .unwrap(),
                output
            );
        }
    }
}

#[test]
fn exact_raw_uniqueness_keeps_duplicate_keys_and_numeric_lexemes() {
    let schema =
        json_schema::import_str(r#"{"type":"array","uniqueItems":true,"items":{}}"#).unwrap();
    let prepared = PreparedSchema::new(schema.clone());
    for (input, duplicate) in [
        ("[1,1.0]", true),
        ("[-0.0,0]", true),
        ("[9007199254740992,9007199254740993]", false),
        ("[0.1,0.10000000000000001]", false),
        (r#"[{"x":0,"x":1},{"x":1}]"#, true),
        (r#"[{"x":1,"y":2},{"y":2,"x":1}]"#, true),
        ("[[1,2],[2,1]]", false),
    ] {
        let expected = duplicate.then_some("unique");
        assert_eq!(
            from_str(input, &schema).as_ref().err().map(error_kind),
            expected,
            "{input}"
        );
        assert_eq!(
            prepared.from_str(input).as_ref().err().map(error_kind),
            expected,
            "{input}"
        );
        // The original public validator paths remain usable by downstreams.
        assert_eq!(
            json_schema::unique_items::validate_raw_json_unique_items(&schema, input).is_err(),
            duplicate
        );
    }
}

#[test]
fn legacy_unique_items_paths_reexport_the_runtime_api() {
    use format_json::runtime_constraints;

    let values = serde_json::from_str::<Vec<serde_json::Value>>("[1,1.0]").unwrap();
    let current: runtime_constraints::UniqueItemsValidationError =
        runtime_constraints::validate_unique_json_items(&values).unwrap_err();
    let compatible: json_schema::unique_items::UniqueItemsValidationError = current;
    assert!(matches!(
        &compatible,
        runtime_constraints::UniqueItemsValidationError::Duplicate {
            first_index: 1,
            duplicate_index: 2,
        }
    ));
    assert_eq!(
        json_schema::unique_items::validate_unique_json_items(&values),
        Err(compatible)
    );
}

#[test]
fn alternatives_and_null_absence_survive_preparation_and_normalized_output() {
    let optional = json_schema::import_str(
        r#"{"type":"object","properties":{"optional":{"type":["integer","null"]}}}"#,
    )
    .unwrap();
    let prepared = PreparedSchema::new(optional.clone());
    let absent = from_str("{}", &optional).unwrap();
    let explicit_null = from_str(r#"{"optional":null}"#, &optional).unwrap();
    assert_ne!(absent, explicit_null);
    for (input, instance) in [("{}", absent), (r#"{"optional":null}"#, explicit_null)] {
        assert_eq!(prepared.from_str(input).unwrap(), instance);
        assert_eq!(
            prepared.to_value(&instance).unwrap(),
            serde_json::from_str::<serde_json::Value>(input).unwrap()
        );
    }
    let schema = json_schema::import_str(r#"{
        "type":"object", "additionalProperties":false,
        "properties":{
            "optional":{"type":["integer","null"]},
            "choice":{"oneOf":[
                {"type":"object","required":["kind","n"],"additionalProperties":false,"properties":{"kind":{"const":"n"},"n":{"type":"integer","minimum":2}}},
                {"type":"object","required":["kind","s"],"additionalProperties":false,"properties":{"kind":{"const":"s"},"s":{"type":"string","pattern":"^ok$"}}}
            ]}
        }
    }"#).unwrap();
    let prepared = PreparedSchema::new(schema.clone());
    for (input, accepted) in [
        (r#"{"choice":{"kind":"n","n":2}}"#, true),
        (r#"{"optional":null,"choice":{"kind":"n","n":2}}"#, true),
        (r#"{"choice":{"kind":"s","s":"ok"}}"#, true),
        (r#"{"choice":{"kind":"n","n":1}}"#, false),
        (r#"{"choice":{"kind":"s","s":"no"}}"#, false),
        (r#"{"choice":{"kind":"n","s":"ok"}}"#, false),
    ] {
        let result = from_str(input, &schema);
        assert_eq!(result.is_ok(), accepted, "{input}: {result:?}");
        assert_eq!(
            prepared.from_str(input).map_err(|e| format!("{e:?}")),
            result
                .as_ref()
                .map(Clone::clone)
                .map_err(|e| format!("{e:?}"))
        );
        if let Ok(instance) = result {
            assert_eq!(
                prepared.to_value(&instance).unwrap(),
                serde_json::from_str::<serde_json::Value>(input).unwrap()
            );
        }
    }
    let integer =
        json_schema::import_str(r#"{"type":"integer","minimum":5,"multipleOf":2}"#).unwrap();
    let prepared = PreparedSchema::new(integer.clone());
    for (instance, expected) in [
        (Instance::Scalar(Value::String("3".into())), Some("range")),
        (
            Instance::Scalar(Value::String("7".into())),
            Some("multiple"),
        ),
        (Instance::Scalar(Value::String("6".into())), None),
    ] {
        let output = to_value(&integer, &instance);
        assert_eq!(output.as_ref().err().map(error_kind), expected);
        assert_eq!(
            prepared.to_value(&instance).map_err(|e| format!("{e:?}")),
            output.map_err(|e| format!("{e:?}"))
        );
        assert_eq!(
            to_string(&integer, &instance)
                .as_ref()
                .err()
                .map(error_kind),
            expected
        );
    }
}
