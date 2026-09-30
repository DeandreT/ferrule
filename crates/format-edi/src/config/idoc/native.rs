//! Strict, lossless-for-supported-directives IDoc config parsing and rendering.
//!
//! This grammar is deliberately narrower than the executable legacy importer.
//! An unsupported directive prevents provenance certification, while the legacy
//! schema/layout compiler can still execute the file as before.

use std::fmt::Write;
use std::num::NonZeroU32;

use mapping::{
    IdocNativeCode, IdocNativeConfig, IdocNativeConfigError, IdocNativeField, IdocNativeFieldType,
    IdocNativeGroup, IdocNativeNode, IdocNativeSegment, IdocNativeStatus, MAX_IDOC_NATIVE_BYTES,
    MAX_IDOC_NATIVE_DEPTH,
};

const MAX_LINES: usize = 200_000;

/// Parses one complete native IDoc configuration. Unknown or misplaced
/// directives, missing metadata, and unsupported field types fail closed.
pub fn parse_native_config(text: &str) -> Result<IdocNativeConfig, IdocNativeConfigError> {
    if text.len() > MAX_IDOC_NATIVE_BYTES {
        return Err(IdocNativeConfigError::Limit("file size"));
    }
    let mut lines = Vec::new();
    for (index, source) in text.lines().enumerate() {
        if index >= MAX_LINES {
            return Err(IdocNativeConfigError::Limit("line count"));
        }
        let source = source.trim_end_matches('\r').trim_start();
        if source.trim().is_empty() {
            continue;
        }
        let key_end = source.find(char::is_whitespace).unwrap_or(source.len());
        lines.push(Line {
            key: &source[..key_end],
            arg: source[key_end..].trim_start(),
            number: index + 1,
        });
    }
    let mut parser = Parser { lines, cursor: 0 };
    parser.take_exact("BEGIN_SEGMENT_SECTION")?;
    let name = parser.take_value("BEGIN_IDOC")?.to_owned();
    let nodes = parser.parse_nodes("END_IDOC", 0)?;
    parser.take_exact("END_SEGMENT_SECTION")?;
    if parser.peek().is_some() {
        return Err(parser.error("content after END_SEGMENT_SECTION"));
    }
    IdocNativeConfig::new(name, nodes)
}

/// Renders a validated descriptor in the supported line-oriented grammar.
/// The result is normalized UTF-8 text, independent of source whitespace.
pub fn render_native_config(config: &IdocNativeConfig) -> Result<String, IdocNativeConfigError> {
    config.project()?;
    let mut output = String::new();
    writeln!(output, "BEGIN_SEGMENT_SECTION").unwrap();
    writeln!(output, "  BEGIN_IDOC {}", config.name()).unwrap();
    let mut lines = 2;
    for node in config.nodes() {
        render_node(&mut output, node, 2, &mut lines)?;
    }
    writeln!(output, "  END_IDOC").unwrap();
    writeln!(output, "END_SEGMENT_SECTION").unwrap();
    lines += 2;
    if lines > MAX_LINES {
        return Err(IdocNativeConfigError::Limit("line count"));
    }
    if output.len() > MAX_IDOC_NATIVE_BYTES {
        return Err(IdocNativeConfigError::Limit("file size"));
    }
    Ok(output)
}

