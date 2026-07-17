//! Bounded SWIFT MT envelope and text-block input.

use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, SchemaKind, SchemaNode, Value};
use mapping::{SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtLayout, SwiftValueExpr};

use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES, read_bounded_input};

const MAX_MESSAGES: usize = 100_000;
const MAX_FIELDS_PER_MESSAGE: usize = 100_000;
const MAX_PARSE_STATES: usize = 100_000;

pub fn read(
    path: &Path,
    schema: &SchemaNode,
    layout: &SwiftMtLayout,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    let bytes = read_bounded_input(path, EdiFormatError::SwiftLimit("input size"))?;
    from_bytes(&bytes, schema, layout, lenient)
}

pub fn from_bytes(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &SwiftMtLayout,
    lenient: bool,
) -> Result<Instance, EdiFormatError> {
    if bytes.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::SwiftLimit("input size"));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| EdiFormatError::NotSwift("input is not UTF-8"))?;
    let envelopes = parse_envelopes(text)?;
    if envelopes.len() > MAX_MESSAGES {
        return Err(EdiFormatError::SwiftLimit("message count"));
    }
    let message_schema = schema
        .child("Message")
        .ok_or_else(|| EdiFormatError::UnsupportedSchema(schema.name.clone()))?;
    let mut messages = Vec::with_capacity(envelopes.len());
    for blocks in envelopes {
        let application = blocks.get("2").ok_or(EdiFormatError::NotSwift(
            "message has no application header block",
        ))?;
        let (direction, message_type) = application_header(application)?;
        let message_layout = layout
            .message(&message_type)
            .ok_or_else(|| EdiFormatError::UnknownSwiftMessage(message_type.clone()))?;
        let text_block = blocks
            .get("4")
            .ok_or(EdiFormatError::NotSwift("message has no text block"))?;
        let parsed = parse_text_block(text_block, message_layout, lenient)?;
        let selected_schema = message_schema
            .child(&message_type)
            .ok_or_else(|| EdiFormatError::UnsupportedSchema(message_type.clone()))?;
        let selected = build_node(
            selected_schema,
            std::slice::from_ref(&message_type),
            &parsed,
        );
        messages.push(Instance::Group(vec![
            (
                "Application Header".into(),
                Instance::Group(vec![(direction.into(), Instance::Group(Vec::new()))]),
            ),
            (message_type, selected),
        ]));
    }
    Ok(Instance::Group(vec![(
        "Message".into(),
        Instance::Repeated(messages),
    )]))
}

