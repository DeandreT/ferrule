use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use super::{Fact, LINK, TargetRow, Unit, XBRLDI, XBRLI, XbrlFormatError};
use crate::bounded::{XbrlLimit, XbrlLimitOptions, XmlBudget, check};

const XLINK: &str = "http://www.w3.org/1999/xlink";
pub(super) struct Prefixes {
    by_uri: BTreeMap<String, String>,
}

impl Prefixes {
    pub(super) fn new(uris: BTreeSet<String>) -> Self {
        let mut by_uri = BTreeMap::new();
        let mut index = 1usize;
        for uri in uris {
            let prefix = match uri.as_str() {
                XBRLI => "xbrli".to_string(),
                XBRLDI => "xbrldi".to_string(),
                LINK => "link".to_string(),
                XLINK => "xlink".to_string(),
                "http://www.xbrl.org/2003/iso4217" => "iso4217".to_string(),
                _ => {
                    let prefix = format!("ns{index}");
                    index += 1;
                    prefix
                }
            };
            by_uri.insert(uri, prefix);
        }
        Self { by_uri }
    }

    fn prefix(&self, uri: &str) -> &str {
        self.by_uri.get(uri).map_or("ns", String::as_str)
    }

    fn qname(&self, namespace: &str, local: &str) -> String {
        format!("{}:{local}", self.prefix(namespace))
    }

    fn lexical_qname(&self, value: &str) -> String {
        expanded_qname(value).map_or_else(
            || value.to_string(),
            |(namespace, lexical)| {
                let local = lexical.rsplit_once(':').map_or(lexical, |(_, local)| local);
                self.qname(namespace, local)
            },
        )
    }
}

pub(super) fn expanded_qname(value: &str) -> Option<(&str, &str)> {
    let value = value.strip_prefix('{')?;
    let (namespace, lexical) = value.split_once('}')?;
    (!namespace.is_empty() && !lexical.is_empty()).then_some((namespace, lexical))
}

pub(super) fn is_structural_namespace(namespace: &str) -> bool {
    matches!(namespace, XBRLI | XBRLDI | LINK | super::MFD_VIEW | "view")
}

