//! Finite catalog codec: preserve absent fields without schema-reader defaults.
pub fn instance(value: &serde_json::Value) -> ir::Instance {
    use ir::{Instance, Value};
    match value {
        serde_json::Value::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), instance(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
        serde_json::Value::Array(items) => Instance::Repeated(items.iter().map(instance).collect()),
        serde_json::Value::String(value) => Instance::Scalar(Value::String(value.clone())),
        serde_json::Value::Bool(value) => Instance::Scalar(Value::Bool(*value)),
        serde_json::Value::Number(value) => {
            Instance::Scalar(Value::Int(value.as_i64().expect("finite catalog integer")))
        }
        serde_json::Value::Null => Instance::Scalar(Value::json_null()),
    }
}