fn render_node(
    output: &mut String,
    node: &IdocNativeNode,
    indent: usize,
    lines: &mut usize,
) -> Result<(), IdocNativeConfigError> {
    let pad = " ".repeat(indent);
    match node {
        IdocNativeNode::Group(group) => {
            writeln!(output, "{pad}BEGIN_GROUP {}", group.number()).unwrap();
            render_occurrence(
                output,
                &pad,
                group.level(),
                group.status(),
                group.loop_min(),
                group.loop_max(),
            );
            *lines += 5;
            for child in group.children() {
                render_node(output, child, indent + 2, lines)?;
            }
            writeln!(output, "{pad}END_GROUP").unwrap();
            *lines += 1;
        }
        IdocNativeNode::Segment(segment) => {
            writeln!(output, "{pad}BEGIN_SEGMENT {}", segment.record_name()).unwrap();
            writeln!(output, "{pad}  SEGMENTTYPE {}", segment.segment_type()).unwrap();
            *lines += 2;
            if segment.qualified() {
                writeln!(output, "{pad}  QUALIFIED").unwrap();
                *lines += 1;
            }
            render_occurrence(
                output,
                &pad,
                segment.level(),
                segment.status(),
                segment.loop_min(),
                segment.loop_max(),
            );
            writeln!(output, "{pad}  BEGIN_FIELDS").unwrap();
            *lines += 5;
            for field in segment.fields() {
                writeln!(output, "{pad}    NAME {}", field.name()).unwrap();
                writeln!(output, "{pad}    TEXT {}", field.text()).unwrap();
                writeln!(output, "{pad}    TYPE {}", field.field_type().keyword()).unwrap();
                writeln!(output, "{pad}    LENGTH {:06}", field.length()).unwrap();
                writeln!(output, "{pad}    FIELD_POS {:04}", field.position()).unwrap();
                writeln!(output, "{pad}    BYTE_FIRST {:06}", field.first_byte()).unwrap();
                writeln!(output, "{pad}    BYTE_LAST {:06}", field.last_byte()).unwrap();
                *lines += 7;
                for code in field.codes() {
                    writeln!(output, "{pad}    VALUE '{}'", code.value()).unwrap();
                    writeln!(output, "{pad}    VALUE_TEXT {}", code.text()).unwrap();
                    *lines += 2;
                }
            }
            writeln!(output, "{pad}  END_FIELDS").unwrap();
            writeln!(output, "{pad}END_SEGMENT").unwrap();
            *lines += 2;
        }
    }
    if *lines > MAX_LINES {
        return Err(IdocNativeConfigError::Limit("line count"));
    }
    if output.len() > MAX_IDOC_NATIVE_BYTES {
        return Err(IdocNativeConfigError::Limit("file size"));
    }
    Ok(())
}

fn render_occurrence(
    output: &mut String,
    pad: &str,
    level: NonZeroU32,
    status: IdocNativeStatus,
    min: u64,
    max: u64,
) {
    writeln!(output, "{pad}  LEVEL {:02}", level).unwrap();
    writeln!(output, "{pad}  STATUS {}", status.keyword()).unwrap();
    writeln!(output, "{pad}  LOOPMIN {min:010}").unwrap();
    writeln!(output, "{pad}  LOOPMAX {max:010}").unwrap();
}

