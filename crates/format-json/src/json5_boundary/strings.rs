use super::{Json5SyntaxError, Json5SyntaxKind, Parser, line_terminator};

impl Parser<'_> {
    pub(super) fn string(&mut self) -> Result<(), Json5SyntaxError> {
        let start = self.index;
        let quote = self.take()?;
        self.append("\"")?;
        loop {
            let offset = self.index;
            let Some(c) = self.peek()? else {
                return Err(self.syntax(Json5SyntaxKind::UnterminatedString, start));
            };
            self.take()?;
            match c {
                c if c == quote => return self.append("\""),
                '\\' => {
                    if let Some(decoded) = self.escape(offset)? {
                        self.escaped_scalar(decoded)?;
                    }
                }
                '\n' | '\r' => {
                    return Err(self.syntax(Json5SyntaxKind::LineTerminatorInString, offset));
                }
                _ => self.escaped_scalar(c)?,
            }
        }
    }

    pub(super) fn identifier(&mut self) -> Result<(), Json5SyntaxError> {
        self.append("\"")?;
        let mut first = true;
        loop {
            let offset = self.index;
            let Some(c) = self.peek()? else {
                break;
            };
            if c == '\\' {
                self.take()?;
                if self.peek()? != Some('u') {
                    return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                }
                self.take()?;
                let value = self.hex(4, offset)?;
                let Some(decoded) = char::from_u32(value) else {
                    return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                };
                if !identifier_char(decoded, first) {
                    return Err(self.syntax(Json5SyntaxKind::UnsupportedIdentifier, offset));
                }
                self.escaped_scalar(decoded)?;
            } else if identifier_char(c, first) {
                self.take()?;
                self.escaped_scalar(c)?;
            } else {
                if !c.is_ascii() && !super::whitespace(c) {
                    return Err(self.syntax(Json5SyntaxKind::UnsupportedIdentifier, offset));
                }
                break;
            }
            first = false;
        }
        if first {
            return Err(self.syntax(Json5SyntaxKind::UnsupportedIdentifier, self.index));
        }
        self.append("\"")
    }

    fn escape(&mut self, offset: usize) -> Result<Option<char>, Json5SyntaxError> {
        let Some(c) = self.peek()? else {
            return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
        };
        self.take()?;
        let decoded = match c {
            '\r' => {
                if self.peek()? == Some('\n') {
                    self.take()?;
                }
                return Ok(None);
            }
            c if line_terminator(c) => return Ok(None),
            'b' => '\u{0008}',
            't' => '\t',
            'n' => '\n',
            'v' => '\u{000b}',
            'f' => '\u{000c}',
            'r' => '\r',
            '0' => {
                if self.peek()?.is_some_and(|next| next.is_ascii_digit()) {
                    return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                }
                '\0'
            }
            '1'..='9' => return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset)),
            'x' => {
                let value = self.hex(2, offset)?;
                char::from_u32(value)
                    .ok_or_else(|| self.syntax(Json5SyntaxKind::InvalidEscape, offset))?
            }
            'u' => {
                let first = self.hex(4, offset)?;
                let value = match first {
                    0xd800..=0xdbff => {
                        if self.peek()? != Some('\\') {
                            return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                        }
                        self.take()?;
                        if self.peek()? != Some('u') {
                            return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                        }
                        self.take()?;
                        let second = self.hex(4, offset)?;
                        if !(0xdc00..=0xdfff).contains(&second) {
                            return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                        }
                        0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                    }
                    0xdc00..=0xdfff => {
                        return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
                    }
                    _ => first,
                };
                char::from_u32(value)
                    .ok_or_else(|| self.syntax(Json5SyntaxKind::InvalidEscape, offset))?
            }
            _ => c,
        };
        Ok(Some(decoded))
    }

    fn hex(&mut self, count: usize, offset: usize) -> Result<u32, Json5SyntaxError> {
        let mut value = 0;
        for _ in 0..count {
            let Some(c) = self.peek()? else {
                return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
            };
            let digit = c.to_digit(16).filter(|_| c.is_ascii());
            let Some(digit) = digit else {
                return Err(self.syntax(Json5SyntaxKind::InvalidEscape, offset));
            };
            self.take()?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn escaped_scalar(&mut self, c: char) -> Result<(), Json5SyntaxError> {
        match c {
            '"' => self.append("\\\""),
            '\\' => self.append("\\\\"),
            '\u{0008}' => self.append("\\b"),
            '\t' => self.append("\\t"),
            '\n' => self.append("\\n"),
            '\u{000c}' => self.append("\\f"),
            '\r' => self.append("\\r"),
            c if c < '\u{0020}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let value = c as usize;
                let encoded = [b'\\', b'u', b'0', b'0', HEX[value >> 4], HEX[value & 15]];
                // All bytes are statically ASCII; avoid an unchecked string conversion.
                for byte in encoded {
                    self.append(char::from(byte).encode_utf8(&mut [0; 4]))?;
                }
                Ok(())
            }
            c => self.append(c.encode_utf8(&mut [0; 4])),
        }
    }
}

fn identifier_char(c: char, first: bool) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '$') || (!first && c.is_ascii_digit())
}
