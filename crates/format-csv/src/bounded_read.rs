//! Bounded in-memory CSV admission using the existing native parser and values.
use ir::{Instance, SchemaNode};
use thiserror::Error;

use crate::{CsvFormatError, CsvReadOptions};

pub const MAX_CSV_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CSV_INPUT_CELL_BYTES: usize = 1024 * 1024;
pub const MAX_CSV_INPUT_RECORDS: usize = 100_000;
pub const MAX_CSV_INPUT_COLUMNS: usize = 256;
pub const MAX_CSV_INPUT_NODES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsvReadLimit {
    InputBytes,
    DecodedBytes,
    CellBytes,
    Records,
    Columns,
    Nodes,
}

#[derive(Debug, Error)]
pub enum CsvReadError {
    #[error(transparent)]
    Format(#[from] CsvFormatError),
    #[error("CSV input bytes must be valid UTF-8: {0}")]
    Encoding(#[from] std::str::Utf8Error),
    #[error(
        "CSV input {kind:?} exceeds {maximum} (observed {observed}) at record {record:?}, column {column:?}"
    )]
    Limit {
        kind: CsvReadLimit,
        maximum: usize,
        observed: usize,
        record: Option<usize>,
        column: Option<usize>,
    },
}

fn limit(
    kind: CsvReadLimit,
    maximum: usize,
    observed: usize,
    record: Option<usize>,
    column: Option<usize>,
) -> Result<(), CsvReadError> {
    if observed > maximum {
        return Err(CsvReadError::Limit {
            kind,
            maximum,
            observed,
            record,
            column,
        });
    }
    Ok(())
}

/// Parse a complete UTF-8 document without filesystem access. The physical byte
/// limit includes a leading BOM; logical records include a skipped header.
/// No rows are returned on failure. Limits bound admitted data, not process memory.
pub fn from_bytes_with_options_bounded(
    bytes: &[u8],
    schema: &SchemaNode,
    options: &CsvReadOptions,
) -> Result<Vec<Instance>, CsvReadError> {
    limit(
        CsvReadLimit::InputBytes,
        MAX_CSV_INPUT_BYTES,
        bytes.len(),
        None,
        None,
    )?;
    from_str_with_options_bounded(std::str::from_utf8(bytes)?, schema, options)
}

/// An explicit byte-decoding companion. The non-flushing profile checks the
/// physical byte limit, decodes, checks the decoded UTF-8 byte limit, then uses
/// the unchanged bounded text parser. No rows are returned on any failure.
pub fn from_bytes_with_decoding_policy_bounded(
    bytes: &[u8],
    schema: &SchemaNode,
    options: &CsvReadOptions,
    policy: crate::CsvByteDecodingPolicy,
) -> Result<Vec<Instance>, CsvReadError> {
    if policy == crate::CsvByteDecodingPolicy::StrictUtf8 {
        return from_bytes_with_options_bounded(bytes, schema, options);
    }
    limit(
        CsvReadLimit::InputBytes,
        MAX_CSV_INPUT_BYTES,
        bytes.len(),
        None,
        None,
    )?;
    let text = crate::byte_decoding::decode_non_flushing(bytes);
    limit(
        CsvReadLimit::DecodedBytes,
        MAX_CSV_INPUT_BYTES,
        text.len(),
        None,
        None,
    )?;
    from_str_with_options_bounded(&text, schema, options)
}

