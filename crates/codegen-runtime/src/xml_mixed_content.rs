use std::collections::BTreeMap;

use ir::{
    XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};

use crate::{Instance, RuntimeError, ScopeContext, Value};

pub type XmlMixedContentExpression = for<'a> fn(&ScopeContext<'a>) -> Result<Value, RuntimeError>;

/// One statically validated direct-element replacement.
pub struct XmlMixedContentReplacement<'a> {
    pub element: &'a str,
    pub collection: &'a [&'a str],
    pub expression: XmlMixedContentExpression,
}

/// Atomizes retained XML text/element content in source order.
pub fn xml_mixed_content(
    context: &ScopeContext<'_>,
    frame: Option<&[&str]>,
    path: &[&str],
    replacements: &[XmlMixedContentReplacement<'_>],
) -> Result<Value, RuntimeError> {
    let group = context.resolve_xml_instance(frame, path)?;
    let Some(items) = group
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
    else {
        return Ok(group
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null));
    };

    let mut output = String::new();
    let mut occurrences = BTreeMap::<&str, usize>::new();
    for item in items {
        let name = string_field(item, XML_NODE_NAME_FIELD);
        let text = string_field(item, XML_TEXT_FIELD);
        let Some(replacement) = replacements.iter().find(|rule| rule.element == name) else {
            output.push_str(text);
            continue;
        };
        let item_context;
        let expression_context = if !replacement.collection.is_empty()
            && let Some(value) = item.field(XML_MIXED_CONTENT_VALUE_FIELD)
        {
            let occurrence = occurrences.entry(name).or_default();
            *occurrence += 1;
            item_context =
                context.with_xml_mixed_content_value(value, replacement.collection, *occurrence);
            &item_context
        } else {
            context
        };
        append_value(&mut output, (replacement.expression)(expression_context)?);
    }
    Ok(Value::String(output))
}

fn string_field<'a>(item: &'a Instance, name: &str) -> &'a str {
    item.field(name)
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or_default()
}

fn append_value(output: &mut String, value: Value) {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {}
        Value::Bool(value) => output.push_str(if value { "true" } else { "false" }),
        Value::Int(value) => output.push_str(&value.to_string()),
        Value::Float(value) => output.push_str(&value.to_string()),
        Value::String(value) => output.push_str(&value),
    }
}
