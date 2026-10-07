use std::fmt;
use std::io::{self, Write};

use ir::{Instance, ScalarType, SchemaNode};

use super::{
    CsvFormatError, CsvWriteOptions, dialect_bytes, encoding::RecordEncoder, format_row,
    require_executable_dependency, requires_quoting, row_fields,
};

/// Failure from bounded CSV output. Native format errors retain their cause.
#[derive(Debug)]
pub enum CsvBoundedError {
    Format(CsvFormatError),
    OutputTooLarge { maximum: usize },
}

impl fmt::Display for CsvBoundedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(formatter),
            Self::OutputTooLarge { maximum } => {
                write!(formatter, "CSV output exceeds the {maximum}-byte maximum")
            }
        }
    }
}

impl std::error::Error for CsvBoundedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::OutputTooLarge { .. } => None,
        }
    }
}

impl From<CsvFormatError> for CsvBoundedError {
    fn from(error: CsvFormatError) -> Self {
        Self::Format(error)
    }
}

#[derive(Debug)]
struct OutputLimit {
    maximum: usize,
}

impl fmt::Display for OutputLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CSV output exceeds the {}-byte maximum",
            self.maximum
        )
    }
}

impl std::error::Error for OutputLimit {}

struct BoundedBytes {
    bytes: Vec<u8>,
    maximum: usize,
    failed: bool,
}

impl BoundedBytes {
    fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum,
            failed: false,
        }
    }

    fn limit_error(&self) -> io::Error {
        io::Error::other(OutputLimit {
            maximum: self.maximum,
        })
    }
}

impl Write for BoundedBytes {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(self.limit_error());
        }
        let fits = self
            .bytes
            .len()
            .checked_add(buffer.len())
            .is_some_and(|length| length <= self.maximum);
        if !fits {
            self.failed = true;
            return Err(self.limit_error());
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bounded_io_error(error: io::Error) -> CsvBoundedError {
    if let Some(limit) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<OutputLimit>())
    {
        CsvBoundedError::OutputTooLarge {
            maximum: limit.maximum,
        }
    } else {
        CsvBoundedError::Format(CsvFormatError::Io(error))
    }
}

pub(super) struct ValidatedCsv<'a> {
    pub(super) fields: Vec<(&'a str, ScalarType)>,
    pub(super) delimiter: u8,
    pub(super) quote: Option<u8>,
}

pub(super) fn validate_csv<'a>(
    schema: &'a SchemaNode,
    rows: &[Instance],
    options: &CsvWriteOptions,
) -> Result<ValidatedCsv<'a>, CsvFormatError> {
    require_executable_dependency(options.repair_dependency)?;
    let fields = row_fields(schema)?;
    let (delimiter, quote) =
        dialect_bytes(options.delimiter, options.quote, options.quote_disabled)?;
    let mut unquoted_error = None;
    if options.quote_disabled && options.has_headers {
        for (name, _) in &fields {
            if requires_quoting(name, delimiter) {
                unquoted_error = Some(CsvFormatError::UnquotedHeaderBoundary {
                    field: (*name).to_string(),
                });
                break;
            }
        }
    }
    for (row, instance) in rows.iter().enumerate() {
        // Use the native row shape, field order, and lexical rules, then
        // release this record before validating the next row.
        let record = format_row(row, instance, &fields)?;
        if options.quote_disabled && unquoted_error.is_none() {
            if record.len() == 1 && record[0].is_empty() {
                unquoted_error = Some(CsvFormatError::UnquotedSingleEmptyRow { row });
            } else {
                for ((field, _), value) in fields.iter().zip(&record) {
                    if requires_quoting(value, delimiter) {
                        unquoted_error = Some(CsvFormatError::UnquotedFieldBoundary {
                            row,
                            field: (*field).to_string(),
                        });
                        break;
                    }
                }
            }
        }
    }
    // Every shape/type error in every row has priority over a disabled-quote
    // error. Retain only the first header/row error until validation completes.
    if let Some(error) = unquoted_error {
        return Err(error);
    }
    Ok(ValidatedCsv {
        fields,
        delimiter,
        quote,
    })
}

/// Serialize CSV using the native row/dialect rules and a UTF-8 byte ceiling.
/// Every row is validated before BOM, headers, or records are written. The
/// ceiling bounds output bytes, not the allocated input tree or one formatted row.
pub fn to_bytes_with_options_bounded(
    schema: &SchemaNode,
    rows: &[Instance],
    options: &CsvWriteOptions,
    maximum: usize,
) -> Result<Vec<u8>, CsvBoundedError> {
    let ValidatedCsv {
        fields,
        delimiter,
        quote,
    } = validate_csv(schema, rows, options)?;
    let mut sink = BoundedBytes::new(maximum);
    if options.utf8_bom {
        sink.write_all(b"\xef\xbb\xbf").map_err(bounded_io_error)?;
    }
    let mut writer = RecordEncoder::new(delimiter, quote, options.quote_disabled);
    if options.has_headers {
        writer
            .write_record(&mut sink, fields.iter().map(|(name, _)| *name))
            .map_err(bounded_io_error)?;
    }
    for (row, instance) in rows.iter().enumerate() {
        let record = format_row(row, instance, &fields)?;
        writer
            .write_record(&mut sink, record.iter().map(String::as_str))
            .map_err(bounded_io_error)?;
    }
    Ok(sink.bytes)
}

/// Serialize bounded native CSV text without copying the successful byte buffer.
pub fn to_string_with_options_bounded(
    schema: &SchemaNode,
    rows: &[Instance],
    options: &CsvWriteOptions,
    maximum: usize,
) -> Result<String, CsvBoundedError> {
    let bytes = to_bytes_with_options_bounded(schema, rows, options, maximum)?;
    String::from_utf8(bytes).map_err(|error| {
        CsvBoundedError::Format(CsvFormatError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            error,
        )))
    })
}