fn parse_envelopes(text: &str) -> Result<Vec<BTreeMap<String, String>>, EdiFormatError> {
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    let mut messages = Vec::<BTreeMap<String, String>>::new();
    while cursor < bytes.len() {
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        if bytes.get(cursor) != Some(&b'{') {
            return Err(EdiFormatError::NotSwift("expected a block opening brace"));
        }
        let tag_start = cursor + 1;
        let colon = bytes[tag_start..]
            .iter()
            .position(|byte| *byte == b':')
            .map(|offset| tag_start + offset)
            .ok_or(EdiFormatError::NotSwift("block has no tag separator"))?;
        let tag = text
            .get(tag_start..colon)
            .ok_or(EdiFormatError::NotSwift("block tag is not UTF-8"))?;
        let mut depth = 1usize;
        let mut end = colon + 1;
        while end < bytes.len() && depth > 0 {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        if depth != 0 {
            return Err(EdiFormatError::NotSwift("unterminated block"));
        }
        if tag == "1" || messages.is_empty() {
            messages.push(BTreeMap::new());
        }
        let content = text
            .get(colon + 1..end - 1)
            .ok_or(EdiFormatError::NotSwift("block content is not UTF-8"))?;
        let current = messages
            .last_mut()
            .ok_or(EdiFormatError::NotSwift("block precedes a message"))?;
        if current
            .insert(tag.to_string(), content.to_string())
            .is_some()
        {
            return Err(EdiFormatError::NotSwift(
                "message contains a duplicate block",
            ));
        }
        cursor = end;
    }
    if messages.is_empty() || messages.iter().any(|message| !message.contains_key("1")) {
        return Err(EdiFormatError::NotSwift(
            "message has no basic header block",
        ));
    }
    Ok(messages)
}

fn application_header(value: &str) -> Result<(&'static str, String), EdiFormatError> {
    let direction = match value.as_bytes().first() {
        Some(b'I') => "Input",
        Some(b'O') => "Output",
        _ => {
            return Err(EdiFormatError::NotSwift(
                "invalid application-header direction",
            ));
        }
    };
    let digits = value
        .get(1..4)
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or(EdiFormatError::NotSwift(
            "invalid application-header message type",
        ))?;
    Ok((direction, format!("MT{digits}")))
}

#[derive(Debug)]
struct ParsedFields {
    values: BTreeMap<Vec<String>, Vec<FieldCaptures>>,
}

type FieldCapture = (Vec<String>, Value);
type FieldCaptures = Vec<FieldCapture>;

fn parse_text_block(
    value: &str,
    layout: &SwiftMessageLayout,
    lenient: bool,
) -> Result<ParsedFields, EdiFormatError> {
    let value = value
        .strip_prefix("\r\n")
        .or_else(|| value.strip_prefix('\n'))
        .unwrap_or(value);
    let value = value
        .strip_suffix("\r\n-")
        .or_else(|| value.strip_suffix("\n-"))
        .or_else(|| value.strip_suffix('-'))
        .unwrap_or(value);
    let raw_fields = tagged_fields(value)?;
    if raw_fields.len() > MAX_FIELDS_PER_MESSAGE {
        return Err(EdiFormatError::SwiftLimit("field count"));
    }
    let mut parsed = ParsedFields {
        values: BTreeMap::new(),
    };
    for (tag, raw) in raw_fields {
        let Some(field) = layout.fields().iter().find(|field| field.tag() == tag) else {
            if lenient {
                continue;
            }
            return Err(EdiFormatError::SwiftFieldParse { tag });
        };
        let captures = parse_field(field, &raw)?;
        let values = parsed.values.entry(field.path().to_vec()).or_default();
        if !field.repeating() && !values.is_empty() {
            return Err(EdiFormatError::SwiftFieldParse { tag });
        }
        values.push(captures);
    }
    Ok(parsed)
}

fn tagged_fields(value: &str) -> Result<Vec<(String, String)>, EdiFormatError> {
    let mut result = Vec::<(String, String)>::new();
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    for line in normalized.split_terminator('\n') {
        if let Some(rest) = line.strip_prefix(':')
            && let Some((tag, content)) = rest.split_once(':')
            && !tag.is_empty()
            && tag
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            result.push((tag.to_string(), content.to_string()));
        } else {
            let Some((_, content)) = result.last_mut() else {
                return Err(EdiFormatError::NotSwift(
                    "text block content precedes its field tag",
                ));
            };
            content.push_str("\r\n");
            content.push_str(line);
        }
    }
    Ok(result)
}

#[derive(Clone)]
struct ParseState {
    position: usize,
    captures: FieldCaptures,
}

fn parse_field(field: &SwiftFieldLayout, value: &str) -> Result<FieldCaptures, EdiFormatError> {
    if !value.is_ascii() {
        return Err(EdiFormatError::SwiftFieldParse {
            tag: field.tag().to_string(),
        });
    }
    let mut budget = MAX_PARSE_STATES;
    let states = parse_expr(
        field.value(),
        value,
        ParseState {
            position: 0,
            captures: Vec::new(),
        },
        &mut budget,
    );
    states
        .into_iter()
        .filter(|state| state.position == value.len())
        .max_by_key(|state| state.captures.len())
        .map(|state| state.captures)
        .ok_or_else(|| EdiFormatError::SwiftFieldParse {
            tag: field.tag().to_string(),
        })
}

