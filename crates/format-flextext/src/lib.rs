//! Bounded execution of validated recursive structured-text layouts.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    DelimitedDialect, DelimitedRecordField, FixedWidthRecordField, FlexCommand, FlexTextLayout,
    ManySplitter, OnceSplitter, StoreTrim, TrimSide,
};
use thiserror::Error;

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_INSTANCE_DEPTH: usize = 64;
pub const MAX_INSTANCE_NODES: usize = 1_000_000;
pub const MAX_RECORDS: usize = 1_000_000;
pub const MAX_VALUE_BYTES: usize = 1_048_576;

#[derive(Debug, Error)]
pub enum FlexTextError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime schema does not match the FlexText layout schema")]
    SchemaMismatch,
    #[error("input exceeds the {MAX_INPUT_BYTES}-byte limit")]
    InputTooLarge,
    #[error("output exceeds the {MAX_OUTPUT_BYTES}-byte limit")]
    OutputTooLarge,
    #[error("instance nesting exceeds the limit of {MAX_INSTANCE_DEPTH}")]
    InstanceTooDeep,
    #[error("instance exceeds the limit of {MAX_INSTANCE_NODES} nodes")]
    TooManyNodes,
    #[error("record count exceeds the limit of {MAX_RECORDS}")]
    TooManyRecords,
    #[error("value at `{path}` exceeds the {MAX_VALUE_BYTES}-byte limit")]
    ValueTooLarge { path: String },
    #[error("invalid UTF-8 input: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("at `{path}`: {message}")]
    Data { path: String, message: String },
}

/// Reads one UTF-8 structured-text document from `path`.
pub fn read(
    path: &Path,
    schema: &SchemaNode,
    layout: &FlexTextLayout,
) -> Result<Instance, FlexTextError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(FlexTextError::InputTooLarge);
    }
    let text = String::from_utf8(bytes)?;
    from_str(&text, schema, layout)
}

