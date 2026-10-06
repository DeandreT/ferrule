use std::fmt;
use std::io::{self, Write};

use ir::{Instance, SchemaNode};

use super::{CsvFormatError, CsvWriteOptions, PreparedCsv, prepare_csv};

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

const FIELD_CHUNK_BYTES: usize = 4096;
const ENCODE_BUFFER_BYTES: usize = 2 * FIELD_CHUNK_BYTES + 2;

fn write_record<'a>(
    writer: &mut csv_core::Writer,
    sink: &mut BoundedBytes,
    buffer: &mut [u8; ENCODE_BUFFER_BYTES],
    fields: impl IntoIterator<Item = &'a str>,
) -> Result<(), CsvBoundedError> {
    for (index, field) in fields.into_iter().enumerate() {
        if index != 0 {
            let (_, written) = writer.delimiter(buffer);
            sink.write_all(&buffer[..written])
                .map_err(bounded_io_error)?;
        }
        // Necessary quoting must inspect the complete field, including a
        // delimiter or quote beyond the first output buffer. Do this once.
        let bytes = field.as_bytes();
        let (_, consumed, written) = writer.field(bytes, buffer);
        sink.write_all(&buffer[..written])
            .map_err(bounded_io_error)?;
        // csv-core retains the field's quoting state. Its worst-case escaped
        // output is twice the chunk length, so each remaining chunk fits and
        // is completely consumed. This avoids rescanning a large suffix each
        // time the fixed output buffer fills.
        for chunk in bytes[consumed..].chunks(FIELD_CHUNK_BYTES) {
            let (_, _, written) = writer.field(chunk, buffer);
            sink.write_all(&buffer[..written])
                .map_err(bounded_io_error)?;
        }
    }
    // Keep csv-core's empty-record, closing-quote, and LF behavior intact.
    let (_, written) = writer.terminator(buffer);
    sink.write_all(&buffer[..written]).map_err(bounded_io_error)
}

/// Serialize CSV using the native row/dialect rules and a UTF-8 byte ceiling.
/// Every row is validated before BOM, headers, or records are written. The
/// ceiling bounds output bytes, not the allocated input tree or validated rows.
pub fn to_bytes_with_options_bounded(
    schema: &SchemaNode,
    rows: &[Instance],
    options: &CsvWriteOptions,
    maximum: usize,
) -> Result<Vec<u8>, CsvBoundedError> {
    let PreparedCsv {
        fields,
        delimiter,
        quote,
        records,
    } = prepare_csv(schema, rows, options)?;
    let mut sink = BoundedBytes::new(maximum);
    if options.utf8_bom {
        sink.write_all(b"\xef\xbb\xbf").map_err(bounded_io_error)?;
    }
    let mut writer = csv_core::WriterBuilder::new()
        .delimiter(delimiter)
        .quote(quote.unwrap_or(b'"'))
        .quote_style(if options.quote_disabled {
            csv_core::QuoteStyle::Never
        } else {
            csv_core::QuoteStyle::Necessary
        })
        .terminator(csv_core::Terminator::Any(b'\n'))
        .build();
    let mut buffer = [0; ENCODE_BUFFER_BYTES];
    if options.has_headers {
        write_record(
            &mut writer,
            &mut sink,
            &mut buffer,
            fields.iter().map(|(name, _)| *name),
        )?;
    }
    for record in records {
        write_record(
            &mut writer,
            &mut sink,
            &mut buffer,
            record.iter().map(String::as_str),
        )?;
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
