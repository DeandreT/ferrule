//! Bounded SWIFT MT output. The supported subset is documented on
//! [`to_bytes`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{Instance, SchemaKind, SchemaNode, Value};
use mapping::{SwiftCharset, SwiftFieldLayout, SwiftMessageLayout, SwiftMtLayout, SwiftValueExpr};
use thiserror::Error;

use super::{
    FieldCaptures, MAX_FIELDS_PER_MESSAGE, MAX_MESSAGES, MAX_PARSE_STATES, ParsedFields,
    SwiftHeaders, build_field_value, build_node, capture_value, from_bytes, message_instance,
    parse_field, root_instance, scalar_union_name,
};
use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES};

const APPLICATION_HEADER: &str = "Application Header";

#[derive(Debug, Error)]
pub enum SwiftWriteError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("`{value}` is not a supported SWIFT {block}")]
    InvalidHeader { block: &'static str, value: String },
    #[error("no SWIFT {0} application header was configured")]
    MissingApplicationHeader(&'static str),
    #[error("unsupported SWIFT output schema: {0}")]
    UnsupportedSchema(String),
    #[error("SWIFT layout cannot be written exactly: {0}")]
    UnsupportedLayout(String),
    #[error("SWIFT message type `{0}` has no embedded layout")]
    UnknownMessage(String),
    #[error("SWIFT output has no messages")]
    NoMessages,
    #[error("SWIFT output has {messages} messages but {headers} header sets")]
    HeaderCount { messages: usize, headers: usize },
    #[error("SWIFT message {message} repeats the {block} of an earlier message")]
    RepeatedHeader { message: usize, block: &'static str },
    #[error(
        "SWIFT message {message} has occurrences of several repeating fields ({tags}); \
         their interleaving is not recorded"
    )]
    InterleavedRepetition { message: usize, tags: String },
    #[error("SWIFT message {message} must select exactly one {item}, found {found}")]
    Selection {
        message: usize,
        item: &'static str,
        found: usize,
    },
    #[error("SWIFT node `{path}` expected {expected}, got {got}")]
    InstanceShape {
        path: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("SWIFT group `{path}` has unexpected field `{field}`")]
    UnexpectedField { path: String, field: String },
    #[error("SWIFT group `{path}` has duplicate field `{field}`")]
    DuplicateField { path: String, field: String },
    #[error("SWIFT value at `{path}` is not written by any layout field")]
    Unrepresentable { path: String },
    #[error("SWIFT field `{tag}` cannot write the value at `{path}` with its layout grammar")]
    Unconsumed { tag: String, path: String },
    #[error("SWIFT field `{tag}` requires a value at `{path}`")]
    MissingValue { tag: String, path: String },
    #[error("SWIFT field `{tag}` value at `{path}` expected {expected}, got {got}")]
    ValueType {
        tag: String,
        path: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("SWIFT field `{tag}` value at `{path}` is not an unsigned finite decimal: `{value}`")]
    InvalidDecimal {
        tag: String,
        path: String,
        value: String,
    },
    #[error(
        "SWIFT field `{tag}` value at `{path}` contains {character:?}, outside the {charset} character set"
    )]
    InvalidCharacter {
        tag: String,
        path: String,
        character: char,
        charset: &'static str,
    },
    #[error("SWIFT field `{tag}` value at `{path}` contains a line break that is not CRLF")]
    BareLineBreak { tag: String, path: String },
    #[error(
        "SWIFT field `{tag}` value at `{path}` is {actual} characters; the layout allows {min}..={max}"
    )]
    Length {
        tag: String,
        path: String,
        min: usize,
        max: usize,
        actual: usize,
    },
    #[error("SWIFT field `{tag}` value at `{path}` is `{value}`, not one of its layout codes")]
    NotInCodeList {
        tag: String,
        path: String,
        value: String,
    },
    #[error("SWIFT field `{tag}` occurs {count} times, but its layout does not repeat")]
    Repetition { tag: String, count: usize },
    #[error("SWIFT field `{tag}` at `{path}` reuses a tag that an earlier layout field reads")]
    ShadowedTag { tag: String, path: String },
    #[error("SWIFT field `{tag}` line {line} cannot start with `{first}`")]
    LineStart {
        tag: String,
        line: usize,
        first: char,
    },
    #[error("SWIFT field `{tag}` cannot end with a line break")]
    TrailingLineBreak { tag: String },
    #[error("SWIFT field `{tag}` has several layout alternatives for its values")]
    AmbiguousChoice { tag: String },
    #[error(
        "SWIFT field `{tag}` has an optional constant whose presence the instance does not record"
    )]
    AmbiguousOptional { tag: String },
    #[error("SWIFT field `{tag}` does not re-read to the written values")]
    AmbiguousField { tag: String },
    #[error("SWIFT output could not be re-read: {0}")]
    ReadBack(#[source] EdiFormatError),
    #[error("SWIFT output does not re-read to the written instance")]
    ReadBackMismatch,
    #[error("SWIFT output exceeds the {0} limit")]
    Limit(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OutputLimits {
    pub(super) output_bytes: usize,
    pub(super) messages: usize,
    pub(super) fields_per_message: usize,
    /// Expression steps per field occurrence, like the reader's parse budget.
    pub(super) render_steps: usize,
}

impl OutputLimits {
    pub(super) const DEFAULT: Self = Self {
        output_bytes: MAX_RUNTIME_INPUT_BYTES,
        messages: MAX_MESSAGES,
        fields_per_message: MAX_FIELDS_PER_MESSAGE,
        render_steps: MAX_PARSE_STATES,
    };
}

/// Serializes and verifies the complete output before touching `path`, so a
/// refused instance never replaces an existing destination.
pub fn write(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    layout: &SwiftMtLayout,
    headers: &[SwiftHeaders],
) -> Result<(), SwiftWriteError> {
    let bytes = to_bytes(schema, instance, layout, headers)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Serializes a SWIFT MT message stream for a documented subset of the
/// embedded [`SwiftMtLayout`] grammar.
///
/// The writer emits exactly what [`from_bytes`] reads back:
///
/// - **Blocks.** Each message is `{1:…}{2:…}{4:\r\n…-}` with no separator
///   between messages. The instance does not carry header values, so
///   `headers` holds one [`SwiftHeaders`] per message, in order; a repeated
///   basic header or output message input reference is refused. User-header
///   (3) and trailer (5) blocks are not written.
/// - **Messages.** The root group holds one repeating `Message`; any other
///   present root value is refused. Each message selects exactly one
///   `Application Header` direction (`Input` or `Output`, an empty group) and
///   one message-type group named `MTnnn` with an embedded layout. Sibling
///   message-type groups without any present value are ignored; a lone
///   message-type group is selected even when empty.
/// - **Field order.** Fields are written in layout order; repeated occurrences
///   of one field are consecutive. Each field is `:TAG:content\r\n`, where the
///   tag is uppercase letters and digits and only the first layout field with
///   a given tag is readable, so later fields sharing it are refused.
/// - **Repetition.** A repeating schema node writes one occurrence per item.
///   More than one occurrence of a field whose layout does not repeat is
///   refused, as is any value under a repeating node that is not a field.
///   The layout does not record how occurrences of different fields
///   interleave (for example `:61:`/`:86:` pairs), so a message with
///   occurrences of more than one repeating field is refused.
/// - **Required values.** A non-repeating field is written when any of its
///   captured values is present; null, JSON null, XML nil and the empty string
///   are absent. Every non-optional capture of a written field needs a value,
///   and every present value must be consumed by the grammar. Layout
///   constants whose presence the instance cannot record (a non-repeating
///   field without captures, an optional constant, or alternatives that differ
///   only in constants) are refused rather than guessed.
/// - **Values.** Captures keep the layout's length range. Numeric, alphabetic
///   and alphanumeric captures accept strings (or integers) in their reader
///   character classes. Text captures accept only the SWIFT X character set
///   with CRLF line breaks. The layout maps the `x`, `y` and `z` formats to
///   one text class, so the wider `z` characters are refused, while values
///   for a `y` field are not checked for its narrower set (no lowercase).
///   Decimal captures accept unsigned finite numbers or plain decimal strings
///   and are written with a comma separator (`12,5`, `3,`).
/// - **Lines.** Continuation lines may not start with `:` or `-`, and field
///   content may not end with a line break.
/// - **Limits.** The reader's output size, message and field counts apply,
///   and each field occurrence has the reader's per-field work budget.
///
/// Every field is re-parsed with the reader's grammar, and the complete output
/// is re-read and compared with the expected instance before it is returned,
/// so ambiguous grammars fail with a typed error instead of drifting.
pub fn to_bytes(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &SwiftMtLayout,
    headers: &[SwiftHeaders],
) -> Result<Vec<u8>, SwiftWriteError> {
    to_bytes_with_limits(schema, instance, layout, headers, OutputLimits::DEFAULT)
}

pub(super) fn to_bytes_with_limits(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &SwiftMtLayout,
    headers: &[SwiftHeaders],
    limits: OutputLimits,
) -> Result<Vec<u8>, SwiftWriteError> {
    let (root_children, message_schema) = output_schema(schema)?;
    let root_fields = group_fields(|| schema.name.clone(), instance, root_children)?;
    let messages = match root_fields.iter().find(|(name, _)| name == "Message") {
        Some((_, Instance::Repeated(items))) => items.as_slice(),
        Some((_, other)) => {
            return Err(shape_error(
                format!("{}/Message", schema.name),
                "repeating values",
                other,
            ));
        }
        None => &[],
    };
    // The reader keeps only `Message`, so any other present root value is lost.
    if let Some((name, _)) = root_fields
        .iter()
        .find(|(name, value)| name != "Message" && !is_absent_instance(value))
    {
        return Err(SwiftWriteError::Unrepresentable {
            path: format!("{}/{name}", schema.name),
        });
    }
    if messages.is_empty() {
        return Err(SwiftWriteError::NoMessages);
    }
    if messages.len() > limits.messages {
        return Err(SwiftWriteError::Limit("message count"));
    }
    if headers.len() != messages.len() {
        return Err(SwiftWriteError::HeaderCount {
            messages: messages.len(),
            headers: headers.len(),
        });
    }
    let mut writer = Writer {
        output: Vec::new(),
        limits,
        validated: BTreeSet::new(),
        basic_headers: BTreeSet::new(),
        input_references: BTreeSet::new(),
    };
    let mut expected = Vec::with_capacity(messages.len());
    for (index, (message, header)) in messages.iter().zip(headers).enumerate() {
        expected.push(writer.message(index + 1, message_schema, message, layout, header)?);
    }
    let output = writer.output;
    let reread = from_bytes(&output, schema, layout, false).map_err(SwiftWriteError::ReadBack)?;
    if reread != root_instance(expected) {
        return Err(SwiftWriteError::ReadBackMismatch);
    }
    Ok(output)
}

struct Writer {
    output: Vec<u8>,
    limits: OutputLimits,
    /// Message types whose layout has passed the per-type checks.
    validated: BTreeSet<String>,
    /// Header values already written, which FIN never repeats in a stream.
    basic_headers: BTreeSet<String>,
    input_references: BTreeSet<String>,
}

impl Writer {
    fn push(&mut self, text: &str) -> Result<(), SwiftWriteError> {
        let fits = self
            .output
            .len()
            .checked_add(text.len())
            .is_some_and(|size| size <= self.limits.output_bytes);
        if !fits {
            return Err(SwiftWriteError::Limit("output size"));
        }
        self.output.extend_from_slice(text.as_bytes());
        Ok(())
    }

    /// Writes one message and returns the instance the reader must produce.
    fn message(
        &mut self,
        index: usize,
        message_schema: &SchemaNode,
        message: &Instance,
        layout: &SwiftMtLayout,
        headers: &SwiftHeaders,
    ) -> Result<Instance, SwiftWriteError> {
        let SchemaKind::Group { children, .. } = &message_schema.kind else {
            return Err(SwiftWriteError::UnsupportedSchema(
                "`Message` is not a group".into(),
            ));
        };
        let fields = group_fields(|| format!("Message[{index}]"), message, children)?;
        let direction = match fields.iter().find(|(name, _)| name == APPLICATION_HEADER) {
            Some((_, header)) => {
                direction(index, message_schema.child(APPLICATION_HEADER), header)?
            }
            None => {
                return Err(SwiftWriteError::Selection {
                    message: index,
                    item: "application header direction",
                    found: 0,
                });
            }
        };
        let mut selected = fields
            .iter()
            .filter(|(name, _)| name != APPLICATION_HEADER)
            .collect::<Vec<_>>();
        // A mapping can materialize empty groups for unselected message types.
        if selected.len() > 1 {
            selected.retain(|(_, value)| !is_absent_instance(value));
        }
        let [(message_type, selected_instance)] = selected.as_slice() else {
            return Err(SwiftWriteError::Selection {
                message: index,
                item: "message type",
                found: selected.len(),
            });
        };
        let message_layout = layout
            .message(message_type)
            .ok_or_else(|| SwiftWriteError::UnknownMessage(message_type.clone()))?;
        let digits = message_type_digits(message_type)?;
        let selected_schema = message_schema
            .child(message_type)
            .filter(|schema| !schema.repeating && matches!(schema.kind, SchemaKind::Group { .. }))
            .ok_or_else(|| {
                SwiftWriteError::UnsupportedSchema(format!(
                    "message type `{message_type}` is not a single group"
                ))
            })?;
        let (letter, tail) = match direction {
            "Input" => ("I", headers.input_tail()),
            _ => ("O", headers.output_tail()),
        };
        let tail = tail.ok_or(SwiftWriteError::MissingApplicationHeader(direction))?;
        let repeated = if !self
            .basic_headers
            .insert(headers.basic_header().to_string())
        {
            Some("basic header")
        } else if let Some(reference) = headers.message_input_reference()
            && letter == "O"
            && !self.input_references.insert(reference.to_string())
        {
            Some("message input reference")
        } else {
            None
        };
        if let Some(block) = repeated {
            return Err(SwiftWriteError::RepeatedHeader {
                message: index,
                block,
            });
        }

        if !self.validated.contains(message_type.as_str()) {
            validate_message_fields(message_layout.fields())?;
            self.validated.insert(message_type.clone());
        }
        let mut collected = BTreeMap::new();
        collect_node(
            selected_schema,
            &mut vec![message_type.clone()],
            Some(selected_instance),
            message_layout.fields(),
            &mut collected,
        )?;
        let repeated = message_layout
            .fields()
            .iter()
            .filter(|field| {
                collected
                    .get(field.path())
                    .is_some_and(|entry| entry.schema.repeating)
            })
            .map(SwiftFieldLayout::tag)
            .collect::<Vec<_>>();
        if repeated.len() > 1 {
            return Err(SwiftWriteError::InterleavedRepetition {
                message: index,
                tags: repeated.join(", "),
            });
        }

        self.push("{1:")?;
        self.push(headers.basic_header())?;
        self.push("}{2:")?;
        self.push(letter)?;
        self.push(digits)?;
        self.push(tail)?;
        self.push("}{4:\r\n")?;
        let mut parsed = ParsedFields {
            values: BTreeMap::new(),
        };
        let mut field_count = 0usize;
        for (position, field) in message_layout.fields().iter().enumerate() {
            let Some(entry) = collected.get(field.path()) else {
                continue;
            };
            check_tag_syntax(field)?;
            check_shadowed_tag(message_layout.fields(), position)?;
            for occurrence in &entry.occurrences {
                field_count += 1;
                if field_count > self.limits.fields_per_message {
                    return Err(SwiftWriteError::Limit("field count"));
                }
                // Leaves room for `:TAG:` and the line break so oversized
                // content fails while it is being rendered.
                let room = self
                    .limits
                    .output_bytes
                    .saturating_sub(self.output.len() + field.tag().len() + 4);
                let budget = Budget {
                    steps: self.limits.render_steps,
                    room,
                };
                let (content, captures) = render_field(field, entry.schema, occurrence, budget)?;
                self.push(":")?;
                self.push(field.tag())?;
                self.push(":")?;
                self.push(&content)?;
                self.push("\r\n")?;
                parsed
                    .values
                    .entry(field.path().to_vec())
                    .or_default()
                    .push(captures);
            }
        }
        self.push("-}")?;
        let selected = build_node(selected_schema, std::slice::from_ref(message_type), &parsed);
        Ok(message_instance(direction, message_type.clone(), selected))
    }
}

/// The root's fields and its repeating `Message` group, which every output
/// needs.
pub(super) fn output_schema(
    schema: &SchemaNode,
) -> Result<(&[SchemaNode], &SchemaNode), SwiftWriteError> {
    // The shared refusal only reports an unsupported schema.
    crate::refuse_json_tuple(schema).map_err(|error| match error {
        EdiFormatError::UnsupportedSchema(message) => SwiftWriteError::UnsupportedSchema(message),
        other => SwiftWriteError::UnsupportedSchema(other.to_string()),
    })?;
    if let Some(name) = scalar_union_name(schema) {
        return Err(SwiftWriteError::UnsupportedSchema(format!(
            "field `{name}` cannot preserve a heterogeneous scalar union"
        )));
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(SwiftWriteError::UnsupportedSchema(format!(
            "root `{}` is not a group",
            schema.name
        )));
    };
    let message_schema = schema
        .child("Message")
        .filter(|message| message.repeating)
        .ok_or_else(|| {
            SwiftWriteError::UnsupportedSchema(format!(
                "root `{}` has no repeating `Message` group",
                schema.name
            ))
        })?;
    Ok((children, message_schema))
}

/// Layout checks that depend on neither the instance, the headers nor the
/// mapping. Shadowed tags depend on which fields are written; see
/// [`check_shadowed_tag`].
pub(super) fn validate_message_layout(message: &SwiftMessageLayout) -> Result<(), SwiftWriteError> {
    message_type_digits(message.message_type())?;
    validate_message_fields(message.fields())?;
    message.fields().iter().try_for_each(check_tag_syntax)
}

fn message_type_digits(message_type: &str) -> Result<&str, SwiftWriteError> {
    message_type
        .strip_prefix("MT")
        .filter(|digits| digits.len() == 3 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| {
            SwiftWriteError::UnsupportedLayout(format!(
                "message type `{message_type}` is not `MT` followed by three digits"
            ))
        })
}

fn validate_message_fields(fields: &[SwiftFieldLayout]) -> Result<(), SwiftWriteError> {
    match fields
        .iter()
        .find(|field| !field.repeating() && !has_captures(field.value()))
    {
        Some(field) => Err(SwiftWriteError::UnsupportedLayout(format!(
            "non-repeating field `{}` has no value captures, so its presence is not recorded",
            field.tag()
        ))),
        None => Ok(()),
    }
}

/// The reader recognizes only uppercase-and-digit tags.
fn check_tag_syntax(field: &SwiftFieldLayout) -> Result<(), SwiftWriteError> {
    if field.tag().is_empty()
        || !field
            .tag()
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err(SwiftWriteError::UnsupportedLayout(format!(
            "field tag `{}` is not uppercase letters and digits",
            field.tag()
        )));
    }
    Ok(())
}

/// The reader reads only the first layout field with a given tag, so a
/// written field whose tag an earlier layout field uses would re-read as
/// that earlier field.
pub(super) fn check_shadowed_tag(
    fields: &[SwiftFieldLayout],
    position: usize,
) -> Result<(), SwiftWriteError> {
    let field = &fields[position];
    if fields[..position]
        .iter()
        .any(|earlier| earlier.tag() == field.tag())
    {
        return Err(SwiftWriteError::ShadowedTag {
            tag: field.tag().to_string(),
            path: field.path().join("/"),
        });
    }
    Ok(())
}

fn direction(
    index: usize,
    schema: Option<&SchemaNode>,
    header: &Instance,
) -> Result<&'static str, SwiftWriteError> {
    let Some(SchemaKind::Group { children, .. }) = schema.map(|schema| &schema.kind) else {
        return Err(SwiftWriteError::UnsupportedSchema(format!(
            "`{APPLICATION_HEADER}` is not a group"
        )));
    };
    let path = || format!("Message[{index}]/{APPLICATION_HEADER}");
    let fields = group_fields(path, header, children)?;
    let mut selected = None;
    for (name, value) in fields {
        let direction = match name.as_str() {
            "Input" => "Input",
            "Output" => "Output",
            _ => {
                return Err(SwiftWriteError::UnexpectedField {
                    path: path(),
                    field: name.clone(),
                });
            }
        };
        if !matches!(value, Instance::Group(group) if group.is_empty()) {
            return Err(shape_error(
                format!("{}/{name}", path()),
                "an empty group",
                value,
            ));
        }
        if selected.replace(direction).is_some() {
            return Err(SwiftWriteError::Selection {
                message: index,
                item: "application header direction",
                found: fields.len(),
            });
        }
    }
    selected.ok_or(SwiftWriteError::Selection {
        message: index,
        item: "application header direction",
        found: 0,
    })
}

/// Present values of one field occurrence, keyed by capture path relative to
/// the field.
type Occurrence<'a> = Vec<(Vec<String>, &'a Value)>;

