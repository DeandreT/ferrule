use ir::{Instance, InstanceGroup, Value, XML_TYPE_ORIGIN_FIELD, XmlTypeOrigin};

fn fields() -> Vec<(String, Instance)> {
    vec![("Code".into(), Instance::Scalar(Value::String("a".into())))]
}

#[test]
fn typed_origin_is_private_before_object_constraints_without_hiding_data_names() {
    let schema = format_json::json_schema::import_str(r#"{"type":"object","additionalProperties":{"type":"string"},"minProperties":1,"maxProperties":1,"propertyNames":{"enum":["Code"]}}"#).unwrap();
    for origin in [
        XmlTypeOrigin::Absent,
        XmlTypeOrigin::Explicit("{urn:types}Derived"),
    ] {
        let value = Instance::Group(
            InstanceGroup::from(fields())
                .with_xml_type_origin(origin)
                .unwrap(),
        );
        let output: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&schema, &value).unwrap()).unwrap();
        assert_eq!(output, serde_json::json!({"Code":"a"}));
        assert_eq!(value.xml_type_origin(), Ok(origin));
    }
}

#[test]
fn actual_physical_and_dynamic_origin_spelling_roundtrip_as_ordinary_json_data() {
    for physical in [false, true] {
        let schema_json = if physical {
            serde_json::json!({"type":"object","properties":{XML_TYPE_ORIGIN_FIELD:{"type":"string"}},"required":[XML_TYPE_ORIGIN_FIELD],"additionalProperties":false})
        } else {
            serde_json::json!({"type":"object","additionalProperties":{"type":"string"},"minProperties":1,"maxProperties":1})
        };
        let schema = format_json::json_schema::import_str(&schema_json.to_string()).unwrap();
        for text in ["", "ordinary JSON value", "{urn:types}Derived"] {
            let input = serde_json::json!({XML_TYPE_ORIGIN_FIELD:text});
            let value = format_json::from_str(&input.to_string(), &schema).unwrap();
            assert_eq!(value.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
            assert_eq!(
                value
                    .field(XML_TYPE_ORIGIN_FIELD)
                    .and_then(Instance::as_scalar),
                Some(&Value::String(text.into()))
            );
            let output = format_json::to_string(&schema, &value).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&output).unwrap(),
                input
            );
        }
    }
}

#[test]
fn json_uniqueness_uses_data_even_when_runtime_origins_differ() {
    let schema = format_json::json_schema::import_str(r#"{"type":"array","uniqueItems":true,"items":{"type":"object","additionalProperties":{"type":"string"}}}"#).unwrap();
    let a = Instance::Group(
        InstanceGroup::from(fields())
            .with_xml_type_origin(XmlTypeOrigin::Absent)
            .unwrap(),
    );
    let b = Instance::Group(
        InstanceGroup::from(fields())
            .with_xml_type_origin(XmlTypeOrigin::Explicit("Derived"))
            .unwrap(),
    );
    assert_ne!(a, b);
    assert!(format_json::to_string(&schema, &Instance::Repeated(vec![a, b])).is_err());
}
