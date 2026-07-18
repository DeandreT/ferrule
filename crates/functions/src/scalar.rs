use ir::Value;

pub(super) fn text(value: &Value) -> String {
    match value {
        Value::Null | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}