/// Parses one in-memory UTF-8 structured-text document.
pub fn from_str(
    text: &str,
    schema: &SchemaNode,
    layout: &FlexTextLayout,
) -> Result<Instance, FlexTextError> {
    checked_schema(schema, layout)?;
    if text.len() > MAX_INPUT_BYTES {
        return Err(FlexTextError::InputTooLarge);
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut state = ParseState::default();
    let fields = parse_command(layout.command(), text, layout.root_name(), 1, &mut state)?;
    state.add_nodes(1)?;
    Ok(Instance::Group(fields))
}

/// Writes one structured-text document after fully validating and rendering it.
pub fn write(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    layout: &FlexTextLayout,
) -> Result<(), FlexTextError> {
    let text = to_string(schema, instance, layout)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Renders one structured-text document in memory.
pub fn to_string(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &FlexTextLayout,
) -> Result<String, FlexTextError> {
    checked_schema(schema, layout)?;
    validate_instance_bounds(instance, layout.root_name(), 0, &mut 0)?;
    let root = group_fields(instance, layout.root_name())?;
    reject_duplicate_fields(root, layout.root_name())?;
    let value = required_command_value(root, layout.command(), layout.root_name())?;
    reject_unexpected_fields(
        root,
        layout.command().output_name().into_iter(),
        layout.root_name(),
    )?;
    let rendered = render_command(layout.command(), value, layout.root_name(), 1)?;
    let rendered = normalize_line_endings(&rendered, layout.output_line_ending().as_str())?;
    let mut output = String::new();
    if layout.write_bom() {
        output.push('\u{feff}');
    }
    push_bounded(&mut output, &rendered)?;
    Ok(output)
}

fn checked_schema(schema: &SchemaNode, layout: &FlexTextLayout) -> Result<(), FlexTextError> {
    if schema == &layout.schema() {
        Ok(())
    } else {
        Err(FlexTextError::SchemaMismatch)
    }
}

#[derive(Default)]
struct ParseState {
    nodes: usize,
    records: usize,
}

impl ParseState {
    fn add_nodes(&mut self, count: usize) -> Result<(), FlexTextError> {
        self.nodes = self
            .nodes
            .checked_add(count)
            .ok_or(FlexTextError::TooManyNodes)?;
        if self.nodes > MAX_INSTANCE_NODES {
            Err(FlexTextError::TooManyNodes)
        } else {
            Ok(())
        }
    }

    fn add_records(&mut self, count: usize) -> Result<(), FlexTextError> {
        self.records = self
            .records
            .checked_add(count)
            .ok_or(FlexTextError::TooManyRecords)?;
        if self.records > MAX_RECORDS {
            Err(FlexTextError::TooManyRecords)
        } else {
            Ok(())
        }
    }
}

fn parse_command(
    command: &FlexCommand,
    input: &str,
    path: &str,
    depth: usize,
    state: &mut ParseState,
) -> Result<Vec<(String, Instance)>, FlexTextError> {
    if depth > MAX_INSTANCE_DEPTH {
        return Err(FlexTextError::InstanceTooDeep);
    }
    match command {
        FlexCommand::Ignore => Ok(Vec::new()),
        FlexCommand::Store { name, ty, trim } => {
            let field_path = join_path(path, name);
            let value = parse_stored(input, *ty, trim.as_ref(), &field_path)?;
            state.add_nodes(1)?;
            Ok(vec![(name.clone(), Instance::Scalar(value))])
        }
        FlexCommand::SplitOnce {
            name,
            splitter,
            first,
            second,
        } => {
            let command_path = join_path(path, name);
            let (first_input, second_input) = split_once(input, splitter);
            let mut fields = parse_command(first, first_input, &command_path, depth + 1, state)?;
            fields.extend(parse_command(
                second,
                second_input,
                &command_path,
                depth + 1,
                state,
            )?);
            state.add_nodes(1)?;
            Ok(vec![(name.clone(), Instance::Group(fields))])
        }
        FlexCommand::SplitMany {
            name,
            splitter,
            child,
        } => {
            let command_path = join_path(path, name);
            let chunks = split_many(input, splitter)?;
            state.add_records(chunks.len())?;
            let mut items = Vec::with_capacity(chunks.len());
            for (index, chunk) in chunks.into_iter().enumerate() {
                let item_path = indexed_path(&command_path, index);
                let fields = parse_command(child, chunk, &item_path, depth + 1, state)?;
                if !fields.is_empty() {
                    state.add_nodes(1)?;
                    items.push(Instance::Group(fields));
                }
            }
            state.add_nodes(1)?;
            Ok(vec![(name.clone(), Instance::Repeated(items))])
        }
        FlexCommand::FixedWidthRecords { name, fields } => {
            let command_path = join_path(path, name);
            let records = input_records(input)?;
            state.add_records(records.len())?;
            let mut items = Vec::with_capacity(records.len());
            for (index, record) in records.into_iter().enumerate() {
                items.push(parse_fixed_record(
                    record,
                    fields,
                    &indexed_path(&command_path, index),
                    state,
                )?);
            }
            state.add_nodes(1)?;
            Ok(vec![(name.clone(), Instance::Repeated(items))])
        }
        FlexCommand::DelimitedRecords {
            name,
            dialect,
            fields,
        } => {
            let command_path = join_path(path, name);
            let records = parse_delimited(input, dialect, &command_path)?;
            state.add_records(records.len())?;
            let mut items = Vec::with_capacity(records.len());
            for (index, record) in records.into_iter().enumerate() {
                items.push(parse_delimited_record(
                    record,
                    fields,
                    &indexed_path(&command_path, index),
                    state,
                )?);
            }
            state.add_nodes(1)?;
            Ok(vec![(name.clone(), Instance::Repeated(items))])
        }
        FlexCommand::Switch {
            name,
            arms,
            default,
        } => {
            let command_path = join_path(path, name);
            let mut fields = Vec::new();
            let mut matched = false;
            for arm in arms {
                if input.starts_with(arm.prefix()) {
                    matched = true;
                    fields.extend(parse_command(
                        arm.command(),
                        input,
                        &command_path,
                        depth + 1,
                        state,
                    )?);
                }
            }
            if !matched && let Some(default) = default {
                fields.extend(parse_command(
                    default,
                    input,
                    &command_path,
                    depth + 1,
                    state,
                )?);
            }
            if fields.is_empty() {
                return Ok(Vec::new());
            }
            state.add_nodes(1)?;
            Ok(vec![(name.clone(), Instance::Group(fields))])
        }
    }
}

fn parse_stored(
    input: &str,
    ty: ScalarType,
    trim: Option<&StoreTrim>,
    path: &str,
) -> Result<Value, FlexTextError> {
    let input = match trim {
        None => input,
        Some(trim) => match trim.side() {
            TrimSide::Left => input.trim_start_matches(|c| trim.characters().contains(c)),
            TrimSide::Right => input.trim_end_matches(|c| trim.characters().contains(c)),
            TrimSide::Both => input.trim_matches(|c| trim.characters().contains(c)),
        },
    };
    parse_value(input, ty, false, path)
}

fn parse_value(
    input: &str,
    ty: ScalarType,
    empty_is_null: bool,
    path: &str,
) -> Result<Value, FlexTextError> {
    if input.len() > MAX_VALUE_BYTES {
        return Err(FlexTextError::ValueTooLarge {
            path: path.to_string(),
        });
    }
    if empty_is_null && input.is_empty() {
        return Ok(Value::Null);
    }
    let invalid = || data_error(path, format!("`{input}` is not a valid {ty:?} value"));
    match ty {
        ScalarType::String => Ok(Value::String(input.to_string())),
        ScalarType::Int => input.parse().map(Value::Int).map_err(|_| invalid()),
        ScalarType::Float => {
            let value: f64 = input.parse().map_err(|_| invalid())?;
            if value.is_finite() {
                Ok(Value::Float(value))
            } else {
                Err(invalid())
            }
        }
        ScalarType::Bool => input.parse().map(Value::Bool).map_err(|_| invalid()),
    }
}

fn split_once<'a>(input: &'a str, splitter: &OnceSplitter) -> (&'a str, &'a str) {
    let boundary = match splitter {
        OnceSplitter::FixedLines(count) => line_boundary(input, count.get() as usize),
        OnceSplitter::FixedColumns(count) => input
            .char_indices()
            .nth(count.get() as usize)
            .map_or(input.len(), |(index, _)| index),
        OnceSplitter::Delimiter(delimiter) => {
            if let Some(index) = input.find(delimiter) {
                return (&input[..index], &input[index + delimiter.len()..]);
            }
            input.len()
        }
        OnceSplitter::LineStartingWith(marker) => line_start_offsets(input)
            .find(|offset| input[*offset..].starts_with(marker))
            .unwrap_or(input.len()),
        OnceSplitter::LineContaining(marker) => line_start_offsets(input)
            .find(|offset| line_at(input, *offset).contains(marker))
            .unwrap_or(input.len()),
    };
    (&input[..boundary], &input[boundary..])
}

fn split_many<'a>(input: &'a str, splitter: &ManySplitter) -> Result<Vec<&'a str>, FlexTextError> {
    match splitter {
        ManySplitter::FixedLines(count) => {
            let mut chunks = Vec::new();
            let mut offset = 0;
            while offset < input.len() {
                let length = line_boundary(&input[offset..], count.get() as usize);
                if length == 0 {
                    break;
                }
                if chunks.len() == MAX_RECORDS {
                    return Err(FlexTextError::TooManyRecords);
                }
                chunks.push(&input[offset..offset + length]);
                offset += length;
            }
            Ok(chunks)
        }
        ManySplitter::LinesStartingWith(marker) => {
            let mut offsets = Vec::new();
            for offset in
                line_start_offsets(input).filter(|offset| input[*offset..].starts_with(marker))
            {
                if offsets.len() == MAX_RECORDS {
                    return Err(FlexTextError::TooManyRecords);
                }
                offsets.push(offset);
            }
            Ok(offsets
                .iter()
                .enumerate()
                .map(|(index, start)| {
                    let end = offsets.get(index + 1).copied().unwrap_or(input.len());
                    &input[*start..end]
                })
                .collect())
        }
    }
}