fn parse_expr(
    expression: &SwiftValueExpr,
    input: &str,
    state: ParseState,
    budget: &mut usize,
) -> Vec<ParseState> {
    if *budget == 0 {
        return Vec::new();
    }
    *budget -= 1;
    match expression {
        SwiftValueExpr::Empty => vec![state],
        SwiftValueExpr::Literal { value } => input[state.position..]
            .starts_with(value)
            .then(|| ParseState {
                position: state.position + value.len(),
                captures: state.captures,
            })
            .into_iter()
            .collect(),
        SwiftValueExpr::Capture {
            path,
            min,
            max,
            charset,
        } => {
            let available = input.len().saturating_sub(state.position);
            (*min as usize..=available.min(*max as usize))
                .rev()
                .filter_map(|length| {
                    let raw = &input[state.position..state.position + length];
                    let value = capture_value(*charset, raw)?;
                    Some({
                        let mut captures = state.captures.clone();
                        captures.push((path.clone(), value));
                        ParseState {
                            position: state.position + length,
                            captures,
                        }
                    })
                })
                .collect()
        }
        SwiftValueExpr::EnumCapture { path, values } => {
            let mut values = values.iter().collect::<Vec<_>>();
            values.sort_by_key(|value| std::cmp::Reverse(value.len()));
            values
                .into_iter()
                .filter(|value| input[state.position..].starts_with(value.as_str()))
                .map(|value| {
                    let mut captures = state.captures.clone();
                    captures.push((path.clone(), Value::String(value.clone())));
                    ParseState {
                        position: state.position + value.len(),
                        captures,
                    }
                })
                .collect()
        }
        SwiftValueExpr::Sequence { parts } => {
            let mut states = vec![state];
            for part in parts {
                states = states
                    .into_iter()
                    .flat_map(|state| parse_expr(part, input, state, budget))
                    .take(MAX_PARSE_STATES)
                    .collect();
                if states.is_empty() {
                    break;
                }
            }
            states
        }
        SwiftValueExpr::Alternatives { choices } => choices
            .iter()
            .flat_map(|choice| parse_expr(choice, input, state.clone(), budget))
            .take(MAX_PARSE_STATES)
            .collect(),
        SwiftValueExpr::Optional { value } => {
            let mut present = parse_expr(value, input, state.clone(), budget);
            present.push(state);
            present
        }
    }
}

fn charset_accepts(charset: SwiftCharset, value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| match charset {
            SwiftCharset::Numeric => byte.is_ascii_digit(),
            SwiftCharset::Alphabetic => byte.is_ascii_uppercase(),
            SwiftCharset::Alphanumeric => byte.is_ascii_uppercase() || byte.is_ascii_digit(),
            SwiftCharset::Decimal => byte.is_ascii_digit() || byte == b',',
            SwiftCharset::Text => byte.is_ascii_graphic() || matches!(byte, b' ' | b'\r' | b'\n'),
        })
}

fn capture_value(charset: SwiftCharset, value: &str) -> Option<Value> {
    if !charset_accepts(charset, value) {
        return None;
    }
    if charset == SwiftCharset::Decimal {
        value
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float)
    } else {
        Some(Value::String(value.to_string()))
    }
}

fn build_node(schema: &SchemaNode, path: &[String], parsed: &ParsedFields) -> Instance {
    if let Some(occurrences) = parsed.values.get(path) {
        if schema.repeating {
            return Instance::Repeated(
                occurrences
                    .iter()
                    .map(|captures| build_field_value(schema, captures, &[]))
                    .collect(),
            );
        }
        if let Some(captures) = occurrences.first() {
            return build_field_value(schema, captures, &[]);
        }
    }
    if schema.repeating {
        return Instance::Repeated(Vec::new());
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => Instance::Scalar(Value::Null),
        SchemaKind::Group { children, .. } => Instance::Group(
            children
                .iter()
                .map(|child| {
                    let mut child_path = path.to_vec();
                    child_path.push(child.name.clone());
                    (child.name.clone(), build_node(child, &child_path, parsed))
                })
                .collect(),
        ),
    }
}

