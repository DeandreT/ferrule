//! Explicit native JSON stream boundary. Opening, closing and acceptance are
//! host-owned; a failed write is not retried by disposing another writer.
use crate::{MapForceJson5Error, serialize_mapforce_json_ascii_keys};
use ir::Instance;
use std::{
    fmt,
    io::{self, Write},
};

#[derive(Debug)]
pub enum MapForceJsonWriteError {
    Format(MapForceJson5Error),
    Io(io::Error),
}
impl fmt::Display for MapForceJsonWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for MapForceJsonWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

/// Serialize a schema-bound value, write through a native-sized UTF-16 buffer,
/// then flush once. The caller closes successfully before accepting a document.
pub fn write_mapforce_json_ascii_keys(
    descriptor: &str,
    instance: &Instance,
    output: &mut dyn Write,
) -> Result<(), MapForceJsonWriteError> {
    let text = serialize_mapforce_json_ascii_keys(descriptor, instance)
        .map_err(MapForceJsonWriteError::Format)?;
    write_utf8(&text, output).map_err(MapForceJsonWriteError::Io)
}

fn write_utf8(text: &str, output: &mut dyn Write) -> io::Result<()> {
    let mut units = text.encode_utf16();
    let mut pending = None;
    loop {
        let mut chunk = Vec::with_capacity(1025);
        if let Some(high) = pending.take() {
            chunk.push(high);
        }
        let count = chunk.len();
        chunk.extend(units.by_ref().take(1024));
        if chunk.len() == count {
            break;
        }
        if chunk
            .last()
            .is_some_and(|unit| (0xd800..=0xdbff).contains(unit))
        {
            pending = chunk.pop();
        }
        let encoded = String::from_utf16(&chunk).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid native JSON encoder state",
            )
        })?;
        if !encoded.is_empty() {
            output.write_all(encoded.as_bytes())?;
        }
    }
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Sink {
        chunks: Vec<Vec<u8>>,
        flushes: usize,
        fail: bool,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail {
                return Err(io::Error::other("injected"));
            }
            self.chunks.push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
    #[test]
    fn buffers_count_utf16_units_and_hold_split_surrogates() {
        let text = "a".repeat(1023) + "🙂" + &"b".repeat(1023);
        let mut sink = Sink::default();
        write_utf8(&text, &mut sink).unwrap();
        assert_eq!(
            sink.chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            [1023, 1027]
        );
        assert_eq!(sink.chunks.concat(), text.as_bytes());
        assert_eq!(sink.flushes, 1);
    }
    #[test]
    fn failure_does_not_flush_or_retry() {
        let mut sink = Sink {
            fail: true,
            ..Default::default()
        };
        assert!(write_utf8("body", &mut sink).is_err());
        assert_eq!(sink.flushes, 0);
    }

    #[test]
    fn format_failure_never_writes_or_flushes() {
        let mut sink = Sink::default();
        assert!(matches!(
            write_mapforce_json_ascii_keys("{}", &Instance::Scalar(ir::Value::Null), &mut sink),
            Err(MapForceJsonWriteError::Format(_))
        ));
        assert!(sink.chunks.is_empty());
        assert_eq!(sink.flushes, 0);
    }
}