fn line_boundary(input: &str, count: usize) -> usize {
    input
        .match_indices('\n')
        .nth(count.saturating_sub(1))
        .map_or(input.len(), |(index, _)| index + 1)
}

fn line_start_offsets(input: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(input.match_indices('\n').map(|(index, _)| index + 1))
}

fn line_at(input: &str, offset: usize) -> &str {
    let remainder = &input[offset..];
    let length = remainder
        .find('\n')
        .map_or(remainder.len(), |index| index + 1);
    &remainder[..length]
}

fn input_records(input: &str) -> Result<Vec<&str>, FlexTextError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for record in input.split_inclusive('\n') {
        if records.len() == MAX_RECORDS {
            return Err(FlexTextError::TooManyRecords);
        }
        let record = record.strip_suffix('\n').unwrap_or(record);
        records.push(record.strip_suffix('\r').unwrap_or(record));
    }
    Ok(records)
}

fn parse_fixed_record(
    record: &str,
    fields: &[FixedWidthRecordField],
    path: &str,
    state: &mut ParseState,
) -> Result<Instance, FlexTextError> {
    let characters = record.chars().collect::<Vec<_>>();
    let expected: usize = fields
        .iter()
        .map(|field| field.width().get() as usize)
        .sum();
    if characters.len() != expected {
        return Err(data_error(
            path,
            format!(
                "fixed-width record expected {expected} character(s), got {}",
                characters.len()
            ),
        ));
    }
    let mut offset = 0;
    let mut values = Vec::with_capacity(fields.len());
    for field in fields {
        let width = field.width().get() as usize;
        let raw = characters[offset..offset + width]
            .iter()
            .collect::<String>();
        let raw = match field.ty() {
            ScalarType::String => raw.trim_end_matches(' '),
            _ => raw.trim_matches(' '),
        };
        let field_path = join_path(path, field.name());
        let value = parse_value(raw, field.ty(), true, &field_path)?;
        values.push((field.name().to_string(), Instance::Scalar(value)));
        offset += width;
    }
    state.add_nodes(fields.len() + 1)?;
    Ok(Instance::Group(values))
}

