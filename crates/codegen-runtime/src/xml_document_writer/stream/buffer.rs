//! Safe implementation of the synchronous native XML buffer profile. The
//! cursor/flush contract follows .NET 8.0.22 XmlUtf8RawTextWriter (MIT):
//! https://github.com/dotnet/runtime/blob/v8.0.22/src/libraries/System.Private.Xml/src/System/Xml/Core/XmlUtf8RawTextWriter.cs
use super::{MapForceXmlError, MapForceXmlWriteError};
use std::io::Write;

const BUFFER: usize = 6144;
const OVERFLOW: usize = 32;

pub(super) struct NativeXmlBuffer<'a> {
    output: &'a mut dyn Write,
    bytes: [u8; BUFFER + OVERFLOW],
    position: usize,
    written: usize,
    suppress_writes: bool,
}
impl<'a> NativeXmlBuffer<'a> {
    pub(super) fn new(output: &'a mut dyn Write) -> Self {
        Self {
            output,
            bytes: [0; BUFFER + OVERFLOW],
            position: 1,
            written: 0,
            suppress_writes: false,
        }
    }
    fn put(&mut self, cursor: &mut usize, value: &[u8]) -> Result<(), MapForceXmlWriteError> {
        let end = cursor
            .checked_add(value.len())
            .filter(|end| *end <= self.bytes.len())
            .ok_or(MapForceXmlError::Limit)?;
        self.bytes[*cursor..end].copy_from_slice(value);
        *cursor = end;
        Ok(())
    }
    pub(super) fn literal(&mut self, value: &[u8]) -> Result<(), MapForceXmlWriteError> {
        let mut cursor = self.position;
        self.put(&mut cursor, value)?;
        self.position = cursor;
        Ok(())
    }
    pub(super) fn raw(&mut self, value: &str) -> Result<(), MapForceXmlWriteError> {
        debug_assert!(value.is_ascii());
        let mut source = value.as_bytes();
        let mut cursor = self.position;
        while !source.is_empty() {
            let count = source.len().min(BUFFER.saturating_sub(cursor));
            self.put(&mut cursor, &source[..count])?;
            source = &source[count..];
            if source.is_empty() {
                break;
            }
            self.position = cursor;
            self.flush_buffer()?;
            cursor = self.position;
        }
        self.position = cursor;
        Ok(())
    }
    pub(super) fn text(
        &mut self,
        value: &str,
        attribute: bool,
    ) -> Result<(), MapForceXmlWriteError> {
        let mut remaining = value.encode_utf16().count();
        let mut chars = value.chars().peekable();
        let mut cursor = self.position;
        loop {
            let end = cursor.saturating_add(remaining).min(BUFFER);
            while cursor < end
                && chars.peek().is_some_and(|c| {
                    matches!(*c, ' '..='\u{7f}') && !matches!(*c, '"' | '\'' | '&' | '<' | '>')
                })
            {
                let ch = chars.next().expect("checked character");
                self.bytes[cursor] = ch as u8;
                cursor += 1;
                remaining -= 1;
            }
            let Some(ch) = chars.peek().copied() else {
                break;
            };
            if cursor >= end {
                self.position = cursor;
                self.flush_buffer()?;
                cursor = self.position;
                continue;
            }
            chars.next();
            remaining -= ch.len_utf16();
            match ch {
                '&' => self.put(&mut cursor, b"&amp;")?,
                '<' => self.put(&mut cursor, b"&lt;")?,
                '>' => self.put(&mut cursor, b"&gt;")?,
                '"' if attribute => self.put(&mut cursor, b"&quot;")?,
                '\t' if attribute => self.put(&mut cursor, b"&#x9;")?,
                '\r' if attribute => self.put(&mut cursor, b"&#xD;")?,
                '\n' if attribute => self.put(&mut cursor, b"&#xA;")?,
                '\r' | '\n' => {
                    if ch == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                        remaining -= 1;
                    }
                    self.position = cursor;
                    self.raw("\r\n")?;
                    cursor = self.position;
                }
                '\t'
                | '\u{20}'..='\u{d7ff}'
                | '\u{e000}'..='\u{fffd}'
                | '\u{10000}'..='\u{10ffff}' => {
                    self.put(&mut cursor, ch.encode_utf8(&mut [0; 4]).as_bytes())?
                }
                _ => return Err(MapForceXmlError::Character(ch as u32).into()),
            }
        }
        self.position = cursor;
        Ok(())
    }
    pub(super) fn indent(&mut self, depth: usize) -> Result<(), MapForceXmlWriteError> {
        self.raw("\r\n")?;
        for _ in 0..depth {
            self.raw("\t")?;
        }
        Ok(())
    }
    fn flush_buffer(&mut self) -> Result<(), MapForceXmlWriteError> {
        let count = self.position - 1;
        let result = if self.suppress_writes || count == 0 {
            Ok(())
        } else if count > super::MAX_BYTES - self.written {
            Err(MapForceXmlError::Limit.into())
        } else {
            self.output
                .write_all(&self.bytes[1..self.position])
                .map_err(MapForceXmlWriteError::Io)
        };
        self.position = 1;
        if result.is_err() {
            self.suppress_writes = true;
        } else {
            self.written += count;
        }
        result
    }
    pub(super) fn flush_writer(&mut self) -> Result<(), MapForceXmlWriteError> {
        self.flush_buffer()?;
        self.output.flush().map_err(MapForceXmlWriteError::Io)
    }
    pub(super) fn close_writer(&mut self) -> Result<(), MapForceXmlWriteError> {
        let result = self.flush_buffer();
        self.output
            .flush()
            .map_err(MapForceXmlWriteError::Io)
            .and(result)
    }
}