struct CollectedField<'a> {
    schema: &'a SchemaNode,
    occurrences: Vec<Occurrence<'a>>,
}

/// Walks the selected message exactly as the reader's `build_node` does and
/// collects the occurrences of each layout field.
fn collect_node<'a>(
    schema: &'a SchemaNode,
    path: &mut Vec<String>,
    instance: Option<&'a Instance>,
    fields: &[SwiftFieldLayout],
    collected: &mut BTreeMap<Vec<String>, CollectedField<'a>>,
) -> Result<(), SwiftWriteError> {
    if let Some(field) = fields.iter().find(|field| field.path() == path.as_slice()) {
        let items: Vec<&Instance> = match (schema.repeating, instance) {
            (_, None) => Vec::new(),
            (true, Some(Instance::Repeated(items))) => items.iter().collect(),
            (true, Some(other)) => {
                return Err(shape_error(path.join("/"), "repeating values", other));
            }
            (false, Some(item @ (Instance::Scalar(_) | Instance::Group(_)))) => vec![item],
            (false, Some(other)) => return Err(shape_error(path.join("/"), "one value", other)),
        };
        let mut occurrences = Vec::with_capacity(items.len());
        for item in items {
            let mut values = Vec::new();
            field_values(schema, item, &mut Vec::new(), path, &mut values)?;
            if schema.repeating || !values.is_empty() {
                occurrences.push(values);
            }
        }
        if !field.repeating() && occurrences.len() > 1 {
            return Err(SwiftWriteError::Repetition {
                tag: field.tag().to_string(),
                count: occurrences.len(),
            });
        }
        if !occurrences.is_empty() {
            collected.insert(
                path.clone(),
                CollectedField {
                    schema,
                    occurrences,
                },
            );
        }
        return Ok(());
    }
    let Some(instance) = instance else {
        return Ok(());
    };
    if schema.repeating {
        return match instance {
            Instance::Repeated(items) if items.is_empty() => Ok(()),
            Instance::Repeated(_) => Err(SwiftWriteError::Unrepresentable {
                path: path.join("/"),
            }),
            other => Err(shape_error(path.join("/"), "repeating values", other)),
        };
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => match instance {
            Instance::Scalar(value) if is_absent(value) => Ok(()),
            Instance::Scalar(_) => Err(SwiftWriteError::Unrepresentable {
                path: path.join("/"),
            }),
            other => Err(shape_error(path.join("/"), "a scalar", other)),
        },
        SchemaKind::Group { children, .. } => {
            let values = group_fields(|| path.join("/"), instance, children)?;
            for child in children {
                let value = values
                    .iter()
                    .find(|(name, _)| name == &child.name)
                    .map(|(_, value)| value);
                path.push(child.name.clone());
                let result = collect_node(child, path, value, fields, collected);
                path.pop();
                result?;
            }
            Ok(())
        }
    }
}

