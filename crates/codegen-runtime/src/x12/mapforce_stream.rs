//! Explicit stream boundary. The caller owns opening, truncation and closing.
use super::*;
use std::io::{self, Write};

#[derive(Debug)]
pub enum MapForceX12WriteError {
    Format(X12BoundaryError),
    Io(io::Error),
}
impl fmt::Display for MapForceX12WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
        }
    }
}
impl Error for MapForceX12WriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Format(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}

/// Render into an already opened output, then write and flush. This does not
/// close or accept the document; the host must close successfully before acceptance.
pub fn write_mapforce_x12_document(
    descriptor_text: &str,
    document: &Instance,
    execution: &ExecutionContext<'_>,
    output: &mut dyn Write,
) -> Result<(), MapForceX12WriteError> {
    let text =
        super::mapforce_writer::render_mapforce_x12_document(descriptor_text, document, execution)
            .map_err(MapForceX12WriteError::Format)?;
    write_text(&text, output).map_err(MapForceX12WriteError::Io)
}

// StreamWriter's 1024-character buffer retains a terminal high surrogate in
// the encoder until the next chunk. Encoding each Rust char or byte chunk
// independently would preserve full bytes but change observable write prefixes.
pub(super) fn write_text(text: &str, output: &mut dyn Write) -> io::Result<()> {
    let mut units = text.encode_utf16();
    let mut pending = None;
    loop {
        let mut chunk = [0_u16; 1024];
        let mut count = 0;
        for unit in units.by_ref().take(chunk.len()) {
            chunk[count] = unit;
            count += 1;
        }
        let mut bytes = [0_u8; 1025];
        let mut length = 0;
        if let Some(unit) = pending.take() {
            bytes[length] = super::latin1_best_fit::unit(unit);
            length += 1;
        }
        if count == 0 {
            if length != 0 {
                output.write_all(&bytes[..length])?;
            }
            break;
        }
        if count == chunk.len() && (0xd800..=0xdbff).contains(&chunk[count - 1]) {
            count -= 1;
            pending = Some(chunk[count]);
        }
        for unit in &chunk[..count] {
            bytes[length] = super::latin1_best_fit::unit(*unit);
            length += 1;
        }
        if length != 0 {
            output.write_all(&bytes[..length])?;
        }
    }
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Sink {
        writes: Vec<Vec<u8>>,
        flushes: usize,
        fail: bool,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail {
                return Err(io::Error::other("injected"));
            }
            self.writes.push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
    #[test]
    fn retains_utf16_encoder_state_between_character_buffers() {
        let text = "a".repeat(1023) + "😀" + &"b".repeat(1023);
        let mut sink = Sink::default();
        write_text(&text, &mut sink).unwrap();
        assert_eq!(
            sink.writes.iter().map(Vec::len).collect::<Vec<_>>(),
            [1023, 1025]
        );
        assert_eq!(
            sink.writes.concat(),
            text.encode_utf16()
                .map(super::super::latin1_best_fit::unit)
                .collect::<Vec<_>>()
        );
        assert_eq!(sink.flushes, 1);
    }
    #[test]
    fn no_flush_follows_a_failed_write() {
        let mut sink = Sink {
            fail: true,
            ..Default::default()
        };
        assert!(write_text("data", &mut sink).is_err());
        assert_eq!(sink.flushes, 0);
    }

    #[test]
    fn supplementary_boundaries_match_the_fixed_native_encoder_controls() {
        for prefix in [0, 1, 1022, 1023, 1024, 1025, 2047, 2048] {
            let text = "A".repeat(prefix) + "😀" + &"B".repeat(3072 - prefix);
            let mut sink = Sink::default();
            write_text(&text, &mut sink).unwrap();
            let expected = match prefix {
                1023 => vec![1023, 1025, 1024, 2],
                2047 => vec![1024, 1023, 1025, 2],
                _ => vec![1024, 1024, 1024, 2],
            };
            assert_eq!(
                sink.writes.iter().map(Vec::len).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(sink.flushes, 1);
            assert_eq!(
                sink.writes.concat(),
                text.encode_utf16()
                    .map(super::super::latin1_best_fit::unit)
                    .collect::<Vec<_>>()
            );
        }
    }
}
