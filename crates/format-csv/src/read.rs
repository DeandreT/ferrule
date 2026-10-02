use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{CsvTextRepairDependency, FormatOptions};

use crate::{CsvFormatError, dialect_bytes, parse_present_value, row_fields};

/// CSV input dialect and presence policy. Numeric and boolean empty cells
/// remain null; only physically present text cells can become empty strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvReadOptions {
    pub delimiter: Option<char>,
    pub quote: Option<char>,
    pub quote_disabled: bool,
    pub has_headers: bool,
    pub preserve_empty_strings: bool,
    pub repair_dependency: Option<CsvTextRepairDependency>,
}

impl Default for CsvReadOptions {
    fn default() -> Self {
        Self {
            delimiter: None,
            quote: None,
            quote_disabled: false,
            has_headers: true,
            preserve_empty_strings: false,
            repair_dependency: None,
        }
    }
}

impl From<&FormatOptions> for CsvReadOptions {
    fn from(options: &FormatOptions) -> Self {
        Self {
            delimiter: options.delimiter,
            quote: options.csv_quote,
            quote_disabled: options.csv_quote_disabled,
            has_headers: options.has_header_row.unwrap_or(true),
            preserve_empty_strings: options.csv_preserve_empty_strings,
            repair_dependency: options.csv_text_repair_dependency,
        }
    }
}

/// Reads a CSV file into one [`Instance::Group`] per row, parsing each
/// column according to its declared scalar type (columns are positional;
/// when `has_headers` the first row is skipped). Missing trailing columns
/// are represented as [`Value::Null`]. `delimiter` defaults to `,`.
pub fn read(
    path: &Path,
    schema: &SchemaNode,
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    read_with_quote(path, schema, delimiter, None, has_headers)
}

/// Read CSV rows with an explicit single-byte quote character.
pub fn read_with_quote(
    path: &Path,
    schema: &SchemaNode,
    delimiter: Option<char>,
    quote: Option<char>,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    read_with_dialect(path, schema, delimiter, quote, false, has_headers)
}

/// Read CSV rows with optional quote recognition disabled.
pub fn read_with_dialect(
    path: &Path,
    schema: &SchemaNode,
    delimiter: Option<char>,
    quote: Option<char>,
    quote_disabled: bool,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    read_with_options(
        path,
        schema,
        &CsvReadOptions {
            delimiter,
            quote,
            quote_disabled,
            has_headers,
            ..CsvReadOptions::default()
        },
    )
}

/// Read CSV rows with explicit dialect and empty-text policy.
pub fn read_with_options(
    path: &Path,
    schema: &SchemaNode,
    options: &CsvReadOptions,
) -> Result<Vec<Instance>, CsvFormatError> {
    crate::require_executable_dependency(options.repair_dependency)?;
    let fields = row_fields(schema)?;
    let (delimiter, quote) =
        dialect_bytes(options.delimiter, options.quote, options.quote_disabled)?;
    let reader = csv::ReaderBuilder::new()
        .has_headers(options.has_headers)
        .flexible(true)
        .delimiter(delimiter)
        .quote(quote.unwrap_or(b'"'))
        .quoting(quote.is_some())
        .from_path(path)?;
    read_records(reader, &fields, options.preserve_empty_strings)
}

/// Reads CSV text into one [`Instance::Group`] per row.
///
/// This is the in-memory equivalent of [`read`], suitable for hosts without
/// filesystem access such as WebAssembly applications.
pub fn from_str(
    text: &str,
    schema: &SchemaNode,
    delimiter: Option<char>,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    from_str_with_quote(text, schema, delimiter, None, has_headers)
}

/// Parse CSV text with an explicit single-byte quote character.
pub fn from_str_with_quote(
    text: &str,
    schema: &SchemaNode,
    delimiter: Option<char>,
    quote: Option<char>,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    from_str_with_dialect(text, schema, delimiter, quote, false, has_headers)
}

/// Parse CSV text with optional quote recognition disabled.
pub fn from_str_with_dialect(
    text: &str,
    schema: &SchemaNode,
    delimiter: Option<char>,
    quote: Option<char>,
    quote_disabled: bool,
    has_headers: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    from_str_with_options(
        text,
        schema,
        &CsvReadOptions {
            delimiter,
            quote,
            quote_disabled,
            has_headers,
            ..CsvReadOptions::default()
        },
    )
}

/// Parse CSV text with explicit dialect and empty-text policy.
pub fn from_str_with_options(
    text: &str,
    schema: &SchemaNode,
    options: &CsvReadOptions,
) -> Result<Vec<Instance>, CsvFormatError> {
    crate::require_executable_dependency(options.repair_dependency)?;
    let fields = row_fields(schema)?;
    let (delimiter, quote) =
        dialect_bytes(options.delimiter, options.quote, options.quote_disabled)?;
    let reader = csv::ReaderBuilder::new()
        .has_headers(options.has_headers)
        .flexible(true)
        .delimiter(delimiter)
        .quote(quote.unwrap_or(b'"'))
        .quoting(quote.is_some())
        .from_reader(text.as_bytes());
    read_records(reader, &fields, options.preserve_empty_strings)
}

fn read_records<R: std::io::Read>(
    mut reader: csv::Reader<R>,
    fields: &[(&str, ScalarType)],
    preserve_empty_strings: bool,
) -> Result<Vec<Instance>, CsvFormatError> {
    let mut out = Vec::new();
    for (row_idx, result) in reader.records().enumerate() {
        let raw = result?;
        if raw.len() > fields.len() {
            return Err(CsvFormatError::ColumnCount {
                row: row_idx,
                expected: fields.len(),
                got: raw.len(),
            });
        }
        let mut row = Vec::with_capacity(fields.len());
        for (column, (name, ty)) in fields.iter().enumerate() {
            let value = match raw.get(column) {
                None => Value::Null,
                Some("") if preserve_empty_strings && *ty == ScalarType::String => {
                    Value::String(String::new())
                }
                Some("") => Value::Null,
                Some(cell) => parse_present_value(name, *ty, cell, row_idx)?,
            };
            row.push((name.to_string(), Instance::Scalar(value)));
        }
        out.push(Instance::Group((row).into()));
    }
    Ok(out)
}