#[derive(Clone, Copy)]
struct Line<'a> {
    key: &'a str,
    arg: &'a str,
    number: usize,
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<Line<'a>> {
        self.lines.get(self.cursor).copied()
    }

    fn next(&mut self) -> Result<Line<'a>, IdocNativeConfigError> {
        let line = self
            .peek()
            .ok_or_else(|| self.error("unexpected end of config"))?;
        self.cursor += 1;
        Ok(line)
    }

    fn take_exact(&mut self, key: &str) -> Result<(), IdocNativeConfigError> {
        let line = self.next()?;
        if line.key != key || !line.arg.is_empty() {
            return Err(self.line_error(line, format!("expected `{key}` without arguments")));
        }
        Ok(())
    }

    fn take_value(&mut self, key: &str) -> Result<&'a str, IdocNativeConfigError> {
        let line = self.next()?;
        if line.key != key || line.arg.is_empty() {
            return Err(self.line_error(line, format!("expected `{key}` with a value")));
        }
        Ok(line.arg)
    }

    fn error(&self, reason: impl Into<String>) -> IdocNativeConfigError {
        IdocNativeConfigError::Invalid(reason.into())
    }

    fn line_error(&self, line: Line<'_>, reason: impl Into<String>) -> IdocNativeConfigError {
        self.error(format!("line {}: {}", line.number, reason.into()))
    }

    fn parse_nodes(
        &mut self,
        end: &str,
        depth: usize,
    ) -> Result<Vec<IdocNativeNode>, IdocNativeConfigError> {
        if depth > MAX_IDOC_NATIVE_DEPTH {
            return Err(IdocNativeConfigError::Limit("nesting depth"));
        }
        let mut nodes = Vec::new();
        loop {
            let line = self
                .peek()
                .ok_or_else(|| self.error(format!("missing `{end}`")))?;
            if line.key == end {
                self.take_exact(end)?;
                return Ok(nodes);
            }
            let node = match line.key {
                "BEGIN_GROUP" => IdocNativeNode::Group(self.parse_group(depth + 1)?),
                "BEGIN_SEGMENT" => IdocNativeNode::Segment(self.parse_segment()?),
                _ => {
                    return Err(
                        self.line_error(line, format!("unexpected `{}` in IDoc body", line.key))
                    );
                }
            };
            nodes.push(node);
            if nodes.len() > mapping::MAX_IDOC_NATIVE_NODES {
                return Err(IdocNativeConfigError::Limit("node count"));
            }
        }
    }

    fn parse_group(&mut self, depth: usize) -> Result<IdocNativeGroup, IdocNativeConfigError> {
        let number = parse_nonzero(self.take_value("BEGIN_GROUP")?, "BEGIN_GROUP")?;
        let mut header = Header::default();
        while let Some(line) = self.peek() {
            if matches!(line.key, "BEGIN_GROUP" | "BEGIN_SEGMENT" | "END_GROUP") {
                break;
            }
            header.accept(self.next()?)?;
        }
        let (level, status, min, max) = header.complete("group")?;
        let children = self.parse_nodes("END_GROUP", depth)?;
        IdocNativeGroup::new(number, level, status, min, max, children)
    }

    fn parse_segment(&mut self) -> Result<IdocNativeSegment, IdocNativeConfigError> {
        let record_name = self.take_value("BEGIN_SEGMENT")?.to_owned();
        let mut header = Header::default();
        let mut segment_type = None;
        let mut qualified = false;
        loop {
            let line = self.peek().ok_or_else(|| {
                self.error(format!("segment `{record_name}` has no BEGIN_FIELDS"))
            })?;
            if line.key == "BEGIN_FIELDS" {
                self.take_exact("BEGIN_FIELDS")?;
                break;
            }
            let line = self.next()?;
            match line.key {
                "SEGMENTTYPE" => set_once(&mut segment_type, line, "SEGMENTTYPE", true)?,
                "QUALIFIED" if line.arg.is_empty() && !qualified => qualified = true,
                "LEVEL" | "STATUS" | "LOOPMIN" | "LOOPMAX" => header.accept(line)?,
                _ => {
                    return Err(self.line_error(
                        line,
                        format!("unsupported segment directive `{}`", line.key),
                    ));
                }
            }
        }
        let (level, status, min, max) = header.complete("segment")?;
        let segment_type = segment_type.ok_or_else(|| self.error("segment lacks SEGMENTTYPE"))?;
        let fields = self.parse_fields()?;
        self.take_exact("END_SEGMENT")?;
        IdocNativeSegment::new(
            record_name,
            segment_type,
            qualified,
            level,
            status,
            min,
            max,
            fields,
        )
    }

    fn parse_fields(&mut self) -> Result<Vec<IdocNativeField>, IdocNativeConfigError> {
        let mut fields = Vec::new();
        loop {
            let line = self
                .peek()
                .ok_or_else(|| self.error("missing END_FIELDS"))?;
            if line.key == "END_FIELDS" {
                self.take_exact("END_FIELDS")?;
                return Ok(fields);
            }
            let name = self.take_value("NAME")?.to_owned();
            fields.push(self.parse_field(name)?);
            if fields.len() > mapping::MAX_IDOC_FIELDS {
                return Err(IdocNativeConfigError::Limit("field count"));
            }
        }
    }

    fn parse_field(&mut self, name: String) -> Result<IdocNativeField, IdocNativeConfigError> {
        let mut text = None;
        let mut field_type = None;
        let mut length = None;
        let mut position = None;
        let mut first = None;
        let mut last = None;
        let mut codes = Vec::new();
        while let Some(line) = self.peek() {
            if matches!(line.key, "NAME" | "END_FIELDS") {
                break;
            }
            let line = self.next()?;
            match line.key {
                "TEXT" => set_once(&mut text, line, "TEXT", false)?,
                "TYPE" => {
                    if field_type.is_some() || line.arg != "CHARACTER" {
                        return Err(self.line_error(line, "only one TYPE CHARACTER is supported"));
                    }
                    field_type = Some(IdocNativeFieldType::Character);
                }
                "LENGTH" => set_number(&mut length, line)?,
                "FIELD_POS" => set_number(&mut position, line)?,
                "BYTE_FIRST" => set_number(&mut first, line)?,
                "BYTE_LAST" => set_number(&mut last, line)?,
                "VALUE" => {
                    let value = parse_quoted_code(line)?;
                    let description = self.next()?;
                    if description.key != "VALUE_TEXT" {
                        return Err(
                            self.line_error(description, "VALUE must be followed by VALUE_TEXT")
                        );
                    }
                    codes.push(IdocNativeCode::new(value, description.arg)?);
                    if codes.len() > mapping::MAX_IDOC_NATIVE_CODES {
                        return Err(IdocNativeConfigError::Limit("code count"));
                    }
                }
                _ => {
                    return Err(self
                        .line_error(line, format!("unsupported field directive `{}`", line.key)));
                }
            }
        }
        let missing = |label| self.error(format!("field `{name}` lacks {label}"));
        let text = text.ok_or_else(|| missing("TEXT"))?;
        let field_type = field_type.ok_or_else(|| missing("TYPE"))?;
        let length = length.ok_or_else(|| missing("LENGTH"))?;
        let position = position.ok_or_else(|| missing("FIELD_POS"))?;
        let first = first.ok_or_else(|| missing("BYTE_FIRST"))?;
        let last = last.ok_or_else(|| missing("BYTE_LAST"))?;
        IdocNativeField::new(name, text, field_type, length, position, first, last, codes)
    }
}