fn parse_delimited(
    input: &str,
    dialect: &DelimitedDialect,
    path: &str,
) -> Result<Vec<Vec<String>>, FlexTextError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut index = 0;
    let mut quoted = false;
    while index < input.len() {
        let rest = &input[index..];
        let character = rest
            .chars()
            .next()
            .ok_or_else(|| data_error(path, "invalid character boundary"))?;
        let length = character.len_utf8();
        if quoted {
            if character == dialect.escape() {
                let after_escape = index + length;
                if dialect.escape() == dialect.quote()
                    && input[after_escape..].starts_with(dialect.quote())
                {
                    field.push(dialect.quote());
                    index = after_escape + dialect.quote().len_utf8();
                    continue;
                }
                if dialect.escape() != dialect.quote()
                    && let Some(escaped) = input[after_escape..].chars().next()
                {
                    field.push(escaped);
                    index = after_escape + escaped.len_utf8();
                    continue;
                }
            }
            if character == dialect.quote() {
                quoted = false;
            } else {
                field.push(character);
            }
            index += length;
            continue;
        }
        if character == dialect.quote() && field.is_empty() {
            quoted = true;
            index += length;
        } else if character == dialect.field_separator() {
            record.push(std::mem::take(&mut field));
            index += length;
        } else if let Some(separator_len) = record_separator_len(rest, dialect.record_separator()) {
            record.push(std::mem::take(&mut field));
            if records.len() == MAX_RECORDS {
                return Err(FlexTextError::TooManyRecords);
            }
            records.push(std::mem::take(&mut record));
            index += separator_len;
        } else {
            field.push(character);
            index += length;
        }
    }
    if quoted {
        return Err(data_error(path, "unterminated quoted field"));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        if records.len() == MAX_RECORDS {
            return Err(FlexTextError::TooManyRecords);
        }
        records.push(record);
    }
    Ok(records)
}

fn record_separator_len(input: &str, configured: &str) -> Option<usize> {
    if matches!(configured, "\n" | "\r\n") {
        input
            .starts_with("\r\n")
            .then_some(2)
            .or_else(|| input.starts_with('\n').then_some(1))
    } else {
        input.starts_with(configured).then_some(configured.len())
    }
}

