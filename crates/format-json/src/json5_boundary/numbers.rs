use super::{Json5SyntaxError, Json5SyntaxKind, Parser};

impl Parser<'_> {
    pub(super) fn number(&mut self) -> Result<(), Json5SyntaxError> {
        let start = self.index;
        let end = self.token_end()?;
        let source = self.source;
        let token = &source[start..end];
        let negative = token.starts_with('-');
        let bare = if negative || token.starts_with('+') {
            &token[1..]
        } else {
            token
        };
        // Sign/prefix inspection plus one complete grammar/finite-word pass.
        self.charge(token.len().saturating_add(16))?;
        if matches!(bare, "Infinity" | "NaN") {
            return Err(self.syntax(Json5SyntaxKind::NonFiniteNumber, start));
        }
        if let Some(hex) = bare.strip_prefix("0x").or_else(|| bare.strip_prefix("0X")) {
            if hex.is_empty() {
                return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start));
            }
            let mut magnitude = Some(0u64);
            for byte in hex.bytes() {
                let digit = match byte {
                    b'0'..=b'9' => u64::from(byte - b'0'),
                    b'a'..=b'f' => u64::from(byte - b'a' + 10),
                    b'A'..=b'F' => u64::from(byte - b'A' + 10),
                    _ => return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start)),
                };
                magnitude =
                    magnitude.and_then(|current| current.checked_mul(16)?.checked_add(digit));
            }
            return self.integer(magnitude, negative, start);
        }

        let bytes = bare.as_bytes();
        let mut index = 0;
        let integer_start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        let integer_digits = index - integer_start;
        if integer_digits > 1 && bytes.first() == Some(&b'0') {
            return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start));
        }
        let has_point = bytes.get(index) == Some(&b'.');
        let mut fraction_digits = 0;
        if has_point {
            index += 1;
            let fraction_start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            fraction_digits = index - fraction_start;
        }
        if integer_digits == 0 && fraction_digits == 0 {
            return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start));
        }
        let exponent = index;
        let has_exponent = matches!(bytes.get(index), Some(b'e' | b'E'));
        if has_exponent {
            index += 1;
            if matches!(bytes.get(index), Some(b'+' | b'-')) {
                index += 1;
            }
            let exponent_start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if exponent_start == index {
                return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start));
            }
        }
        if index != bare.len() {
            return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start));
        }

        if !has_point && !has_exponent {
            // The exact integer fold is a separate complete repeat pass.
            self.charge(bare.len())?;
            let mut magnitude = Some(0u64);
            for byte in bare.bytes() {
                magnitude = magnitude.and_then(|current| {
                    current.checked_mul(10)?.checked_add(u64::from(byte - b'0'))
                });
            }
            return self.integer(magnitude, negative, start);
        }

        // The standard finite-number check receives the complete validated
        // token once. Charge its full length separately from grammar scanning;
        // never serialize the parsed Float back into the preserved lexical.
        self.charge(token.len())?;
        match token.parse::<f64>() {
            Ok(value) if value.is_finite() => {}
            Ok(_) => return Err(self.syntax(Json5SyntaxKind::NonFiniteNumber, start)),
            Err(_) => return Err(self.syntax(Json5SyntaxKind::InvalidNumber, start)),
        }
        if negative {
            self.append("-")?;
        }
        if integer_digits == 0 {
            self.append("0")?;
        }
        self.append(&bare[..exponent])?;
        if has_point && fraction_digits == 0 {
            self.append("0")?;
        }
        self.append(&bare[exponent..])
    }

    fn integer(
        &mut self,
        magnitude: Option<u64>,
        negative: bool,
        offset: usize,
    ) -> Result<(), Json5SyntaxError> {
        let Some(magnitude) = magnitude else {
            return Err(self.syntax(Json5SyntaxKind::IntegerOutOfRange, offset));
        };
        if negative && magnitude > (1u64 << 63) {
            return Err(self.syntax(Json5SyntaxKind::IntegerOutOfRange, offset));
        }
        // u64 decimal formatting has at most 20 digits; charge that bounded
        // pass before allocating its at most 20-byte temporary.
        self.charge(20)?;
        let digits = magnitude.to_string();
        if negative && magnitude != 0 {
            self.append("-")?;
        }
        self.append(&digits)
    }
}