/// Mirrors the reader's `build_field_value`: scalars sit at their relative
/// path and groups never repeat inside a field.
fn field_values<'a>(
    schema: &'a SchemaNode,
    instance: &'a Instance,
    relative: &mut Vec<String>,
    field_path: &[String],
    out: &mut Occurrence<'a>,
) -> Result<(), SwiftWriteError> {
    let display = |relative: &[String]| join_path(field_path, relative);
    match &schema.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => match instance {
            Instance::Scalar(value) => {
                if !is_absent(value) {
                    out.push((relative.clone(), value));
                }
                Ok(())
            }
            other => Err(shape_error(display(relative), "a scalar", other)),
        },
        SchemaKind::Group { children, .. } => {
            let fields = group_fields(|| display(relative), instance, children)?;
            for child in children {
                if let Some((_, value)) = fields.iter().find(|(name, _)| name == &child.name) {
                    relative.push(child.name.clone());
                    let result = field_values(child, value, relative, field_path, out);
                    relative.pop();
                    result?;
                }
            }
            Ok(())
        }
    }
}

fn group_fields<'a>(
    path: impl Fn() -> String,
    instance: &'a Instance,
    children: &[SchemaNode],
) -> Result<&'a [(String, Instance)], SwiftWriteError> {
    let Instance::Group(fields) = instance else {
        return Err(shape_error(path(), "a group", instance));
    };
    for (index, (name, _)) in fields.iter().enumerate() {
        if !children.iter().any(|child| child.name == *name) {
            return Err(SwiftWriteError::UnexpectedField {
                path: path(),
                field: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(SwiftWriteError::DuplicateField {
                path: path(),
                field: name.clone(),
            });
        }
    }
    Ok(fields.as_slice())
}

fn shape_error(path: String, expected: &'static str, instance: &Instance) -> SwiftWriteError {
    let got = match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "a group",
        Instance::Repeated(_) => "repeating values",
        Instance::MappedSequence(_) => "a mapped sequence",
        Instance::DocumentSet(_) => "a document set",
    };
    SwiftWriteError::InstanceShape {
        path,
        expected,
        got,
    }
}