fn build_field_value(schema: &SchemaNode, captures: &[FieldCapture], path: &[String]) -> Instance {
    match &schema.kind {
        SchemaKind::Scalar { .. } => Instance::Scalar(
            captures
                .iter()
                .find(|(capture_path, _)| capture_path == path)
                .map_or(Value::Null, |(_, value)| value.clone()),
        ),
        SchemaKind::Group { children, .. } => Instance::Group(
            children
                .iter()
                .map(|child| {
                    let mut child_path = path.to_vec();
                    child_path.push(child.name.clone());
                    (
                        child.name.clone(),
                        build_field_value(child, captures, &child_path),
                    )
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn capture(path: &[&str], min: u16, max: u16, charset: SwiftCharset) -> SwiftValueExpr {
        SwiftValueExpr::Capture {
            path: path.iter().map(|value| value.to_string()).collect(),
            min,
            max,
            charset,
        }
    }

    fn fixture() -> (SchemaNode, SwiftMtLayout) {
        let statement = SwiftMessageLayout::new(
            "MT950",
            vec![
                SwiftFieldLayout::new(
                    "20",
                    vec!["MT950".into(), "20".into()],
                    false,
                    capture(&[], 1, 16, SwiftCharset::Text),
                ),
                SwiftFieldLayout::new(
                    "61",
                    vec!["MT950".into(), "61".into()],
                    true,
                    SwiftValueExpr::Sequence {
                        parts: vec![
                            capture(&["Date"], 6, 6, SwiftCharset::Numeric),
                            SwiftValueExpr::EnumCapture {
                                path: vec!["Mark".into()],
                                values: vec!["C".into(), "D".into()],
                            },
                            capture(&["Amount"], 1, 15, SwiftCharset::Decimal),
                            SwiftValueExpr::Optional {
                                value: Box::new(SwiftValueExpr::Sequence {
                                    parts: vec![
                                        SwiftValueExpr::Literal {
                                            value: "\r\n".into(),
                                        },
                                        capture(&["Details"], 1, 34, SwiftCharset::Text),
                                    ],
                                }),
                            },
                        ],
                    },
                ),
            ],
        );
        let layout = SwiftMtLayout::new(vec![statement]).unwrap();
        let mut lines = SchemaNode::group(
            "61",
            vec![
                SchemaNode::scalar("Date", ScalarType::String),
                SchemaNode::scalar("Mark", ScalarType::String),
                SchemaNode::scalar("Amount", ScalarType::String),
                SchemaNode::scalar("Details", ScalarType::String),
            ],
        );
        lines.repeating = true;
        let mut message = SchemaNode::group(
            "Message",
            vec![
                SchemaNode::group(
                    "Application Header",
                    vec![SchemaNode::group("Input", Vec::new())],
                ),
                SchemaNode::group(
                    "MT950",
                    vec![SchemaNode::scalar("20", ScalarType::String), lines],
                ),
            ],
        );
        message.repeating = true;
        (SchemaNode::group("SWIFT", vec![message]), layout)
    }

    #[test]
    fn reads_nested_blocks_repeated_fields_and_continuation_lines() {
        let (schema, layout) = fixture();
        let input = b"{1:F01BANK}{2:I950DEST}{3:{121:abc}}{4:\r\n:20:REF-1\r\n:61:240101D12,5\r\nfees\r\n\r\nmore\r\n:61:240102C3,\r\n-}{5:{CHK:123}}";
        let result = from_bytes(input, &schema, &layout, false).unwrap();
        let messages = result
            .field("Message")
            .and_then(Instance::as_repeated)
            .unwrap();
        let statement = messages[0].field("MT950").unwrap();
        assert_eq!(
            statement.field("20").unwrap().as_scalar(),
            Some(&Value::String("REF-1".into()))
        );
        let lines = statement.field("61").unwrap().as_repeated().unwrap();
        assert_eq!(
            lines[0].field("Details").unwrap().as_scalar(),
            Some(&Value::String("fees\r\n\r\nmore".into()))
        );
        assert_eq!(
            lines[0].field("Amount").unwrap().as_scalar(),
            Some(&Value::Float(12.5))
        );
        assert_eq!(
            lines[1].field("Mark").unwrap().as_scalar(),
            Some(&Value::String("C".into()))
        );
    }

    #[test]
    fn absent_repeating_fields_are_empty_sequences() {
        let (schema, layout) = fixture();
        let input = b"{1:F01BANK}{2:I950DEST}{4:\r\n:20:REF-1\r\n-}";

        let result = from_bytes(input, &schema, &layout, false).unwrap();
        let statement = result
            .field("Message")
            .and_then(Instance::as_repeated)
            .and_then(|messages| messages.first())
            .and_then(|message| message.field("MT950"))
            .unwrap();
        assert_eq!(
            statement
                .field("61")
                .and_then(Instance::as_repeated)
                .map(<[Instance]>::len),
            Some(0)
        );
    }
}
