use ir::*;
use serde::de::DeserializeOwned;

fn reject<T: DeserializeOwned>(text: &str) {
    assert!(
        serde_json::from_str::<T>(text).is_err(),
        "from_str accepted {text}"
    );
    assert!(
        serde_json::from_slice::<T>(text.as_bytes()).is_err(),
        "from_slice accepted {text}"
    );
    assert!(
        serde_json::from_reader::<_, T>(text.as_bytes()).is_err(),
        "from_reader accepted {text}"
    );
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<T>();
    assert!(stream.next().unwrap().is_err(), "stream accepted {text}");
}

#[test]
fn forged_numeric_objects_and_other_non_numeric_values_never_become_bounds_or_discriminators() {
    for value in [
        r#"{"$serde_json::private::Number":"1.5"}"#,
        r#"{"\u0024serde_json::private::Number":"\u0031.5"}"#,
        r#"{"$serde_json::private::Number":"1.5","extra":true}"#,
        r#"{"extra":true,"$serde_json::private::Number":"1.5"}"#,
        r#"{"$serde_json::private::Number":"1.5","$serde_json::private::Number":"2.5"}"#,
        r#"{"$serde_json::private::Number":"NaN"}"#,
        r#"{"$serde_json::private::Number":"1e400"}"#,
        r#"{"$serde_json::private::Number":1.5}"#,
        r#"{"value":1.5}"#,
        r#"{}"#,
        r#"[]"#,
        r#"null"#,
        r#"true"#,
        r#""1.5""#,
    ] {
        reject::<FiniteF64>(value);
        reject::<NumberBound>(&format!(r#"{{"value":{value}}}"#));
        for range in [
            format!(r#"{{"kind":"number","bounds":{{"minimum":{{"value":{value}}}}}}}"#),
            format!(r#"{{"bounds":{{"minimum":{{"value":{value}}}}},"kind":"number"}}"#),
        ] {
            reject::<NumericRange>(&range);
            let scalar = format!(
                r#"{{"name":"amount","numeric_range":{range},"kind":{{"kind":"scalar","ty":"float"}}}}"#
            );
            reject::<SchemaNode>(&scalar);
            // Multiple recursive adapters see the literal private key here.
            let child = format!(
                r#"{{"name":"Root","kind":{{"kind":"group","children":[{scalar}],"dynamic":{scalar}}}}}"#
            );
            reject::<SchemaNode>(&child);
            let predicate = format!(r#"{{"kind":"schema","schema":{scalar}}}"#);
            reject::<JsonSchemaPredicate>(&predicate);
            let contains = format!(
                r#"{{"name":"items","repeating":true,"json_contains":[{{"predicate":{predicate},"range":{{"minimum":1}}}}],"kind":{{"kind":"scalar","ty":"float"}}}}"#
            );
            reject::<SchemaNode>(&contains);
            let dependent = format!(
                r#"{{"name":"Root","json_dependent_schemas":[{{"trigger":"enabled","predicate":{predicate}}}],"kind":{{"kind":"group","children":[{{"name":"enabled","kind":{{"kind":"scalar","ty":"bool"}}}}]}}}}"#
            );
            reject::<SchemaNode>(&dependent);
        }
        for tagged in [
            format!(r#"{{"type":"float","value":{value}}}"#),
            format!(r#"{{"value":{value},"type":"float"}}"#),
        ] {
            reject::<JsonAllowedValue>(&tagged);
            reject::<GroupAlternativeConstraintValue>(&tagged);
            let scalar = format!(
                r#"{{"name":"amount","json_allowed_values":[{tagged},{{"type":"float","value":2.5}}],"kind":{{"kind":"scalar","ty":"float"}}}}"#
            );
            reject::<SchemaNode>(&scalar);
            let alternative = format!(
                r#"{{"kind":"group","children":[{{"name":"amount","kind":{{"kind":"scalar","ty":"float"}}}}],"alternatives":[{{"name":"one","members":["amount"],"constraints":[{{"member":"amount","value":{tagged}}}]}}]}}"#
            );
            reject::<SchemaKind>(&alternative);
        }
    }
}

#[test]
fn explicit_value_objects_remain_objects_in_every_numeric_family() {
    // Do not parse the attack through Value: serde_json itself recognizes the
    // private marker while parsing Value and can already erase its origin.
    for value in [
        serde_json::json!({"$serde_json::private::Number":"1.5"}),
        serde_json::json!({"$serde_json::private::Number":"1.5", "extra":true}),
        serde_json::json!({"value":1.5}),
        serde_json::json!([]),
        serde_json::json!(true),
        serde_json::json!("1.5"),
    ] {
        assert!(matches!(
            value,
            serde_json::Value::Object(_)
                | serde_json::Value::Array(_)
                | serde_json::Value::Bool(_)
                | serde_json::Value::String(_)
        ));
        assert!(serde_json::from_value::<FiniteF64>(value.clone()).is_err());
        let bound = serde_json::json!({"value":value});
        assert!(serde_json::from_value::<NumberBound>(bound.clone()).is_err());
        let range = serde_json::json!({"bounds":{"minimum":bound},"kind":"number"});
        assert!(serde_json::from_value::<NumericRange>(range.clone()).is_err());
        let scalar = serde_json::json!({"name":"amount","numeric_range":range,"kind":{"kind":"scalar","ty":"float"}});
        assert!(serde_json::from_value::<SchemaNode>(scalar.clone()).is_err());
        assert!(serde_json::from_value::<SchemaNode>(serde_json::json!({"name":"Root","kind":{"kind":"group","children":[scalar.clone()],"dynamic":scalar.clone()}})).is_err());
        assert!(
            serde_json::from_value::<JsonSchemaPredicate>(
                serde_json::json!({"kind":"schema","schema":scalar})
            )
            .is_err()
        );
        let tagged = serde_json::json!({"value":value,"type":"float"});
        assert!(serde_json::from_value::<JsonAllowedValue>(tagged.clone()).is_err());
        assert!(serde_json::from_value::<GroupAlternativeConstraintValue>(tagged).is_err());
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "wrapper", rename_all = "snake_case")]
enum InternalWrapper {
    Schema { schema: SchemaNode },
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum UntaggedWrapper {
    Schema { schema: SchemaNode },
}

#[test]
fn an_external_buffer_cannot_safely_restore_erased_numeric_provenance() {
    let schema = r#"{"name":"amount","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.50}}},"kind":{"kind":"scalar","ty":"float"}}"#;
    let internal = format!(r#"{{"wrapper":"schema","schema":{schema}}}"#);
    let untagged = format!(r#"{{"schema":{schema}}}"#);
    let first = serde_json::from_str::<InternalWrapper>(&internal);
    let second = serde_json::from_str::<UntaggedWrapper>(&untagged);
    if cfg!(feature = "arbitrary_precision") {
        assert!(first.is_err());
        assert!(second.is_err());
    } else {
        let InternalWrapper::Schema { schema } = first.unwrap();
        assert!(schema.numeric_range.is_some());
        let UntaggedWrapper::Schema { schema } = second.unwrap();
        assert!(schema.numeric_range.is_some());
    }
    for object in [
        r#"{"$serde_json::private::Number":"1.50"}"#,
        r#"{"\u0024serde_json::private::Number":"\u0031.50"}"#,
    ] {
        reject::<InternalWrapper>(&internal.replace("1.50", object));
        reject::<UntaggedWrapper>(&untagged.replace("1.50", object));
    }
}