fn parse_delimited_record(
    record: Vec<String>,
    fields: &[DelimitedRecordField],
    path: &str,
    state: &mut ParseState,
) -> Result<Instance, FlexTextError> {
    if record.len() != fields.len() {
        return Err(data_error(
            path,
            format!(
                "delimited record expected {} field(s), got {}",
                fields.len(),
                record.len()
            ),
        ));
    }
    let values = fields
        .iter()
        .zip(record)
        .map(|(field, raw)| {
            let value = parse_value(&raw, field.ty(), true, &join_path(path, field.name()))?;
            Ok((field.name().to_string(), Instance::Scalar(value)))
        })
        .collect::<Result<Vec<_>, FlexTextError>>()?;
    state.add_nodes(fields.len() + 1)?;
    Ok(Instance::Group(values))
}

fn render_command(
    command: &FlexCommand,
    value: &Instance,
    path: &str,
    depth: usize,
) -> Result<String, FlexTextError> {
    if depth > MAX_INSTANCE_DEPTH {
        return Err(FlexTextError::InstanceTooDeep);
    }
    match command {
        FlexCommand::Ignore => Ok(String::new()),
        FlexCommand::Store { ty, .. } => render_scalar(value, *ty, path),
        FlexCommand::SplitOnce {
            splitter,
            first,
            second,
            ..
        } => render_split_once(value, splitter, first, second, path, depth),
        FlexCommand::SplitMany {
            splitter, child, ..
        } => render_split_many(value, splitter, child, path, depth),
        FlexCommand::FixedWidthRecords { fields, .. } => render_fixed_records(value, fields, path),
        FlexCommand::DelimitedRecords {
            dialect, fields, ..
        } => render_delimited_records(value, dialect, fields, path),
        FlexCommand::Switch { arms, default, .. } => {
            render_switch(value, arms, default.as_deref(), path, depth)
        }
    }
}

fn render_split_once(
    value: &Instance,
    splitter: &OnceSplitter,
    first: &FlexCommand,
    second: &FlexCommand,
    path: &str,
    depth: usize,
) -> Result<String, FlexTextError> {
    let fields = group_fields(value, path)?;
    reject_duplicate_fields(fields, path)?;
    reject_unexpected_fields(
        fields,
        [first.output_name(), second.output_name()]
            .into_iter()
            .flatten(),
        path,
    )?;
    let first_text = render_optional_command(first, fields, path, depth + 1)?;
    let second_text = render_optional_command(second, fields, path, depth + 1)?;
    match splitter {
        OnceSplitter::Delimiter(delimiter) => bounded_concat([
            first_text.as_str(),
            delimiter.as_str(),
            second_text.as_str(),
        ]),
        OnceSplitter::FixedColumns(width) => {
            let width = width.get() as usize;
            let got = first_text.chars().count();
            if got > width {
                return Err(data_error(
                    path,
                    format!("first split has {got} columns, exceeding {width}"),
                ));
            }
            let padding = " ".repeat(width - got);
            bounded_concat([first_text.as_str(), padding.as_str(), second_text.as_str()])
        }
        OnceSplitter::FixedLines(lines) => {
            let got = logical_line_count(&first_text);
            if got != lines.get() as usize {
                return Err(data_error(
                    path,
                    format!("first split expected {} line(s), got {got}", lines.get()),
                ));
            }
            join_line_chunks(&first_text, &second_text)
        }
        OnceSplitter::LineStartingWith(marker) => {
            if !second_text.is_empty() && !second_text.starts_with(marker) {
                return Err(data_error(
                    path,
                    format!("second split must start with marker `{marker}`"),
                ));
            }
            join_line_chunks(&first_text, &second_text)
        }
        OnceSplitter::LineContaining(marker) => {
            if !second_text.is_empty() && !line_at(&second_text, 0).contains(marker) {
                return Err(data_error(
                    path,
                    format!("second split's first line must contain marker `{marker}`"),
                ));
            }
            join_line_chunks(&first_text, &second_text)
        }
    }
}