#[derive(Default)]
struct Header {
    level: Option<NonZeroU32>,
    status: Option<IdocNativeStatus>,
    min: Option<u64>,
    max: Option<u64>,
}

impl Header {
    fn accept(&mut self, line: Line<'_>) -> Result<(), IdocNativeConfigError> {
        match line.key {
            "LEVEL" => set_number(&mut self.level, line),
            "STATUS" => {
                if self.status.is_some() {
                    return Err(invalid_line(line, "duplicate STATUS"));
                }
                self.status = Some(match line.arg {
                    "MANDATORY" => IdocNativeStatus::Mandatory,
                    "OPTIONAL" => IdocNativeStatus::Optional,
                    _ => return Err(invalid_line(line, "unsupported STATUS")),
                });
                Ok(())
            }
            "LOOPMIN" => set_number(&mut self.min, line),
            "LOOPMAX" => set_number(&mut self.max, line),
            _ => Err(invalid_line(
                line,
                format!("unsupported header directive `{}`", line.key),
            )),
        }
    }

    fn complete(
        self,
        kind: &str,
    ) -> Result<(NonZeroU32, IdocNativeStatus, u64, u64), IdocNativeConfigError> {
        let missing = |field| IdocNativeConfigError::Invalid(format!("{kind} lacks {field}"));
        Ok((
            self.level.ok_or_else(|| missing("LEVEL"))?,
            self.status.ok_or_else(|| missing("STATUS"))?,
            self.min.ok_or_else(|| missing("LOOPMIN"))?,
            self.max.ok_or_else(|| missing("LOOPMAX"))?,
        ))
    }
}