fn is_absent(value: &Value) -> bool {
    matches!(value, Value::Null | Value::JsonNull(_) | Value::XmlNil(_))
        || matches!(value, Value::String(text) if text.is_empty())
}

/// No present scalar and no repeated occurrence anywhere below `instance`.
fn is_absent_instance(instance: &Instance) -> bool {
    match instance {
        Instance::Scalar(value) => is_absent(value),
        Instance::Group(fields) => fields.iter().all(|(_, value)| is_absent_instance(value)),
        Instance::Repeated(items) | Instance::MappedSequence(items) => items.is_empty(),
        Instance::DocumentSet(documents) => documents.is_empty(),
    }
}

fn join_path(base: &[String], relative: &[String]) -> String {
    base.iter()
        .chain(relative)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("/")
}

/// Renders one occurrence and returns its content with the captures the
/// reader must produce for it.
fn render_field(
    field: &SwiftFieldLayout,
    schema: &SchemaNode,
    occurrence: &Occurrence<'_>,
    mut budget: Budget,
) -> Result<(String, FieldCaptures), SwiftWriteError> {
    let values = occurrence
        .iter()
        .map(|(path, value)| (path.clone(), *value))
        .collect::<BTreeMap<_, _>>();
    let scope = FieldScope {
        tag: field.tag(),
        path: field.path(),
        values: &values,
    };
    let rendered = render(field.value(), &scope, &mut budget)?;
    if let Some((path, _)) = occurrence.iter().find(|(path, _)| {
        !rendered
            .captures
            .iter()
            .any(|(captured, _)| captured == path)
    }) {
        return Err(SwiftWriteError::Unconsumed {
            tag: field.tag().to_string(),
            path: scope.display(path),
        });
    }
    check_content(&scope, &rendered.text)?;
    let ambiguous = || SwiftWriteError::AmbiguousField {
        tag: field.tag().to_string(),
    };
    let reread = parse_field(field, &rendered.text).map_err(|_| ambiguous())?;
    if build_field_value(schema, &reread, &[]) != build_field_value(schema, &rendered.captures, &[])
    {
        return Err(ambiguous());
    }
    Ok((rendered.text, rendered.captures))
}