/// Text form of the bounded CSV document reader. Uses the native positional,
/// permissive CSV dialect and preserves its typed conversion/row errors.
pub fn from_str_with_options_bounded(
    text: &str,
    schema: &SchemaNode,
    options: &CsvReadOptions,
) -> Result<Vec<Instance>, CsvReadError> {
    crate::require_executable_dependency(options.repair_dependency)?;
    let fields = crate::row_fields(schema)?;
    if fields.len() > MAX_CSV_INPUT_COLUMNS
        || fields.iter().any(|(name, _)| name.is_empty())
        || fields
            .iter()
            .map(|(name, _)| name)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != fields.len()
    {
        return Err(CsvFormatError::UnsupportedSchema.into());
    }
    let (delimiter, quote) =
        crate::dialect_bytes(options.delimiter, options.quote, options.quote_disabled)?;
    limit(
        CsvReadLimit::InputBytes,
        MAX_CSV_INPUT_BYTES,
        text.len(),
        None,
        None,
    )?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .quote(quote.unwrap_or(b'"'))
        .quoting(quote.is_some())
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    let mut nodes = 1;
    for (record, raw) in reader.records().enumerate() {
        let raw = raw.map_err(CsvFormatError::from)?;
        limit(
            CsvReadLimit::Columns,
            MAX_CSV_INPUT_COLUMNS,
            raw.len(),
            Some(record),
            None,
        )?;
        for (column, cell) in raw.iter().enumerate() {
            limit(
                CsvReadLimit::CellBytes,
                MAX_CSV_INPUT_CELL_BYTES,
                cell.len(),
                Some(record),
                Some(column),
            )?;
        }
        limit(
            CsvReadLimit::Records,
            MAX_CSV_INPUT_RECORDS,
            record + 1,
            Some(record),
            None,
        )?;
        if options.has_headers && record == 0 {
            continue;
        }
        if raw.len() > fields.len() {
            return Err(CsvFormatError::ColumnCount {
                row: rows.len(),
                expected: fields.len(),
                got: raw.len(),
            }
            .into());
        }
        nodes += 1 + fields.len();
        limit(
            CsvReadLimit::Nodes,
            MAX_CSV_INPUT_NODES,
            nodes,
            Some(record),
            None,
        )?;
        rows.push(crate::read::materialize_row(
            &raw,
            &fields,
            options.preserve_empty_strings,
            rows.len(),
        )?);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, Value};

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("count", ScalarType::Int),
            ],
        )
    }

    #[test]
    fn whole_document_preserves_quotes_bom_empty_and_missing_cells() {
        let options = CsvReadOptions {
            preserve_empty_strings: true,
            ..Default::default()
        };
        let rows = from_bytes_with_options_bounded(
            b"\xef\xbb\xbfN,C\r\n\"x,\"\"y\"\"\r\nz\",2\r\n,\r\nlast\r\n",
            &schema(),
            &options,
        )
        .unwrap();
        let row = |name, count| {
            Instance::Group(
                vec![
                    ("name".into(), Instance::Scalar(Value::String(name))),
                    ("count".into(), Instance::Scalar(count)),
                ]
                .into(),
            )
        };
        assert_eq!(
            rows,
            vec![
                row("x,\"y\"\r\nz".into(), Value::Int(2)),
                row("".into(), Value::Null),
                row("last".into(), Value::Null)
            ]
        );
        assert!(matches!(
            from_bytes_with_options_bounded(&[0xff], &schema(), &options),
            Err(CsvReadError::Encoding(_))
        ));
        assert!(matches!(
            from_str_with_options_bounded("n,c\nok,1\nlate,no\n", &schema(), &options),
            Err(CsvReadError::Format(CsvFormatError::Parse { row: 1, .. }))
        ));
    }

    #[test]
    fn decoded_utf8_cell_limit_is_inclusive_and_padding_counts_toward_nodes() {
        let options = CsvReadOptions {
            has_headers: false,
            ..Default::default()
        };
        let mut cell = "é".repeat(MAX_CSV_INPUT_CELL_BYTES / 2);
        assert_eq!(
            from_str_with_options_bounded(&cell, &schema(), &options)
                .unwrap()
                .len(),
            1
        );
        cell.push('a');
        assert!(
            matches!(from_str_with_options_bounded(&cell, &schema(), &options),
            Err(CsvReadError::Limit { kind: CsvReadLimit::CellBytes, observed, .. })
                if observed == MAX_CSV_INPUT_CELL_BYTES + 1)
        );
        let wide = SchemaNode::group(
            "Row",
            (0..MAX_CSV_INPUT_COLUMNS)
                .map(|i| SchemaNode::scalar(format!("c{i}"), ScalarType::String))
                .collect(),
        );
        let rows = (MAX_CSV_INPUT_NODES - 1) / (1 + MAX_CSV_INPUT_COLUMNS);
        let mut text = "x\n".repeat(rows);
        assert_eq!(
            from_str_with_options_bounded(&text, &wide, &options)
                .unwrap()
                .len(),
            rows
        );
        text.push_str("x\n");
        assert!(
            matches!(from_str_with_options_bounded(&text, &wide, &options),
            Err(CsvReadError::Limit { kind: CsvReadLimit::Nodes, record: Some(record), .. })
                if record == rows)
        );
    }

    #[test]
    fn input_and_header_limits_are_not_bypassed_by_skipping_rows() {
        let options = CsvReadOptions::default();
        assert!(matches!(
            from_bytes_with_options_bounded(
                &vec![b' '; MAX_CSV_INPUT_BYTES + 1],
                &schema(),
                &options
            ),
            Err(CsvReadError::Limit {
                kind: CsvReadLimit::InputBytes,
                ..
            })
        ));
        assert!(matches!(
            from_str_with_options_bounded(
                &"x".repeat(MAX_CSV_INPUT_CELL_BYTES + 1),
                &schema(),
                &options
            ),
            Err(CsvReadError::Limit {
                kind: CsvReadLimit::CellBytes,
                record: Some(0),
                ..
            })
        ));
        assert!(matches!(
            from_str_with_options_bounded(&",".repeat(MAX_CSV_INPUT_COLUMNS), &schema(), &options),
            Err(CsvReadError::Limit {
                kind: CsvReadLimit::Columns,
                ..
            })
        ));
        assert!(matches!(
            from_str_with_options_bounded(
                &"a\n".repeat(MAX_CSV_INPUT_RECORDS + 1),
                &schema(),
                &options
            ),
            Err(CsvReadError::Limit {
                kind: CsvReadLimit::Records,
                ..
            })
        ));
    }
}