fn set_once(
    slot: &mut Option<String>,
    line: Line<'_>,
    name: &str,
    required_value: bool,
) -> Result<(), IdocNativeConfigError> {
    if slot.is_some() || (required_value && line.arg.is_empty()) {
        return Err(invalid_line(line, format!("duplicate or empty {name}")));
    }
    *slot = Some(line.arg.to_owned());
    Ok(())
}

fn set_number<T: std::str::FromStr>(
    slot: &mut Option<T>,
    line: Line<'_>,
) -> Result<(), IdocNativeConfigError> {
    if slot.is_some() {
        return Err(invalid_line(line, format!("duplicate {}", line.key)));
    }
    if line.arg.is_empty() || !line.arg.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_line(line, format!("invalid {}", line.key)));
    }
    *slot = Some(
        line.arg
            .parse()
            .map_err(|_| invalid_line(line, format!("invalid {}", line.key)))?,
    );
    Ok(())
}

fn parse_nonzero(value: &str, name: &str) -> Result<NonZeroU32, IdocNativeConfigError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(IdocNativeConfigError::Invalid(format!("invalid {name}")));
    }
    value
        .parse()
        .map_err(|_| IdocNativeConfigError::Invalid(format!("invalid {name}")))
}

fn parse_quoted_code(line: Line<'_>) -> Result<&str, IdocNativeConfigError> {
    if line.arg.len() < 2 || !line.arg.starts_with('\'') || !line.arg.ends_with('\'') {
        return Err(invalid_line(line, "VALUE must be a single-quoted literal"));
    }
    let value = &line.arg[1..line.arg.len() - 1];
    if value.contains('\'') {
        return Err(invalid_line(line, "escaped apostrophes are unsupported"));
    }
    Ok(value)
}