struct FieldScope<'s, 'a> {
    tag: &'s str,
    path: &'s [String],
    values: &'s BTreeMap<Vec<String>, &'a Value>,
}

impl<'a> FieldScope<'_, 'a> {
    fn display(&self, relative: &[String]) -> String {
        join_path(self.path, relative)
    }

    fn value(&self, relative: &[String]) -> Result<&'a Value, SwiftWriteError> {
        self.values
            .get(relative)
            .copied()
            .ok_or_else(|| SwiftWriteError::MissingValue {
                tag: self.tag.to_string(),
                path: self.display(relative),
            })
    }

    fn tag(&self) -> String {
        self.tag.to_string()
    }
}

#[derive(Default)]
struct Rendered {
    text: String,
    captures: FieldCaptures,
}

impl Rendered {
    fn consumed(&self) -> usize {
        self.captures
            .iter()
            .map(|(path, _)| path)
            .collect::<BTreeSet<_>>()
            .len()
    }
}

/// Work and output room for one field occurrence.
struct Budget {
    steps: usize,
    room: usize,
}

impl Budget {
    fn step(&mut self) -> Result<(), SwiftWriteError> {
        self.steps = self
            .steps
            .checked_sub(1)
            .ok_or(SwiftWriteError::Limit("render work"))?;
        Ok(())
    }

