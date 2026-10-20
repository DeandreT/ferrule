//! Explicit byte decoding; ordinary CSV byte entry points stay strict UTF-8.

/// Host-selected CSV byte policy, independent of dialect and saved file paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CsvByteDecodingPolicy {
    #[default]
    StrictUtf8,
    /// Detect UTF-8/16/32 BOMs, otherwise use UTF-8. Replace malformed interior
    /// sequences, but leave incomplete terminal decoder state unflushed.
    BomAwareNonFlushingReplacement,
}

pub(super) fn decode_non_flushing(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe, 0x00, 0x00]) {
        utf32(bytes, false)
    } else if let Some(bytes) = bytes.strip_prefix(&[0x00, 0x00, 0xfe, 0xff]) {
        utf32(bytes, true)
    } else if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        utf16(bytes, false)
    } else if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        utf16(bytes, true)
    } else {
        utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
    }
}

fn utf8(mut bytes: &[u8]) -> String {
    let mut output = String::new();
    loop {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                output.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                output.push_str(std::str::from_utf8(&bytes[..valid]).expect("valid UTF-8 prefix"));
                let Some(invalid) = error.error_len() else {
                    // A decoder called with flush=false retains this suffix.
                    break;
                };
                output.push(char::REPLACEMENT_CHARACTER);
                bytes = &bytes[valid + invalid..];
            }
        }
    }
    output
}

fn utf16(bytes: &[u8], big_endian: bool) -> String {
    let mut words = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| {
            let word = [bytes[0], bytes[1]];
            if big_endian {
                u16::from_be_bytes(word)
            } else {
                u16::from_le_bytes(word)
            }
        })
        .peekable();
    let mut output = String::new();
    while let Some(word) = words.next() {
        if (0xd800..=0xdbff).contains(&word) {
            match words.peek().copied() {
                None => break,
                Some(low) if (0xdc00..=0xdfff).contains(&low) => {
                    words.next();
                    let scalar =
                        0x10000 + ((u32::from(word) - 0xd800) << 10) + u32::from(low) - 0xdc00;
                    output.push(char::from_u32(scalar).expect("valid surrogate pair"));
                }
                Some(_) => output.push(char::REPLACEMENT_CHARACTER),
            }
        } else if (0xdc00..=0xdfff).contains(&word) {
            output.push(char::REPLACEMENT_CHARACTER);
        } else {
            output.push(char::from_u32(u32::from(word)).expect("non-surrogate BMP scalar"));
        }
    }
    output
}

fn utf32(bytes: &[u8], big_endian: bool) -> String {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| {
            let word = [bytes[0], bytes[1], bytes[2], bytes[3]];
            let scalar = if big_endian {
                u32::from_be_bytes(word)
            } else {
                u32::from_le_bytes(word)
            };
            char::from_u32(scalar).unwrap_or(char::REPLACEMENT_CHARACTER)
        })
        .collect()
}