fn invalid_line(line: Line<'_>, reason: impl Into<String>) -> IdocNativeConfigError {
    IdocNativeConfigError::Invalid(format!("line {}: {}", line.number, reason.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic() -> &'static str {
        "BEGIN_SEGMENT_SECTION\nBEGIN_IDOC TEST01\nBEGIN_GROUP 1\nLEVEL 01\nSTATUS MANDATORY\nLOOPMIN 0000000001\nLOOPMAX 0000000001\nBEGIN_SEGMENT E2ITEM\nSEGMENTTYPE E1ITEM\nQUALIFIED\nLEVEL 02\nSTATUS OPTIONAL\nLOOPMIN 0000000000\nLOOPMAX 9999999999\nBEGIN_FIELDS\nNAME CODE\nTEXT Code description\nTYPE CHARACTER\nLENGTH 000003\nFIELD_POS 0001\nBYTE_FIRST 000064\nBYTE_LAST 000066\nVALUE ''\nVALUE_TEXT Unset\nVALUE 'A'\nVALUE_TEXT Active\nEND_FIELDS\nEND_SEGMENT\nEND_GROUP\nEND_IDOC\nEND_SEGMENT_SECTION\n"
    }

    #[test]
    fn strict_roundtrip_retains_ordered_metadata_and_large_loop_max() {
        let descriptor = parse_native_config(synthetic()).unwrap();
        let rendered = render_native_config(&descriptor).unwrap();
        assert_eq!(parse_native_config(&rendered).unwrap(), descriptor);
        let IdocNativeNode::Group(group) = &descriptor.nodes()[0] else {
            panic!("expected group")
        };
        assert!(!group.status().is_optional());
        let IdocNativeNode::Segment(segment) = &group.children()[0] else {
            panic!("expected segment")
        };
        assert_eq!(segment.record_name(), "E2ITEM");
        assert_eq!(segment.segment_type(), "E1ITEM");
        assert!(segment.qualified());
        assert_eq!(segment.loop_max(), 9_999_999_999);
        assert_eq!(segment.fields()[0].codes()[0].value(), "");
        assert_eq!(segment.fields()[0].codes()[1].text(), "Active");
        let (schema, layout) = descriptor.project().unwrap();
        assert!(!schema.child("SG1").unwrap().repeating);
        assert!(
            schema
                .child("SG1")
                .unwrap()
                .child("E2ITEM")
                .unwrap()
                .repeating
        );
        assert_eq!(
            layout.segment("E2ITEM").unwrap().fields()[0]
                .first_byte()
                .get(),
            64
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_native_idoc_rendered_{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, rendered).unwrap();
        let imported = crate::config::idoc::import_config(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!((schema, layout), (imported.schema, imported.layout));
        assert_eq!(imported.native, Some(descriptor));
    }

    #[test]
    fn strict_parser_rejects_unaccounted_directives_and_malformed_metadata() {
        for malformed in [
            synthetic().replace("QUALIFIED\n", "OTHER_DIRECTIVE x\n"),
            synthetic().replace("TYPE CHARACTER", "TYPE DECIMAL"),
            synthetic().replace("VALUE_TEXT Active", "TEXT Active"),
            synthetic().replace("VALUE 'A'", "VALUE A"),
            synthetic().replace("LENGTH 000003", "LENGTH 000004"),
            synthetic().replace("BYTE_FIRST 000064", "BYTE_FIRST 000000"),
            synthetic().replace("BYTE_LAST 000066", "BYTE_LAST 1048577"),
            synthetic().replace("LOOPMAX 9999999999", "LOOPMAX 18446744073709551616"),
            synthetic().replace("END_SEGMENT_SECTION", "TRAILER x"),
            synthetic().replace(
                "TEXT Code description",
                "TEXT Code description\nTEXT Duplicate",
            ),
        ] {
            assert!(
                parse_native_config(&malformed).is_err(),
                "accepted:\n{malformed}"
            );
        }
    }

    #[test]
    fn strict_parser_preserves_utf8_code_text_and_checks_line_budget() {
        let text = synthetic()
            .replace("TEXT Code description", "TEXT Código  ")
            .replace("VALUE 'A'", "VALUE 'é'")
            .replace("VALUE_TEXT Active", "VALUE_TEXT Été");
        let descriptor = parse_native_config(&text).unwrap();
        assert_eq!(
            parse_native_config(&render_native_config(&descriptor).unwrap()).unwrap(),
            descriptor
        );

        let many_lines = format!("{}{}", "\n".repeat(MAX_LINES), synthetic());
        assert!(matches!(
            parse_native_config(&many_lines),
            Err(IdocNativeConfigError::Limit("line count"))
        ));
    }

    #[test]
    fn unsupported_provenance_does_not_change_legacy_executable_import() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_native_idoc_{}_{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, synthetic()).unwrap();
        let certified = crate::config::idoc::import_config(&path).unwrap();
        assert!(certified.native.is_some());
        std::fs::write(
            &path,
            synthetic().replace("QUALIFIED", "UNSUPPORTED_DIRECTIVE x"),
        )
        .unwrap();
        let legacy = crate::config::idoc::import_config(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(legacy.native.is_none());
        assert_eq!(legacy.schema, certified.schema);
        assert_eq!(legacy.layout, certified.layout);
    }

    #[test]
    fn local_reference_config_can_be_certified_without_copying_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/ReferenceSamples/ORDERS01-Parseridoc.txt");
        if !path.is_file() {
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let descriptor = parse_native_config(&text).unwrap();
        let rendered = render_native_config(&descriptor).unwrap();
        assert_eq!(parse_native_config(&rendered).unwrap(), descriptor);
        let compiled = crate::config::idoc::import_config(&path).unwrap();
        assert_eq!(compiled.native.as_ref(), Some(&descriptor));
        assert_eq!(
            descriptor.project().unwrap(),
            (compiled.schema, compiled.layout)
        );
    }
}