    fn fits(&self, length: usize) -> Result<(), SwiftWriteError> {
        if length > self.room {
            return Err(SwiftWriteError::Limit("output size"));
        }
        Ok(())
    }
}

fn render(
    expression: &SwiftValueExpr,
    scope: &FieldScope<'_, '_>,
    budget: &mut Budget,
) -> Result<Rendered, SwiftWriteError> {
    budget.step()?;
    match expression {
        SwiftValueExpr::Empty => Ok(Rendered::default()),
        SwiftValueExpr::Literal { value } => {
            budget.fits(value.len())?;
            Ok(Rendered {
                text: value.clone(),
                captures: Vec::new(),
            })
        }
        SwiftValueExpr::Capture {
            path,
            min,
            max,
            charset,
        } => {
            let value = scope.value(path)?;
            let (min, max) = (usize::from(*min).max(1), usize::from(*max));
            let length_error = |actual| SwiftWriteError::Length {
                tag: scope.tag(),
                path: scope.display(path),
                min,
                max,
                actual,
            };
            // Bounds the copy below; decimal spelling adds at most one byte.
            if let Value::String(text) = value
                && text.len() > max + 1
            {
                return Err(length_error(text.len()));
            }
            let text = if *charset == SwiftCharset::Decimal {
                decimal_text(value, scope, path)?
            } else {
                plain_text(value, scope, path)?
            };
            check_charset(*charset, &text, scope, path)?;
            if !(min..=max).contains(&text.len()) {
                return Err(length_error(text.len()));
            }
            budget.fits(text.len())?;
            let normalized =
                capture_value(*charset, &text).ok_or_else(|| SwiftWriteError::InvalidDecimal {
                    tag: scope.tag(),
                    path: scope.display(path),
                    value: text.clone(),
                })?;
            Ok(Rendered {
                text,
                captures: vec![(path.clone(), normalized)],
            })
        }
        SwiftValueExpr::EnumCapture { path, values } => {
            let value = scope.value(path)?;
            let text = match value {
                Value::String(text) => std::borrow::Cow::Borrowed(text.as_str()),
                other => std::borrow::Cow::Owned(plain_text(other, scope, path)?),
            };
            let Some(text) = values.iter().find(|code| **code == text).cloned() else {
                return Err(SwiftWriteError::NotInCodeList {
                    tag: scope.tag(),
                    path: scope.display(path),
                    // Bounded: the value can be arbitrarily long.
                    value: text.chars().take(64).collect(),
                });
            };
            budget.fits(text.len())?;
            Ok(Rendered {
                captures: vec![(path.clone(), Value::String(text.clone()))],
                text,
            })
        }
        SwiftValueExpr::Sequence { parts } => {
            let mut rendered = Rendered::default();
            for part in parts {
                let part = render(part, scope, budget)?;
                budget.fits(rendered.text.len() + part.text.len())?;
                rendered.text.push_str(&part.text);
                rendered.captures.extend(part.captures);
            }
            Ok(rendered)
        }
        SwiftValueExpr::Alternatives { choices } => {
            let mut best: Option<Rendered> = None;
            let mut tied = false;
            let mut first_error = None;
            for choice in choices {
                match render(choice, scope, budget) {
                    Ok(candidate) => match &best {
                        Some(current) if candidate.consumed() < current.consumed() => {}
                        Some(current) if candidate.consumed() == current.consumed() => {
                            tied |= candidate.text != current.text;
                        }
                        _ => {
                            best = Some(candidate);
                            tied = false;
                        }
                    },
                    Err(error @ SwiftWriteError::Limit(_)) => return Err(error),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            match best {
                Some(_) if tied => Err(SwiftWriteError::AmbiguousChoice { tag: scope.tag() }),
                Some(best) => Ok(best),
                None => {
                    Err(first_error
                        .unwrap_or(SwiftWriteError::AmbiguousChoice { tag: scope.tag() }))
                }
            }
        }
        SwiftValueExpr::Optional { value } => match render(value, scope, budget) {
            Ok(inner) if inner.consumed() > 0 || inner.text.is_empty() => Ok(inner),
            Ok(_) if has_captures(value) => Ok(Rendered::default()),
            Ok(_) => Err(SwiftWriteError::AmbiguousOptional { tag: scope.tag() }),
            Err(error @ SwiftWriteError::Limit(_)) => Err(error),
            Err(error) => {
                if mentions(value, scope.values, budget)? {
                    Err(error)
                } else {
                    Ok(Rendered::default())
                }
            }
        },
    }
}

/// Layout validation bounds expression depth and node count, so this needs
/// no work budget.
fn has_captures(expression: &SwiftValueExpr) -> bool {
    match expression {
        SwiftValueExpr::Empty | SwiftValueExpr::Literal { .. } => false,
        SwiftValueExpr::Capture { .. } | SwiftValueExpr::EnumCapture { .. } => true,
        SwiftValueExpr::Sequence { parts: children }
        | SwiftValueExpr::Alternatives { choices: children } => children.iter().any(has_captures),
        SwiftValueExpr::Optional { value } => has_captures(value),
    }
}

/// Whether any capture in `expression` reads one of the present `values`.
fn mentions(
    expression: &SwiftValueExpr,
    values: &BTreeMap<Vec<String>, &Value>,
    budget: &mut Budget,
) -> Result<bool, SwiftWriteError> {
    budget.step()?;
    Ok(match expression {
        SwiftValueExpr::Empty | SwiftValueExpr::Literal { .. } => false,
        SwiftValueExpr::Capture { path, .. } | SwiftValueExpr::EnumCapture { path, .. } => {
            values.contains_key(path)
        }
        SwiftValueExpr::Sequence { parts: children }
        | SwiftValueExpr::Alternatives { choices: children } => {
            for child in children {
                if mentions(child, values, budget)? {
                    return Ok(true);
                }
            }
            false
        }
        SwiftValueExpr::Optional { value } => mentions(value, values, budget)?,
    })
}

fn plain_text(
    value: &Value,
    scope: &FieldScope<'_, '_>,
    path: &[String],
) -> Result<String, SwiftWriteError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Int(number) => Ok(number.to_string()),
        other => Err(SwiftWriteError::ValueType {
            tag: scope.tag(),
            path: scope.display(path),
            expected: "a string",
            got: other.type_name(),
        }),
    }
}

