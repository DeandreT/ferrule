//! Context-neutral scalar producer argument phase, shared with ordinary emission.
//! The caller supplies expression identifiers/context before any user literals
//! are rendered. A null argument empties only the producer, not its consumer.

use super::{GeneratedSequence, NodeId};

pub(super) fn runtime_helper(sequence: &GeneratedSequence) -> Option<&'static str> {
    match sequence {
        GeneratedSequence::Range { .. } => Some("generate_sequence"),
        GeneratedSequence::Tokenize { .. } => Some("tokenize"),
        GeneratedSequence::TokenizeByLength { .. } => Some("tokenize_by_length"),
        GeneratedSequence::TokenizeRegex { .. } => Some("tokenize_regex"),
        GeneratedSequence::RecursiveCollect { .. }
        | GeneratedSequence::OwnedRecursiveCollect { .. }
        | GeneratedSequence::FilterMapV1(_) => None,
    }
}

/// Materialize the complete vector in ordered parent-argument phases. Returning
/// false means this is not a pure producer; no source is appended in that case.
pub(super) fn render_values(
    sequence: &GeneratedSequence,
    indent: &str,
    output: &mut String,
    evaluate: &impl Fn(NodeId) -> String,
) -> bool {
    match sequence {
        GeneratedSequence::Tokenize {
            input, delimiter, ..
        } => {
            let input = evaluate(*input);
            let delimiter = evaluate(*delimiter);
            output.push_str(&format!(
                "{indent}let sequence_input = {input}?;\n{indent}let sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {{\n{indent}    Vec::new()\n{indent}}} else {{\n{indent}    let sequence_parameter = {delimiter}?;\n{indent}    if sequence_parameter == Value::Null || sequence_parameter.is_json_null() {{\n{indent}        Vec::new()\n{indent}    }} else {{\n{indent}        tokenize(sequence_input, sequence_parameter)?\n{indent}    }}\n{indent}}};\n"
            ));
        }
        GeneratedSequence::TokenizeByLength { input, length, .. } => {
            let input = evaluate(*input);
            let length = evaluate(*length);
            output.push_str(&format!(
                "{indent}let sequence_input = {input}?;\n{indent}let sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {{\n{indent}    Vec::new()\n{indent}}} else {{\n{indent}    let sequence_parameter = {length}?;\n{indent}    if sequence_parameter == Value::Null || sequence_parameter.is_json_null() {{\n{indent}        Vec::new()\n{indent}    }} else {{\n{indent}        tokenize_by_length(sequence_input, sequence_parameter)?\n{indent}    }}\n{indent}}};\n"
            ));
        }
        GeneratedSequence::TokenizeRegex {
            input,
            pattern,
            flags,
            ..
        } => {
            let input = evaluate(*input);
            let pattern = evaluate(*pattern);
            output.push_str(&format!(
                "{indent}let sequence_input = {input}?;\n{indent}let sequence_values = if sequence_input == Value::Null || sequence_input.is_json_null() {{\n{indent}    Vec::new()\n{indent}}} else {{\n{indent}    let sequence_pattern = {pattern}?;\n{indent}    if sequence_pattern == Value::Null || sequence_pattern.is_json_null() {{\n{indent}        Vec::new()\n{indent}    }} else {{\n"
            ));
            match flags {
                Some(flags) => {
                    let flags = evaluate(*flags);
                    output.push_str(&format!(
                        "{indent}        let sequence_flags = {flags}?;\n{indent}        if sequence_flags == Value::Null || sequence_flags.is_json_null() {{\n{indent}            Vec::new()\n{indent}        }} else {{\n{indent}            tokenize_regex(sequence_input, sequence_pattern, Some(sequence_flags))?\n{indent}        }}\n"
                    ));
                }
                None => output.push_str(&format!(
                    "{indent}        tokenize_regex(sequence_input, sequence_pattern, None)?\n"
                )),
            }
            output.push_str(&format!("{indent}    }}\n{indent}}};\n"));
        }
        GeneratedSequence::Range {
            from: Some(from),
            to,
            ..
        } => {
            let from = evaluate(*from);
            let to = evaluate(*to);
            output.push_str(&format!(
                "{indent}let sequence_from = {from}?;\n{indent}let sequence_values = if sequence_from == Value::Null || sequence_from.is_json_null() {{\n{indent}    Vec::new()\n{indent}}} else {{\n{indent}    let sequence_to = {to}?;\n{indent}    if sequence_to == Value::Null || sequence_to.is_json_null() {{\n{indent}        Vec::new()\n{indent}    }} else {{\n{indent}        generate_sequence(Some(sequence_from), sequence_to)?\n{indent}    }}\n{indent}}};\n"
            ));
        }
        GeneratedSequence::Range { from: None, to, .. } => {
            let to = evaluate(*to);
            output.push_str(&format!(
                "{indent}let sequence_to = {to}?;\n{indent}let sequence_values = if sequence_to == Value::Null || sequence_to.is_json_null() {{\n{indent}    Vec::new()\n{indent}}} else {{\n{indent}    generate_sequence(None, sequence_to)?\n{indent}}};\n"
            ));
        }
        GeneratedSequence::RecursiveCollect { .. }
        | GeneratedSequence::OwnedRecursiveCollect { .. }
        | GeneratedSequence::FilterMapV1(_) => {
            return false;
        }
    }
    true
}

/// Materialize an owned recursive walk in a paired Basic `context`: prefix,
/// then separator, then the exact-owner root selection. Owned access failures
/// carry the consuming `node`; returns false for every other producer.
pub(super) fn render_owned_values(
    sequence: &GeneratedSequence,
    node: NodeId,
    indent: &str,
    output: &mut String,
    evaluate: &impl Fn(NodeId) -> String,
) -> bool {
    let GeneratedSequence::OwnedRecursiveCollect {
        collection,
        children,
        descent_value,
        values,
        value,
        prefix,
        separator,
        ..
    } = sequence
    else {
        return false;
    };
    let prefix = evaluate(*prefix);
    let separator = evaluate(*separator);
    let owner = super::owned_basic::owner_literal(&collection.owner);
    let path = super::render_string_path;
    output.push_str(&format!(
        "{indent}let sequence_prefix = codegen_runtime::recursive_sequence_parameter({prefix}?)?;\n{indent}let sequence_separator = codegen_runtime::recursive_sequence_parameter({separator}?)?;\n{indent}let sequence_values = codegen_runtime::owned_recursive_collect(\n{indent}    context,\n{indent}    {owner},\n{indent}    codegen_runtime::RecursiveCollectPaths {{\n{indent}        collection: &[{}],\n{indent}        children: &[{}],\n{indent}        descent_value: &[{}],\n{indent}        values: &[{}],\n{indent}        value: &[{}],\n{indent}    }},\n{indent}    &sequence_prefix,\n{indent}    &sequence_separator,\n{indent})\n{indent}.map_err(|error| error.with_owned_source_node({node}))?;\n",
        path(&collection.path),
        path(children),
        path(descent_value),
        path(values),
        path(value),
    ));
    true
}
