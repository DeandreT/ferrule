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

/// One direct-child rename retained in a constructed mixed-content stream.
pub struct XmlMixedContentElement<'a> {
    pub source: &'a str,
    pub target: &'a str,
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

/// Attaches the current source's ordered mixed-content stream to a constructed
/// target group, substituting mapped target occurrences in source order.
pub fn preserve_xml_mixed_content(
    context: &ScopeContext<'_>,
    mut output: Instance,
    elements: &[XmlMixedContentElement<'_>],
) -> Instance {
    let Some(source_items) = context
        .current_instance()
        .and_then(|source| source.field(XML_MIXED_CONTENT_FIELD))
        .and_then(Instance::as_repeated)
    else {
        return output;
    };
    let Instance::Group(fields) = &mut output else {
        return output;
    };

    let mut occurrences = BTreeMap::<&str, usize>::new();
    let items = source_items
        .iter()
        .filter_map(|item| {
            let name = optional_string_field(item, XML_NODE_NAME_FIELD)?;
            if name.is_empty() {
                return Some(item.clone());
            }
            let element = elements.iter().find(|element| element.source == name)?;
            let index = occurrences.entry(element.target).or_default();
            let value = fields
                .iter()
                .find(|(field, _)| field == element.target)
                .and_then(|(_, value)| value.as_repeated())?
                .get(*index)?
                .clone();
            *index += 1;
            let text = value
                .as_scalar()
                .map(mixed_content_text)
                .unwrap_or_default();
            Some(Instance::Group(vec![
                (
                    XML_NODE_NAME_FIELD.to_string(),
                    Instance::Scalar(Value::String(element.target.to_string())),
                ),
                (
                    XML_TEXT_FIELD.to_string(),
                    Instance::Scalar(Value::String(text)),
                ),
                (XML_MIXED_CONTENT_VALUE_FIELD.to_string(), value),
            ]))
        })
        .collect::<Vec<_>>();
    if !items.is_empty() {
        fields.push((
            XML_MIXED_CONTENT_FIELD.to_string(),
            Instance::Repeated(items),
        ));
    }
    output
}

fn string_field<'a>(item: &'a Instance, name: &str) -> &'a str {
    optional_string_field(item, name).unwrap_or_default()
}

fn optional_string_field<'a>(item: &'a Instance, name: &str) -> Option<&'a str> {
    item.field(name)
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
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

fn mixed_content_text(value: &Value) -> String {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{field, group, repeated, scalar, string};

    fn content_item(name: &str, text: &str) -> Instance {
        group([
            field(XML_NODE_NAME_FIELD, scalar(Value::String(name.to_string()))),
            field(XML_TEXT_FIELD, scalar(string(text))),
        ])
    }

    fn ordered_items(output: &Instance) -> &[Instance] {
        output
            .field(XML_MIXED_CONTENT_FIELD)
            .and_then(Instance::as_repeated)
            .unwrap_or_default()
    }

    #[test]
    fn target_preservation_renames_shared_occurrences_and_drops_unmapped_elements() {
        let source = group([field(
            XML_MIXED_CONTENT_FIELD,
            repeated([
                content_item("", "before "),
                content_item("Em", "old"),
                content_item("Strong", "old"),
                content_item("Code", "drop"),
                group([field(XML_TEXT_FIELD, scalar(string("malformed")))]),
                content_item("", " after"),
            ]),
        )]);
        let output = group([field(
            "Styled",
            repeated([scalar(string("first")), scalar(string("second"))]),
        )]);
        let output = preserve_xml_mixed_content(
            &ScopeContext::new(&source),
            output,
            &[
                XmlMixedContentElement {
                    source: "Em",
                    target: "Styled",
                },
                XmlMixedContentElement {
                    source: "Strong",
                    target: "Styled",
                },
            ],
        );

        let ordered = ordered_items(&output);
        assert_eq!(ordered.len(), 4);
        assert_eq!(
            ordered
                .iter()
                .map(|item| string_field(item, XML_NODE_NAME_FIELD))
                .collect::<Vec<_>>(),
            ["", "Styled", "Styled", ""]
        );
        assert_eq!(
            ordered
                .iter()
                .map(|item| string_field(item, XML_TEXT_FIELD))
                .collect::<Vec<_>>(),
            ["before ", "first", "second", " after"]
        );
    }

    #[test]
    fn target_preservation_omits_exhausted_occurrences_and_is_a_noop_without_metadata() {
        let source = group([field(
            XML_MIXED_CONTENT_FIELD,
            repeated([content_item("Em", "old"), content_item("Strong", "old")]),
        )]);
        let output = group([field("Styled", repeated([scalar(string("only"))]))]);
        let output = preserve_xml_mixed_content(
            &ScopeContext::new(&source),
            output,
            &[
                XmlMixedContentElement {
                    source: "Em",
                    target: "Styled",
                },
                XmlMixedContentElement {
                    source: "Strong",
                    target: "Styled",
                },
            ],
        );
        assert_eq!(ordered_items(&output).len(), 1);

        let plain_source = group([field(XML_TEXT_FIELD, scalar(string("plain")))]);
        let plain_output = group([field("Styled", repeated([scalar(string("value"))]))]);
        assert_eq!(
            preserve_xml_mixed_content(
                &ScopeContext::new(&plain_source),
                plain_output.clone(),
                &[XmlMixedContentElement {
                    source: "Em",
                    target: "Styled",
                }],
            ),
            plain_output
        );
    }
}