/// Unsigned finite numbers and plain decimal strings, written with the SWIFT
/// comma separator and an explicit integer part.
fn decimal_text(
    value: &Value,
    scope: &FieldScope<'_, '_>,
    path: &[String],
) -> Result<String, SwiftWriteError> {
    let invalid = |value: String| SwiftWriteError::InvalidDecimal {
        tag: scope.tag(),
        path: scope.display(path),
        value,
    };
    let plain = match value {
        Value::Float(number) if !number.is_finite() || *number < 0.0 => {
            return Err(invalid(number.to_string()));
        }
        // Normalizes negative zero, which the reader cannot produce.
        Value::Float(number) if *number == 0.0 => "0".to_string(),
        Value::Float(number) => number.to_string(),
        Value::Int(number) if *number < 0 => return Err(invalid(number.to_string())),
        Value::Int(number) => number.to_string(),
        // `Decimal96` has no negative zero, so any sign is a negative value.
        Value::Decimal(number) if number.is_negative() => return Err(invalid(number.to_string())),
        Value::Decimal(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => {
            return Err(SwiftWriteError::ValueType {
                tag: scope.tag(),
                path: scope.display(path),
                expected: "a number",
                got: other.type_name(),
            });
        }
    };
    let (whole, fraction) = match plain.find(['.', ',']) {
        Some(index) => (&plain[..index], &plain[index + 1..]),
        None => (plain.as_str(), ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid(plain));
    }
    Ok(format!("{whole},{fraction}"))
}

fn check_charset(
    charset: SwiftCharset,
    text: &str,
    scope: &FieldScope<'_, '_>,
    path: &[String],
) -> Result<(), SwiftWriteError> {
    let (name, accepts): (&'static str, fn(char) -> bool) = match charset {
        SwiftCharset::Numeric => ("numeric", |character| character.is_ascii_digit()),
        SwiftCharset::Alphabetic => ("alphabetic", |character| character.is_ascii_uppercase()),
        SwiftCharset::Alphanumeric => ("alphanumeric", |character| {
            character.is_ascii_uppercase() || character.is_ascii_digit()
        }),
        SwiftCharset::Decimal => ("decimal", |character| {
            character.is_ascii_digit() || character == ','
        }),
        SwiftCharset::Text => return check_x_text(text, scope, &scope.display(path)),
    };
    match text.chars().find(|character| !accepts(*character)) {
        Some(character) => Err(SwiftWriteError::InvalidCharacter {
            tag: scope.tag(),
            path: scope.display(path),
            character,
            charset: name,
        }),
        None => Ok(()),
    }
}

/// SWIFT X: letters, digits, `/ - ? : ( ) . , ' +`, space, and CRLF.
fn check_x_text(text: &str, scope: &FieldScope<'_, '_>, path: &str) -> Result<(), SwiftWriteError> {
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' if characters.peek() == Some(&'\n') => {
                characters.next();
            }
            '\r' | '\n' => {
                return Err(SwiftWriteError::BareLineBreak {
                    tag: scope.tag(),
                    path: path.to_string(),
                });
            }
            character
                if character.is_ascii_alphanumeric()
                    || matches!(
                        character,
                        '/' | '-' | '?' | ':' | '(' | ')' | '.' | ',' | '\'' | '+' | ' '
                    ) => {}
            character => {
                return Err(SwiftWriteError::InvalidCharacter {
                    tag: scope.tag(),
                    path: path.to_string(),
                    character,
                    charset: "SWIFT X",
                });
            }
        }
    }
    Ok(())
}

/// Checks the complete field content, including layout constants.
fn check_content(scope: &FieldScope<'_, '_>, text: &str) -> Result<(), SwiftWriteError> {
    check_x_text(text, scope, &scope.display(&[]))?;
    if text.ends_with("\r\n") {
        return Err(SwiftWriteError::TrailingLineBreak { tag: scope.tag() });
    }
    for (index, line) in text.split("\r\n").enumerate().skip(1) {
        if let Some(first @ (':' | '-')) = line.chars().next() {
            return Err(SwiftWriteError::LineStart {
                tag: scope.tag(),
                line: index + 1,
                first,
            });
        }
    }
    Ok(())
}
