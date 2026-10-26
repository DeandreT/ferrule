//! Whole-document admission without expanding the input into character vectors.
use ir::{Instance, SchemaNode};
use mapping::FixedWidthLayout;
use thiserror::Error;

use crate::{
    CsvFormatError, MAX_CSV_INPUT_BYTES, MAX_CSV_INPUT_CELL_BYTES, MAX_CSV_INPUT_COLUMNS,
    MAX_CSV_INPUT_NODES, MAX_CSV_INPUT_RECORDS, fixed_width,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedWidthReadLimit {
    InputBytes,
    CellBytes,
    Records,
    Columns,
    Nodes,
}

#[derive(Debug, Error)]
pub enum FixedWidthReadError {
    #[error(transparent)]
    Format(#[from] CsvFormatError),
    #[error("fixed-width input bytes must be valid UTF-8: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error(
        "fixed-width input {kind:?} exceeds {maximum} (observed {observed}) at record {record:?}, column {column:?}"
    )]
    Limit {
        kind: FixedWidthReadLimit,
        maximum: usize,
        observed: usize,
        record: Option<usize>,
        column: Option<usize>,
    },
}

fn limit(
    kind: FixedWidthReadLimit,
    maximum: usize,
    observed: usize,
    record: Option<usize>,
    column: Option<usize>,
) -> Result<(), FixedWidthReadError> {
    if observed > maximum {
        return Err(FixedWidthReadError::Limit {
            kind,
            maximum,
            observed,
            record,
            column,
        });
    }
    Ok(())
}

/// Parse complete UTF-8 fixed-width input with the same byte, cell, record,
/// column and node ceilings as bounded CSV. Cell bytes include trailing fill;
/// widths still count Unicode scalars, not UTF-8 bytes. No partial rows escape.
pub fn from_bytes_fixed_width_bounded(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
) -> Result<Vec<Instance>, FixedWidthReadError> {
    limit(
        FixedWidthReadLimit::InputBytes,
        MAX_CSV_INPUT_BYTES,
        bytes.len(),
        None,
        None,
    )?;
    from_str_fixed_width_bounded(std::str::from_utf8(bytes)?, schema, layout)
}

/// Text companion to [`from_bytes_fixed_width_bounded`]. A leading BOM counts
/// toward the byte ceiling. Empty and BOM-only documents are valid empty sets.
pub fn from_str_fixed_width_bounded(
    text: &str,
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
) -> Result<Vec<Instance>, FixedWidthReadError> {
    limit(
        FixedWidthReadLimit::InputBytes,
        MAX_CSV_INPUT_BYTES,
        text.len(),
        None,
        None,
    )?;
    let fields = fixed_width::checked_fields(schema, layout)?;
    limit(
        FixedWidthReadLimit::Columns,
        MAX_CSV_INPUT_COLUMNS,
        fields.len(),
        None,
        None,
    )?;
    if fields.iter().any(|(name, _)| name.is_empty())
        || fields
            .iter()
            .map(|(name, _)| name)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != fields.len()
    {
        return Err(CsvFormatError::UnsupportedSchema.into());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let record_width = layout.record_width();
    let mut nodes = 1usize;
    // The native reader validates every record width before converting values.
    // Keep that error precedence without retaining a document-sized char tree.
    visit_records(text, layout, |record, raw| {
        limit(
            FixedWidthReadLimit::Records,
            MAX_CSV_INPUT_RECORDS,
            record + 1,
            Some(record),
            None,
        )?;
        fixed_width::check_record_width(record, raw.chars().count(), record_width)?;
        nodes += 1 + fields.len();
        limit(
            FixedWidthReadLimit::Nodes,
            MAX_CSV_INPUT_NODES,
            nodes,
            Some(record),
            None,
        )?;
        for (column, cell) in cells(raw, layout).enumerate() {
            limit(
                FixedWidthReadLimit::CellBytes,
                MAX_CSV_INPUT_CELL_BYTES,
                cell.len(),
                Some(record),
                Some(column),
            )?;
        }
        Ok(())
    })?;
    let mut rows = Vec::new();
    visit_records(text, layout, |record, raw| {
        let mut values = Vec::with_capacity(fields.len());
        for ((name, ty), cell) in fields.iter().zip(cells(raw, layout)) {
            values.push((
                (*name).to_owned(),
                Instance::Scalar(fixed_width::parse_field(record, name, *ty, cell, layout)?),
            ));
        }
        rows.push(Instance::Group(values.into()));
        Ok(())
    })?;
    Ok(rows)
}

fn cells<'a>(raw: &'a str, layout: &'a FixedWidthLayout) -> impl Iterator<Item = &'a str> {
    layout.field_widths().iter().scan(raw, |remaining, width| {
        let end = remaining
            .char_indices()
            .nth(width.get() as usize)
            .map_or(remaining.len(), |(offset, _)| offset);
        let (cell, rest) = remaining.split_at(end);
        *remaining = rest;
        Some(cell)
    })
}

fn visit_records(
    text: &str,
    layout: &FixedWidthLayout,
    mut visit: impl FnMut(usize, &str) -> Result<(), FixedWidthReadError>,
) -> Result<(), FixedWidthReadError> {
    if layout.record_delimiters() {
        for (record, raw) in text.split_inclusive('\n').enumerate() {
            let raw = raw
                .strip_suffix('\n')
                .map_or(raw, |raw| raw.strip_suffix('\r').unwrap_or(raw));
            visit(record, raw)?;
        }
    } else {
        let record_width = layout.record_width();
        let mut start = 0;
        let mut scalars = 0;
        let mut record = 0;
        for (offset, ch) in text.char_indices() {
            scalars += 1;
            if scalars == record_width {
                let end = offset + ch.len_utf8();
                visit(record, &text[start..end])?;
                record += 1;
                start = end;
                scalars = 0;
            }
        }
        if start < text.len() {
            visit(record, &text[start..])?;
        }
    }
    Ok(())
}
