use std::collections::{BTreeMap, BTreeSet};

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use super::{Fact, LINK, TargetRow, Unit, XBRLDI, XBRLI, XbrlFormatError};

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
    matches!(
        namespace,
        XBRLI | XBRLDI | LINK | super::MAPFORCE_VIEW | "view"
    )
}

pub(super) fn render_target(
    taxonomy: &str,
    rows: &[TargetRow],
    units: &[Unit],
    prefixes: &Prefixes,
) -> Result<String, XbrlFormatError> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
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
    String::from_utf8(writer.into_inner()).map_err(|error| {
        XbrlFormatError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

fn write_unit(
    writer: &mut Writer<Vec<u8>>,
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

fn write_context(
    writer: &mut Writer<Vec<u8>>,
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
    writer.write_event(Event::Text(BytesText::new(&row.identifier.value)))?;
    writer.write_event(Event::End(BytesEnd::new("xbrli:identifier")))?;
    if !row.dimensions.is_empty() {
        writer.write_event(Event::Start(BytesStart::new("xbrli:segment")))?;
        for dimension in &row.dimensions {
            let mut member = BytesStart::new("xbrldi:explicitMember");
            let dimension_name = prefixes.qname(&dimension.namespace, &dimension.name);
            member.push_attribute(("dimension", dimension_name.as_str()));
            writer.write_event(Event::Start(member))?;
            writer.write_event(Event::Text(BytesText::new(
                &prefixes.lexical_qname(&dimension.member),
            )))?;
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

fn write_fact(
    writer: &mut Writer<Vec<u8>>,
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
    writer.write_event(Event::Text(BytesText::new(&fact.value)))?;
    writer.write_event(Event::End(BytesEnd::new(&name)))?;
    Ok(())
}

fn write_text_element(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    value: &str,
) -> Result<(), XbrlFormatError> {
    writer.write_event(Event::Start(BytesStart::new(name)))?;
    writer.write_event(Event::Text(BytesText::new(value)))?;
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}
