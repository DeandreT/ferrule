use super::*;

pub(super) const FIELDS: [&str; 4] = [
    "TypedNull",
    "UntypedNull",
    "FunctionParameterNull",
    "FunctionParameterXmlNil",
];

pub(super) fn append_controls(nodes: &mut BTreeMap<u32, Node>, bindings: &mut Vec<Binding>) {
    let table = vec![
        (Value::Null, Value::String("null-first".into())),
        (Value::Null, Value::String("null-second".into())),
        (Value::json_null(), Value::String("json-first".into())),
        (Value::json_null(), Value::String("json-second".into())),
        (Value::xml_nil(), Value::String("nil-first".into())),
        (Value::xml_nil(), Value::String("nil-second".into())),
    ];
    // The unchanged original fixture owns node17 (Null), node25 (XmlNil),
    // and the typed parameter function2; the last old main node is30.
    for (node, target_field, input_type) in [
        (31, "TypedNull", Some(ScalarType::String)),
        (32, "UntypedNull", None),
    ] {
        assert!(
            nodes
                .insert(
                    node,
                    Node::ValueMap {
                        input: 17,
                        input_type,
                        table: table.clone(),
                        default: Some(Value::String("marker-default".into())),
                    }
                )
                .is_none()
        );
        bindings.push(Binding {
            target_field: target_field.into(),
            node,
        });
    }
    for (node, target_field, input) in [
        (33, "FunctionParameterNull", 17),
        (34, "FunctionParameterXmlNil", 25),
    ] {
        assert!(
            nodes
                .insert(
                    node,
                    Node::UserFunctionCall {
                        function: FunctionId::new(2),
                        args: vec![input],
                    }
                )
                .is_none()
        );
        bindings.push(Binding {
            target_field: target_field.into(),
            node,
        });
    }
}

pub(super) fn expected() -> Vec<(String, Instance)> {
    vec![
        (
            "TypedNull".into(),
            Instance::Scalar(Value::String("null-first".into())),
        ),
        (
            "UntypedNull".into(),
            Instance::Scalar(Value::String("null-first".into())),
        ),
        (
            "FunctionParameterNull".into(),
            Instance::Scalar(Value::String("null-row".into())),
        ),
        (
            "FunctionParameterXmlNil".into(),
            Instance::Scalar(Value::String("xml-nil-row".into())),
        ),
    ]
}