pub(super) fn render_target(
    taxonomy: &str,
    rows: &[TargetRow],
    units: &[Unit],
    prefixes: &Prefixes,
) -> Result<String, XbrlFormatError> {
    let bytes = render_into(Vec::new(), taxonomy, rows, units, prefixes, None)?;
    String::from_utf8(bytes).map_err(|error| {
        XbrlFormatError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

pub(super) fn render_bounded<W: Write>(
    sink: W,
    taxonomy: &str,
    rows: &[TargetRow],
    units: &[Unit],
    prefixes: &Prefixes,
    limits: &XbrlLimitOptions,
) -> Result<(W, usize), XbrlFormatError> {
    let sink = LimitedWriter {
        inner: sink,
        count: 0,
        maximum: limits.max_output_bytes,
    };
    let result = render_into(sink, taxonomy, rows, units, prefixes, Some(limits));
    match result {
        Ok(sink) => Ok((sink.inner, sink.count)),
        Err(XbrlFormatError::Io(error)) => {
            if let Some(limit) = error
                .get_ref()
                .and_then(|e| e.downcast_ref::<OutputLimit>())
            {
                return Err(XbrlFormatError::LimitExceeded {
                    limit: XbrlLimit::OutputBytes,
                    maximum: limit.maximum,
                    actual: limit.actual,
                });
            }
            Err(XbrlFormatError::Io(error))
        }
        Err(error) => Err(error),
    }
}

fn render_into<W: Write>(
    sink: W,
    taxonomy: &str,
    rows: &[TargetRow],
    units: &[Unit],
    prefixes: &Prefixes,
    limits: Option<&XbrlLimitOptions>,
) -> Result<W, XbrlFormatError> {
    if let Some(limits) = limits {
        check(XbrlLimit::Units, units.len(), limits.max_units)?;
    }
    let mut writer = TargetWriter {
        inner: Writer::new_with_indent(sink, b' ', 2),
        budget: limits.map(XmlBudget::new).transpose()?,
        depth: 0,
        line_break: false,
    };
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    let mut root = BytesStart::new("xbrli:xbrl");
    root.push_attribute(("xmlns:xbrli", XBRLI));
    root.push_attribute(("xmlns:xbrldi", XBRLDI));
    root.push_attribute(("xmlns:link", LINK));
    root.push_attribute(("xmlns:xlink", XLINK));
    for (namespace, prefix) in &prefixes.by_uri {
        if matches!(namespace.as_str(), XBRLI | XBRLDI | LINK | XLINK) {
            continue;
        }
        root.push_attribute((format!("xmlns:{prefix}").as_str(), namespace.as_str()));
    }
    writer.write_event(Event::Start(root))?;

    let mut schema_ref = BytesStart::new("link:schemaRef");
    schema_ref.push_attribute(("xlink:type", "simple"));
    schema_ref.push_attribute(("xlink:href", taxonomy.replace('\\', "/").as_str()));
    writer.write_event(Event::Empty(schema_ref))?;

    for unit in units {
        write_unit(&mut writer, unit, prefixes)?;
    }
    for (index, row) in rows.iter().enumerate() {
        let context_id = format!("c{}", index + 1);
        write_context(&mut writer, &context_id, row, prefixes)?;
        let mut seen = BTreeSet::new();
        for fact in &row.facts {
            if !seen.insert((fact.namespace.clone(), fact.name.clone())) {
                return Err(XbrlFormatError::DuplicateFact {
                    context: context_id,
                    concept: fact.name.clone(),
                });
            }
            write_fact(&mut writer, &context_id, fact, prefixes)?;
        }
    }
    writer.write_event(Event::End(BytesEnd::new("xbrli:xbrl")))?;
    Ok(writer.inner.into_inner())
}

fn write_unit<W: Write>(
    writer: &mut TargetWriter<'_, W>,
    unit: &Unit,
    prefixes: &Prefixes,
) -> Result<(), XbrlFormatError> {
    let mut start = BytesStart::new("xbrli:unit");
    start.push_attribute(("id", unit.id.as_str()));
    writer.write_event(Event::Start(start))?;
    if let Some(measure) = &unit.measure {
        write_text_element(writer, "xbrli:measure", &prefixes.lexical_qname(measure))?;
    } else if let (Some(numerator), Some(denominator)) = (&unit.numerator, &unit.denominator) {
        writer.write_event(Event::Start(BytesStart::new("xbrli:divide")))?;
        writer.write_event(Event::Start(BytesStart::new("xbrli:unitNumerator")))?;
        write_text_element(writer, "xbrli:measure", &prefixes.lexical_qname(numerator))?;
        writer.write_event(Event::End(BytesEnd::new("xbrli:unitNumerator")))?;
        writer.write_event(Event::Start(BytesStart::new("xbrli:unitDenominator")))?;
        write_text_element(
            writer,
            "xbrli:measure",
            &prefixes.lexical_qname(denominator),
        )?;
        writer.write_event(Event::End(BytesEnd::new("xbrli:unitDenominator")))?;
        writer.write_event(Event::End(BytesEnd::new("xbrli:divide")))?;
    }
    writer.write_event(Event::End(BytesEnd::new("xbrli:unit")))?;
    Ok(())
}

fn write_context<W: Write>(
    writer: &mut TargetWriter<'_, W>,
    id: &str,
    row: &TargetRow,
    prefixes: &Prefixes,
) -> Result<(), XbrlFormatError> {
    let mut context = BytesStart::new("xbrli:context");
    context.push_attribute(("id", id));
    writer.write_event(Event::Start(context))?;
    writer.write_event(Event::Start(BytesStart::new("xbrli:entity")))?;
    let mut identifier = BytesStart::new("xbrli:identifier");
    identifier.push_attribute(("scheme", row.identifier.scheme.as_str()));
    writer.write_event(Event::Start(identifier))?;
    write_text(writer, &row.identifier.value)?;
    writer.write_event(Event::End(BytesEnd::new("xbrli:identifier")))?;
    if !row.dimensions.is_empty() {
        writer.write_event(Event::Start(BytesStart::new("xbrli:segment")))?;
        for dimension in &row.dimensions {
            let mut member = BytesStart::new("xbrldi:explicitMember");
            let dimension_name = prefixes.qname(&dimension.namespace, &dimension.name);
            member.push_attribute(("dimension", dimension_name.as_str()));
            writer.write_event(Event::Start(member))?;
            write_text(writer, &prefixes.lexical_qname(&dimension.member))?;
            writer.write_event(Event::End(BytesEnd::new("xbrldi:explicitMember")))?;
        }
        writer.write_event(Event::End(BytesEnd::new("xbrli:segment")))?;
    }
    writer.write_event(Event::End(BytesEnd::new("xbrli:entity")))?;
    writer.write_event(Event::Start(BytesStart::new("xbrli:period")))?;
    if let (Some(start), Some(end)) = (&row.period.start, &row.period.end) {
        write_text_element(writer, "xbrli:startDate", start)?;
        write_text_element(writer, "xbrli:endDate", end)?;
    } else if let Some(instant) = &row.period.instant {
        write_text_element(writer, "xbrli:instant", instant)?;
    } else {
        writer.write_event(Event::Empty(BytesStart::new("xbrli:forever")))?;
    }
    writer.write_event(Event::End(BytesEnd::new("xbrli:period")))?;
    writer.write_event(Event::End(BytesEnd::new("xbrli:context")))?;
    Ok(())
}

fn write_fact<W: Write>(
    writer: &mut TargetWriter<'_, W>,
    context_id: &str,
    fact: &Fact,
    prefixes: &Prefixes,
) -> Result<(), XbrlFormatError> {
    let name = prefixes.qname(&fact.namespace, &fact.name);
    let mut start = BytesStart::new(name.as_str());
    start.push_attribute(("contextRef", context_id));
    if let Some(unit_ref) = &fact.unit_ref {
        start.push_attribute(("unitRef", unit_ref.as_str()));
    }
    if let Some(decimals) = &fact.decimals {
        start.push_attribute(("decimals", decimals.as_str()));
    }
    writer.write_event(Event::Start(start))?;
    write_text(writer, &fact.value)?;
    writer.write_event(Event::End(BytesEnd::new(&name)))?;
    Ok(())
}

fn write_text_element<W: Write>(
    writer: &mut TargetWriter<'_, W>,
    name: &str,
    value: &str,
) -> Result<(), XbrlFormatError> {
    writer.write_event(Event::Start(BytesStart::new(name)))?;
    write_text(writer, value)?;
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

// Stream the same five escapes as BytesText::new without retaining an escaped
// copy of an arbitrarily large value in the counting pass.
fn write_text<W: Write>(
    writer: &mut TargetWriter<'_, W>,
    text: &str,
) -> Result<(), XbrlFormatError> {
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        let reference = match byte {
            b'&' => "amp",
            b'<' => "lt",
            b'>' => "gt",
            b'\'' => "apos",
            b'\"' => "quot",
            _ => continue,
        };
        if start < index {
            writer.write_event(Event::Text(BytesText::from_escaped(&text[start..index])))?;
        }
        writer.write_event(Event::GeneralRef(quick_xml::events::BytesRef::new(
            reference,
        )))?;
        start = index + 1;
    }
    // An empty Text event preserves the legacy writer's no-newline behavior for
    // present empty values; the budget does not count it as an XML node.
    writer.write_event(Event::Text(BytesText::from_escaped(&text[start..])))
}

struct TargetWriter<'a, W> {
    inner: Writer<W>,
    budget: Option<XmlBudget<'a>>,
    depth: usize,
    line_break: bool,
}

impl<W: Write> TargetWriter<'_, W> {
    fn write_event(&mut self, event: Event<'_>) -> Result<(), XbrlFormatError> {
        let text = matches!(event, Event::Text(_) | Event::GeneralRef(_));
        if matches!(event, Event::End(_)) {
            self.depth = self.depth.saturating_sub(1);
        }
        if let Some(budget) = &mut self.budget {
            if self.line_break && !text {
                budget.whitespace(1 + 2 * self.depth)?;
            }
            let namespace = match &event {
                Event::Start(start) | Event::Empty(start) => {
                    if start.name().as_ref().starts_with(b"xbrli:") {
                        Some(XBRLI.as_bytes())
                    } else if start.name().as_ref().starts_with(b"xbrldi:") {
                        Some(XBRLDI.as_bytes())
                    } else if start.name().as_ref().starts_with(b"link:") {
                        Some(LINK.as_bytes())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            budget.event(&event, namespace)?;
        }
        if matches!(event, Event::Start(_)) {
            self.depth += 1;
        }
        self.line_break = !text;
        if let Event::GeneralRef(reference) = event {
            // quick-xml indents GeneralRef as markup; encode it as text pieces
            // to keep the legacy BytesText byte stream and indentation exactly.
            self.inner
                .write_event(Event::Text(BytesText::from_escaped("&")))?;
            let name = std::str::from_utf8(reference.as_ref())
                .map_err(|_| std::io::Error::other("invalid reference"))?;
            self.inner
                .write_event(Event::Text(BytesText::from_escaped(name)))?;
            self.inner
                .write_event(Event::Text(BytesText::from_escaped(";")))?;
        } else {
            self.inner.write_event(event)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct OutputLimit {
    maximum: usize,
    actual: usize,
}
impl std::fmt::Display for OutputLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XBRL output exceeds {} bytes", self.maximum)
    }
}
impl std::error::Error for OutputLimit {}

struct LimitedWriter<W> {
    inner: W,
    count: usize,
    maximum: usize,
}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let actual = self.count.saturating_add(bytes.len());
        if actual > self.maximum {
            return Err(std::io::Error::other(OutputLimit {
                maximum: self.maximum,
                actual,
            }));
        }
        let written = self.inner.write(bytes)?;
        self.count += written;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_text_matches_original_quick_xml_bytes() {
        for text in [
            "",
            "ordinary",
            "&<>'\"",
            "a&b<c>d'e\"f",
            "é\n\r\t中",
            "&&a&&",
        ] {
            let mut old = Writer::new_with_indent(Vec::new(), b' ', 2);
            old.write_event(Event::Start(BytesStart::new("root")))
                .unwrap();
            old.write_event(Event::Start(BytesStart::new("value")))
                .unwrap();
            old.write_event(Event::Text(BytesText::new(text))).unwrap();
            old.write_event(Event::End(BytesEnd::new("value"))).unwrap();
            old.write_event(Event::End(BytesEnd::new("root"))).unwrap();
            let mut new = TargetWriter {
                inner: Writer::new_with_indent(Vec::new(), b' ', 2),
                budget: None,
                depth: 0,
                line_break: false,
            };
            new.write_event(Event::Start(BytesStart::new("root")))
                .unwrap();
            new.write_event(Event::Start(BytesStart::new("value")))
                .unwrap();
            write_text(&mut new, text).unwrap();
            new.write_event(Event::End(BytesEnd::new("value"))).unwrap();
            new.write_event(Event::End(BytesEnd::new("root"))).unwrap();
            assert_eq!(new.inner.into_inner(), old.into_inner(), "{text:?}");
        }
    }
}