fn render_split_many(
    value: &Instance,
    splitter: &ManySplitter,
    child: &FlexCommand,
    path: &str,
    depth: usize,
) -> Result<String, FlexTextError> {
    let items = repeated_items(value, path)?;
    if items.len() > MAX_RECORDS {
        return Err(FlexTextError::TooManyRecords);
    }
    let mut output = String::new();
    for (index, item) in items.iter().enumerate() {
        let item_path = indexed_path(path, index);
        let fields = group_fields(item, &item_path)?;
        let child_value = required_command_value(fields, child, &item_path)?;
        let text = render_command(child, child_value, &item_path, depth + 1)?;
        match splitter {
            ManySplitter::FixedLines(lines) => {
                let got = logical_line_count(&text);
                if got != lines.get() as usize {
                    return Err(data_error(
                        &item_path,
                        format!("split item expected {} line(s), got {got}", lines.get()),
                    ));
                }
            }
            ManySplitter::LinesStartingWith(marker) if !text.starts_with(marker) => {
                return Err(data_error(
                    &item_path,
                    format!("split item must start with marker `{marker}`"),
                ));
            }
            ManySplitter::LinesStartingWith(_) => {}
        }
        if !output.is_empty() && !output.ends_with('\n') {
            push_bounded(&mut output, "\n")?;
        }
        push_bounded(&mut output, &text)?;
    }
    Ok(output)
}

fn render_fixed_records(
    value: &Instance,
    fields: &[FixedWidthRecordField],
    path: &str,
) -> Result<String, FlexTextError> {
    let items = repeated_items(value, path)?;
    if items.len() > MAX_RECORDS {
        return Err(FlexTextError::TooManyRecords);
    }
    let mut records = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let item_path = indexed_path(path, index);
        let values =
            checked_record_fields(item, fields.iter().map(|field| field.name()), &item_path)?;
        let mut record = String::new();
        for field in fields {
            let field_path = join_path(&item_path, field.name());
            let raw = render_scalar(values[field.name()], field.ty(), &field_path)?;
            let width = field.width().get() as usize;
            let got = raw.chars().count();
            if got > width {
                return Err(data_error(
                    &field_path,
                    format!("value has {got} characters, exceeding field width {width}"),
                ));
            }
            let padding = " ".repeat(width - got);
            if field.ty() == ScalarType::String {
                push_bounded(&mut record, &raw)?;
                push_bounded(&mut record, &padding)?;
            } else {
                push_bounded(&mut record, &padding)?;
                push_bounded(&mut record, &raw)?;
            }
        }
        records.push(record);
    }
    bounded_join(&records, "\n")
}

fn render_delimited_records(
    value: &Instance,
    dialect: &DelimitedDialect,
    fields: &[DelimitedRecordField],
    path: &str,
) -> Result<String, FlexTextError> {
    let items = repeated_items(value, path)?;
    if items.len() > MAX_RECORDS {
        return Err(FlexTextError::TooManyRecords);
    }
    let mut records = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let item_path = indexed_path(path, index);
        let values =
            checked_record_fields(item, fields.iter().map(|field| field.name()), &item_path)?;
        let mut encoded = Vec::with_capacity(fields.len());
        for field in fields {
            let raw = render_scalar(
                values[field.name()],
                field.ty(),
                &join_path(&item_path, field.name()),
            )?;
            encoded.push(quote_field(&raw, dialect)?);
        }
        let separator = dialect.field_separator().to_string();
        records.push(bounded_join(&encoded, &separator)?);
    }
    bounded_join(&records, dialect.record_separator())
}

fn render_switch(
    value: &Instance,
    arms: &[mapping::SwitchArm],
    default: Option<&FlexCommand>,
    path: &str,
    depth: usize,
) -> Result<String, FlexTextError> {
    let fields = group_fields(value, path)?;
    reject_duplicate_fields(fields, path)?;
    let allowed = arms
        .iter()
        .filter_map(|arm| arm.command().output_name())
        .chain(default.and_then(FlexCommand::output_name));
    reject_unexpected_fields(fields, allowed, path)?;
    let mut output = String::new();
    let mut selected = false;
    for arm in arms {
        if let Some(name) = arm.command().output_name()
            && let Some(value) = find_field(fields, name)
        {
            selected = true;
            push_bounded(
                &mut output,
                &render_command(arm.command(), value, &join_path(path, name), depth + 1)?,
            )?;
        }
    }
    if !selected
        && let Some(default) = default
        && let Some(name) = default.output_name()
        && let Some(value) = find_field(fields, name)
    {
        push_bounded(
            &mut output,
            &render_command(default, value, &join_path(path, name), depth + 1)?,
        )?;
    }
    Ok(output)
}

