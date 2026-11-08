use std::io::Read;
use std::path::Path;

/// Reads one XML Schema document with bounded UTF-8/UTF-16 decoding.
///
/// Callers that only need schema annotations can use this without invoking
/// the full XSD importer while retaining the same encoding rules.
pub fn read_text(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(max_bytes.min(8_192) as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    decode_owned_schema_bytes(bytes, max_bytes)
}

/// Decodes the exact supplied raw bytes without opening a resource pathname.
pub fn decode_schema_bytes(bytes: &[u8], max_bytes: u64) -> std::io::Result<String> {
    check_schema_size(bytes.len(), max_bytes)?;
    decode_owned_schema_bytes(bytes.to_vec(), max_bytes)
}

fn check_schema_size(length: usize, max_bytes: u64) -> std::io::Result<()> {
    if u64::try_from(length).map_or(true, |length| length > max_bytes) {
        return Err(invalid_xml_encoding(&format!(
            "XML Schema exceeds the {max_bytes}-byte input limit"
        )));
    }
    Ok(())
}

fn decode_owned_schema_bytes(bytes: Vec<u8>, max_bytes: u64) -> std::io::Result<String> {
    check_schema_size(bytes.len(), max_bytes)?;
    if bytes.starts_with(&[0x00, 0x00, 0xfe, 0xff]) || bytes.starts_with(&[0xff, 0xfe, 0x00, 0x00])
    {
        return Err(invalid_xml_encoding("UTF-32 schemas are not supported"));
    }
    if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16(body, u16::from_le_bytes);
    }
    if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16(body, u16::from_be_bytes);
    }
    if bytes.starts_with(&[b'<', 0, b'?', 0]) {
        return decode_utf16(&bytes, u16::from_le_bytes);
    }
    if bytes.starts_with(&[0, b'<', 0, b'?']) {
        return decode_utf16(&bytes, u16::from_be_bytes);
    }
    let decoded = match bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        Some(body) => String::from_utf8(body.to_vec()),
        None => String::from_utf8(bytes),
    };
    decoded.map_err(|error| invalid_xml_encoding(&format!("schema is not UTF-8: {error}")))
}

fn decode_utf16(bytes: &[u8], decode: fn([u8; 2]) -> u16) -> std::io::Result<String> {
    let (chunks, remainder) = bytes.as_chunks::<2>();
    let units = chunks.iter().copied().map(decode).collect::<Vec<_>>();
    if !remainder.is_empty() {
        return Err(invalid_xml_encoding(
            "UTF-16 schema contains an incomplete code unit",
        ));
    }
    String::from_utf16(&units)
        .map_err(|error| invalid_xml_encoding(&format!("schema is not valid UTF-16: {error}")))
}

fn invalid_xml_encoding(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
