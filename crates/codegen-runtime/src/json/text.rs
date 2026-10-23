//! Exact-size JSON text for the generated JSON and JSON5 output boundaries.
//!
//! The text is measured by one serialization pass that writes nothing, so a
//! caller can apply its output byte limit before the document exists and then
//! write it once into a buffer of exactly that length. Growing a buffer while
//! writing allocates about twice the final text and keeps up to one and a half
//! copies alive while it moves.

use std::io;

use serde_json::Value;

use super::JsonOutputFormat;

/// Counts the bytes a serializer writes.
struct ByteCount(usize);

impl io::Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write(
    value: &Value,
    format: JsonOutputFormat,
    writer: impl io::Write,
) -> serde_json::Result<()> {
    match format {
        JsonOutputFormat::Pretty => serde_json::to_writer_pretty(writer, value),
        JsonOutputFormat::Compact => serde_json::to_writer(writer, value),
    }
}

/// The UTF-8 length of `value`'s text, including the line feed that ends a
/// pretty document. Serialization errors are those of the writing pass.
pub(super) fn measure(value: &Value, format: JsonOutputFormat) -> serde_json::Result<usize> {
    let mut count = ByteCount(0);
    write(value, format, &mut count)?;
    Ok(match format {
        JsonOutputFormat::Pretty => count.0.saturating_add(1),
        JsonOutputFormat::Compact => count.0,
    })
}

/// Writes the text [`measure`] counted as `bytes` into one buffer of that size.
pub(super) fn render(
    value: &Value,
    format: JsonOutputFormat,
    bytes: usize,
) -> serde_json::Result<String> {
    let mut text = Vec::with_capacity(bytes);
    write(value, format, &mut text)?;
    if matches!(format, JsonOutputFormat::Pretty) {
        text.push(b'\n');
    }
    // serde_json writes UTF-8; this check cannot fail.
    String::from_utf8(text)
        .map_err(|error| serde_json::Error::io(io::Error::new(io::ErrorKind::InvalidData, error)))
}

/// Measures and writes pretty JSON text after `admit` accepts its length.
pub(crate) fn pretty_with_limit<E>(
    value: &Value,
    serialization: impl Fn(serde_json::Error) -> E,
    admit: impl FnOnce(usize) -> Result<(), E>,
) -> Result<String, E> {
    let bytes = measure(value, JsonOutputFormat::Pretty).map_err(&serialization)?;
    admit(bytes)?;
    render(value, JsonOutputFormat::Pretty, bytes).map_err(serialization)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_text_matches_serde_json_exactly() {
        let values = [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!({"a": [1, 2.5, null, true], "雪": "\u{1F600}\"\n", "b": {"c": []}}),
            serde_json::json!("x"),
            serde_json::json!([[], {}, [{"k": -0.0}]]),
        ];
        for value in &values {
            for (format, expected) in [
                (
                    JsonOutputFormat::Pretty,
                    serde_json::to_string_pretty(value).map(|text| text + "\n"),
                ),
                (JsonOutputFormat::Compact, serde_json::to_string(value)),
            ] {
                let Ok(expected) = expected else {
                    panic!("serde_json serializes the fixture");
                };
                let Ok(bytes) = measure(value, format) else {
                    panic!("fixture measures");
                };
                assert_eq!(bytes, expected.len());
                let Ok(text) = render(value, format, bytes) else {
                    panic!("fixture renders");
                };
                assert_eq!(text, expected);
                assert_eq!(text.capacity(), bytes);
            }
        }
    }

    #[test]
    fn limit_refuses_before_writing() {
        let value = serde_json::json!({"a": 1});
        let refused = pretty_with_limit(
            &value,
            |error| error.to_string(),
            |bytes| Err(format!("{bytes} bytes")),
        );
        assert_eq!(refused, Err("13 bytes".to_string()));
        assert_eq!(
            pretty_with_limit(&value, |error| error.to_string(), |_| Ok(())),
            Ok("{\n  \"a\": 1\n}\n".to_string())
        );
    }
}