fn render_optional_command(
    command: &FlexCommand,
    fields: &[(String, Instance)],
    path: &str,
    depth: usize,
) -> Result<String, FlexTextError> {
    let Some(name) = command.output_name() else {
        return Ok(String::new());
    };
    let value = find_field(fields, name)
        .ok_or_else(|| data_error(path, format!("missing field `{name}`")))?;
    render_command(command, value, &join_path(path, name), depth)
}

fn render_scalar(instance: &Instance, ty: ScalarType, path: &str) -> Result<String, FlexTextError> {
    let Instance::Scalar(value) = instance else {
        return Err(data_error(path, "expected a scalar"));
    };
    let rendered = match (ty, value) {
        (_, Value::Null) => String::new(),
        (ScalarType::String, Value::String(value)) => value.clone(),
        (ScalarType::Int, Value::Int(value)) => value.to_string(),
        (ScalarType::Int, Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| data_error(path, "expected an Int scalar"))?,
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => value.to_string(),
        (ScalarType::Float, Value::Int(value)) => value.to_string(),
        (ScalarType::Float, Value::String(value)) => {
            let parsed = value
                .trim()
                .parse::<f64>()
                .map_err(|_| data_error(path, "expected a Float scalar"))?;
            if !parsed.is_finite() {
                return Err(data_error(path, "expected a finite Float scalar"));
            }
            parsed.to_string()
        }
        (ScalarType::Bool, Value::Bool(value)) => value.to_string(),
        (ScalarType::Bool, Value::String(value)) => value
            .trim()
            .parse::<bool>()
            .map(|value| value.to_string())
            .map_err(|_| data_error(path, "expected a Bool scalar"))?,
        _ => return Err(data_error(path, format!("expected a {ty:?} scalar"))),
    };
    if rendered.len() > MAX_VALUE_BYTES {
        Err(FlexTextError::ValueTooLarge {
            path: path.to_string(),
        })
    } else {
        Ok(rendered)
    }
}

fn quote_field(value: &str, dialect: &DelimitedDialect) -> Result<String, FlexTextError> {
    let needs_quotes = value.contains(dialect.field_separator())
        || value.contains(dialect.record_separator())
        || value.contains(dialect.quote())
        || value.contains('\n')
        || value.contains('\r');
    if !needs_quotes {
        return Ok(value.to_string());
    }
    let mut output = String::new();
    output.push(dialect.quote());
    for character in value.chars() {
        if character == dialect.quote() || character == dialect.escape() {
            output.push(dialect.escape());
        }
        output.push(character);
    }
    output.push(dialect.quote());
    if output.len() > MAX_OUTPUT_BYTES {
        Err(FlexTextError::OutputTooLarge)
    } else {
        Ok(output)
    }
}

fn checked_record_fields<'a, 'b>(
    instance: &'a Instance,
    expected: impl Iterator<Item = &'b str>,
    path: &str,
) -> Result<HashMap<&'a str, &'a Instance>, FlexTextError> {
    let fields = group_fields(instance, path)?;
    reject_duplicate_fields(fields, path)?;
    let expected = expected.collect::<HashSet<_>>();
    reject_unexpected_fields(fields, expected.iter().copied(), path)?;
    let values = fields
        .iter()
        .map(|(name, value)| (name.as_str(), value))
        .collect::<HashMap<_, _>>();
    for name in expected {
        if !values.contains_key(name) {
            return Err(data_error(path, format!("missing field `{name}`")));
        }
    }
    Ok(values)
}

fn required_command_value<'a>(
    fields: &'a [(String, Instance)],
    command: &FlexCommand,
    path: &str,
) -> Result<&'a Instance, FlexTextError> {
    let name = command
        .output_name()
        .ok_or_else(|| data_error(path, "ignore command cannot own a value"))?;
    find_field(fields, name).ok_or_else(|| data_error(path, format!("missing field `{name}`")))
}

fn group_fields<'a>(
    instance: &'a Instance,
    path: &str,
) -> Result<&'a [(String, Instance)], FlexTextError> {
    match instance {
        Instance::Group(fields) => Ok(fields),
        _ => Err(data_error(path, "expected a group")),
    }
}

