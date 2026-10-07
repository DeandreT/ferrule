use std::io::{self, Write};

const FIELD_CHUNK_BYTES: usize = 4096;
const ENCODE_BUFFER_BYTES: usize = 2 * FIELD_CHUNK_BYTES + 2;

/// One native CSV record encoder for bounded and ordinary byte sinks.
pub(super) struct RecordEncoder {
    writer: csv_core::Writer,
    buffer: [u8; ENCODE_BUFFER_BYTES],
}

impl RecordEncoder {
    pub(super) fn new(delimiter: u8, quote: Option<u8>, quote_disabled: bool) -> Self {
        let writer = csv_core::WriterBuilder::new()
            .delimiter(delimiter)
            .quote(quote.unwrap_or(b'"'))
            .quote_style(if quote_disabled {
                csv_core::QuoteStyle::Never
            } else {
                csv_core::QuoteStyle::Necessary
            })
            .terminator(csv_core::Terminator::Any(b'\n'))
            .build();
        Self {
            writer,
            buffer: [0; ENCODE_BUFFER_BYTES],
        }
    }

    pub(super) fn write_record<'a, W: Write + ?Sized>(
        &mut self,
        sink: &mut W,
        fields: impl IntoIterator<Item = &'a str>,
    ) -> io::Result<()> {
        for (index, field) in fields.into_iter().enumerate() {
            if index != 0 {
                let (_, written) = self.writer.delimiter(&mut self.buffer);
                sink.write_all(&self.buffer[..written])?;
            }
            // Necessary quoting must inspect the complete field, including a
            // delimiter or quote beyond the first output buffer. Do this once.
            let bytes = field.as_bytes();
            let (_, consumed, written) = self.writer.field(bytes, &mut self.buffer);
            sink.write_all(&self.buffer[..written])?;
            // csv-core retains the field's quoting state. Its worst-case escaped
            // output is twice the chunk length, so each remaining chunk fits and
            // is completely consumed. This avoids rescanning a large suffix each
            // time the fixed output buffer fills.
            for chunk in bytes[consumed..].chunks(FIELD_CHUNK_BYTES) {
                let (_, _, written) = self.writer.field(chunk, &mut self.buffer);
                sink.write_all(&self.buffer[..written])?;
            }
        }
        // Keep csv-core's empty-record, closing-quote, and LF behavior intact.
        let (_, written) = self.writer.terminator(&mut self.buffer);
        sink.write_all(&self.buffer[..written])
    }
}