fn repeated_items<'a>(instance: &'a Instance, path: &str) -> Result<&'a [Instance], FlexTextError> {
    match instance {
        Instance::Repeated(items) => Ok(items),
        _ => Err(data_error(path, "expected a repeated sequence")),
    }
}

fn find_field<'a>(fields: &'a [(String, Instance)], name: &str) -> Option<&'a Instance> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn reject_duplicate_fields(fields: &[(String, Instance)], path: &str) -> Result<(), FlexTextError> {
    let mut names = HashSet::new();
    for (name, _) in fields {
        if !names.insert(name) {
            return Err(data_error(path, format!("duplicate field `{name}`")));
        }
    }
    Ok(())
}

fn reject_unexpected_fields<'a>(
    fields: &[(String, Instance)],
    expected: impl Iterator<Item = &'a str>,
    path: &str,
) -> Result<(), FlexTextError> {
    let expected = expected.collect::<HashSet<_>>();
    for (name, _) in fields {
        if !expected.contains(name.as_str()) {
            return Err(data_error(path, format!("unexpected field `{name}`")));
        }
    }
    Ok(())
}

fn validate_instance_bounds(
    instance: &Instance,
    path: &str,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), FlexTextError> {
    if depth > MAX_INSTANCE_DEPTH {
        return Err(FlexTextError::InstanceTooDeep);
    }
    *nodes = nodes.checked_add(1).ok_or(FlexTextError::TooManyNodes)?;
    if *nodes > MAX_INSTANCE_NODES {
        return Err(FlexTextError::TooManyNodes);
    }
    match instance {
        Instance::Scalar(Value::String(value)) if value.len() > MAX_VALUE_BYTES => {
            Err(FlexTextError::ValueTooLarge {
                path: path.to_string(),
            })
        }
        Instance::Scalar(_) => Ok(()),
        Instance::Group(fields) => {
            for (name, value) in fields {
                validate_instance_bounds(value, &join_path(path, name), depth + 1, nodes)?;
            }
            Ok(())
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            if items.len() > MAX_RECORDS {
                return Err(FlexTextError::TooManyRecords);
            }
            for (index, value) in items.iter().enumerate() {
                validate_instance_bounds(value, &indexed_path(path, index), depth + 1, nodes)?;
            }
            Ok(())
        }
    }
}

fn logical_line_count(value: &str) -> usize {
    if value.is_empty() {
        0
    } else {
        value.matches('\n').count() + usize::from(!value.ends_with('\n'))
    }
}

fn normalize_line_endings(input: &str, ending: &str) -> Result<String, FlexTextError> {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('\n') {
        let before = &rest[..index];
        push_bounded(&mut output, before.strip_suffix('\r').unwrap_or(before))?;
        push_bounded(&mut output, ending)?;
        rest = &rest[index + 1..];
    }
    push_bounded(&mut output, rest)?;
    Ok(output)
}

fn bounded_concat<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<String, FlexTextError> {
    let mut output = String::new();
    for value in values {
        push_bounded(&mut output, value)?;
    }
    Ok(output)
}

fn join_line_chunks(first: &str, second: &str) -> Result<String, FlexTextError> {
    if first.is_empty() || second.is_empty() || first.ends_with('\n') {
        bounded_concat([first, second])
    } else {
        bounded_concat([first, "\n", second])
    }
}

fn bounded_join(values: &[String], separator: &str) -> Result<String, FlexTextError> {
    let mut output = String::new();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            push_bounded(&mut output, separator)?;
        }
        push_bounded(&mut output, value)?;
    }
    Ok(output)
}

fn push_bounded(output: &mut String, value: &str) -> Result<(), FlexTextError> {
    let length = output
        .len()
        .checked_add(value.len())
        .ok_or(FlexTextError::OutputTooLarge)?;
    if length > MAX_OUTPUT_BYTES {
        return Err(FlexTextError::OutputTooLarge);
    }
    output.push_str(value);
    Ok(())
}

fn join_path(path: &str, name: &str) -> String {
    format!("{path}/{name}")
}

fn indexed_path(path: &str, index: usize) -> String {
    format!("{path}[{}]", index + 1)
}

fn data_error(path: &str, message: impl Into<String>) -> FlexTextError {
    FlexTextError::Data {
        path: path.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests;
