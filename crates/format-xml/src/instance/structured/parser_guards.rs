//! Resource preflight used only by the ordinary structured input profile.

use std::collections::HashSet;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use super::{XmlFormatError, limit};

const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone, Copy)]
struct ParserLimits {
    reservation_slots: u64,
    namespace_references: u64,
    structural_work: u64,
    registry_bytes: u64,
}

impl Default for ParserLimits {
    fn default() -> Self {
        Self {
            reservation_slots: ir::MAX_STRUCTURED_XML_PARSER_RESERVATION_SLOTS as u64,
            namespace_references: ir::MAX_STRUCTURED_XML_NAMESPACE_REFERENCES as u64,
            structural_work: ir::MAX_STRUCTURED_XML_PARSER_WORK as u64,
            registry_bytes: ir::MAX_STRUCTURED_XML_NAMESPACE_REGISTRY_BYTES as u64,
        }
    }
}

struct ParserBudget {
    limits: ParserLimits,
    namespace_references: u64,
    structural_work: u64,
    registry_bytes: u64,
    namespaces: HashSet<(Option<String>, String)>,
}

#[cfg(test)]
#[derive(Debug)]
struct ParserStatistics {
    namespace_references: u64,
    structural_work: u64,
    registry_bytes: u64,
    distinct_namespaces: usize,
}

fn check(resource: &'static str, observed_count: u64, limit: u64) -> Result<(), XmlFormatError> {
    if observed_count > limit {
        return Err(XmlFormatError::StructuredParserResourceLimit {
            resource,
            observed_count,
            limit,
        });
    }
    Ok(())
}

impl ParserBudget {
    fn new(limits: ParserLimits) -> Self {
        let mut namespaces = HashSet::new();
        namespaces.insert((Some("xml".to_owned()), XML_URI.to_owned()));
        Self {
            limits,
            namespace_references: 0,
            structural_work: 0,
            registry_bytes: 0,
            namespaces,
        }
    }

    fn work(&mut self, count: u64) -> Result<(), XmlFormatError> {
        self.structural_work = self.structural_work.saturating_add(count);
        check(
            "parser_structural_work",
            self.structural_work,
            self.limits.structural_work,
        )
    }

    fn element(&mut self, start: &BytesStart<'_>, parent: u64) -> Result<u64, XmlFormatError> {
        bmp_name(start.name().as_ref())?;
        let mut attributes = 0_u64;
        let mut declarations = 0_u64;
        // The dependency's duplicate-name checker must not run before the
        // conservative duplicate-comparison charge has been checked.
        for attribute in start.attributes().with_checks(false) {
            let attribute = attribute.map_err(quick_xml::Error::from)?;
            let key = attribute.key.as_ref();
            bmp_name(key)?;
            attributes = attributes.saturating_add(1);
            if key == b"xmlns:xmlns" {
                return Err(XmlFormatError::StructuredInputNamespaceAttribute);
            }
            if namespace_prefix(key).is_some() {
                declarations = declarations.saturating_add(1);
            } else if key.ends_with(b":xmlns") {
                return Err(XmlFormatError::StructuredInputNamespaceAttribute);
            }
        }
        let in_scope = parent.saturating_add(declarations);
        if declarations != 0 {
            self.namespace_references = self.namespace_references.saturating_add(in_scope);
            check(
                "namespace_references",
                self.namespace_references,
                self.limits.namespace_references,
            )?;
        }
        let duplicate_work = attributes.saturating_mul(attributes.saturating_add(1)) / 2;
        let copy_work = if declarations == 0 {
            0
        } else {
            parent.saturating_mul(in_scope)
        };
        let lookup_work = attributes.saturating_add(1).saturating_mul(in_scope);
        self.work(
            duplicate_work
                .saturating_add(copy_work)
                .saturating_add(lookup_work),
        )?;

        // Only after the lower bounds pass may normalization allocate a value
        // or the global registry retain a new pair. No scope maps are cloned.
        for attribute in start.attributes().with_checks(false) {
            let attribute = attribute.map_err(quick_xml::Error::from)?;
            let Some(prefix) = namespace_prefix(attribute.key.as_ref()) else {
                continue;
            };
            let uri = attribute.normalized_value_with(
                XmlVersion::Explicit1_0,
                1,
                quick_xml::escape::resolve_xml_entity,
            )?;
            let prefix = prefix
                .map(|prefix| std::str::from_utf8(prefix).map(str::to_owned))
                .transpose()
                .map_err(|_| XmlFormatError::StructuredInputPhysicalName)?;
            // One temporary owned lookup key is bounded by the original
            // disjoint prefix/value spans. Repeats drop it instead of retaining
            // another decoded string. The URI normalizer has no DTD resolver.
            let key = (prefix, uri.into_owned());
            let distinct = self.namespaces.len() as u64;
            if self.namespaces.contains(&key) {
                let bit_width = u64::BITS - distinct.leading_zeros();
                self.work(u64::from(bit_width) + 1)?;
            } else {
                self.work(distinct.saturating_mul(2))?;
                let bytes = key.0.as_ref().map_or(0, String::len) as u64 + key.1.len() as u64;
                self.registry_bytes = self.registry_bytes.saturating_add(bytes);
                check(
                    "namespace_registry_utf8_bytes",
                    self.registry_bytes,
                    self.limits.registry_bytes,
                )?;
                self.namespaces.insert(key);
            }
        }
        Ok(in_scope)
    }
}

// Some(None) is a default declaration; Some(Some(prefix)) is a named one.
// None is an ordinary attribute. Values are never URI-canonicalized.
fn namespace_prefix(key: &[u8]) -> Option<Option<&[u8]>> {
    if key == b"xmlns" {
        Some(None)
    } else {
        key.strip_prefix(b"xmlns:").map(Some)
    }
}

fn bmp_name(name: &[u8]) -> Result<(), XmlFormatError> {
    if std::str::from_utf8(name).is_ok_and(|name| name.chars().all(|ch| ch as u32 <= 0xFFFF)) {
        Ok(())
    } else {
        Err(XmlFormatError::StructuredInputPhysicalName)
    }
}

pub(super) fn scan_physical_input(text: &str) -> Result<(), XmlFormatError> {
    scan_with_limits(text, ParserLimits::default()).map(|_| ())
}

fn scan_with_limits(text: &str, limits: ParserLimits) -> Result<ParserBudget, XmlFormatError> {
    let reservation_slots = text
        .bytes()
        .filter(|byte| matches!(*byte, b'<' | b'='))
        .count() as u64;
    check(
        "parser_reservation_slots",
        reservation_slots,
        limits.reservation_slots,
    )?;
    let mut budget = ParserBudget::new(limits);
    let mut parents = Vec::new();
    let mut reader = quick_xml::Reader::from_str(text);
    loop {
        match reader.read_event()? {
            Event::Eof => return Ok(budget),
            Event::DocType(_) => return Err(XmlFormatError::StructuredInputDtd),
            Event::Decl(declaration) => {
                if declaration.encoding().is_some_and(|encoding| {
                    !encoding.is_ok_and(|value| value.eq_ignore_ascii_case(b"utf-8"))
                }) {
                    return Err(XmlFormatError::StructuredInputEncoding);
                }
            }
            Event::Start(start) => {
                if parents.len() + 1 > ir::MAX_STRUCTURED_XML_DEPTH {
                    return Err(limit("physical depth", ir::MAX_STRUCTURED_XML_DEPTH));
                }
                let in_scope = budget.element(&start, parents.last().copied().unwrap_or(0))?;
                parents.push(in_scope);
            }
            Event::Empty(start) => {
                if parents.len() + 1 > ir::MAX_STRUCTURED_XML_DEPTH {
                    return Err(limit("physical depth", ir::MAX_STRUCTURED_XML_DEPTH));
                }
                budget.element(&start, parents.last().copied().unwrap_or(0))?;
            }
            Event::End(end) => {
                bmp_name(end.name().as_ref())?;
                parents.pop();
            }
            Event::PI(instruction) => bmp_name(instruction.target())?,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode};

    fn statistics(text: &str) -> ParserStatistics {
        let budget = scan_with_limits(text, ParserLimits::default()).unwrap();
        ParserStatistics {
            namespace_references: budget.namespace_references,
            structural_work: budget.structural_work,
            registry_bytes: budget.registry_bytes,
            distinct_namespaces: budget.namespaces.len(),
        }
    }

    fn resource_error(
        result: Result<ParserBudget, XmlFormatError>,
        id: &str,
        count: u64,
        max: u64,
    ) {
        assert!(
            matches!(result, Err(XmlFormatError::StructuredParserResourceLimit {
            resource, observed_count, limit,
        }) if resource == id && observed_count == count && limit == max)
        );
    }

    #[test]
    fn combined_raw_slots_count_literal_markers_and_empty_cdata_pieces() {
        let limits = ParserLimits {
            reservation_slots: 4,
            ..ParserLimits::default()
        };
        assert!(scan_with_limits("<Root>==</Root>", limits).is_ok());
        resource_error(
            scan_with_limits("<Root>===</Root>", limits),
            "parser_reservation_slots",
            5,
            4,
        );
        resource_error(
            scan_with_limits("<Root><!--<<--></Root>", limits),
            "parser_reservation_slots",
            5,
            4,
        );
        resource_error(
            scan_with_limits("<Root><![CDATA[]]><![CDATA[]]><![CDATA[]]></Root>", limits),
            "parser_reservation_slots",
            5,
            4,
        );
    }

    #[test]
    fn namespace_references_charge_ancestor_events_without_charging_empty_scopes() {
        let text = "<Root xmlns:a='urn:a'><Child xmlns:a='urn:a'/><Child/></Root>";
        let facts = statistics(text);
        assert_eq!(facts.namespace_references, 3);
        assert_eq!(facts.distinct_namespaces, 2);
        assert_eq!(facts.registry_bytes, 6);
        assert_eq!(facts.structural_work, 16);
        let limits = ParserLimits {
            namespace_references: 2,
            ..ParserLimits::default()
        };
        resource_error(scan_with_limits(text, limits), "namespace_references", 3, 2);
        assert!(
            scan_with_limits(
                text,
                ParserLimits {
                    namespace_references: 3,
                    ..ParserLimits::default()
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn structural_comparison_budget_is_checked_before_normalization_and_insertion() {
        let text = "<Root a='1' b='2'/>";
        assert_eq!(statistics(text).structural_work, 3);
        resource_error(
            scan_with_limits(
                text,
                ParserLimits {
                    structural_work: 2,
                    ..ParserLimits::default()
                },
            ),
            "parser_structural_work",
            3,
            2,
        );
        assert!(
            scan_with_limits(
                text,
                ParserLimits {
                    structural_work: 3,
                    ..ParserLimits::default()
                }
            )
            .is_ok()
        );
        // The base work refuses before the invalid URI entity is decoded.
        resource_error(
            scan_with_limits(
                "<Root xmlns:a='&unknown;' x='1'/>",
                ParserLimits {
                    structural_work: 0,
                    ..ParserLimits::default()
                },
            ),
            "parser_structural_work",
            6,
            0,
        );
    }

    #[test]
    fn registry_bytes_are_prepaid_only_for_new_normalized_pairs() {
        let text = "<Root xmlns:a='urn:a'><Child xmlns:a='urn:&#97;'/></Root>";
        resource_error(
            scan_with_limits(
                text,
                ParserLimits {
                    registry_bytes: 5,
                    ..ParserLimits::default()
                },
            ),
            "namespace_registry_utf8_bytes",
            6,
            5,
        );
        assert!(
            scan_with_limits(
                text,
                ParserLimits {
                    registry_bytes: 6,
                    ..ParserLimits::default()
                }
            )
            .is_ok()
        );
        assert_eq!(statistics(text).distinct_namespaces, 2);
    }

    #[test]
    fn declaration_normalization_preserves_reference_whitespace_and_xml_only_entities() {
        let text = concat!(
            "<Root xmlns:a='urn:a b' xmlns:b='urn:&amp;x'>",
            "<X xmlns:a='urn:a\tb'/><X xmlns:a='urn:a\nb'/>",
            "<X xmlns:a='urn:a\rb'/><X xmlns:a='urn:a\r\nb'/>",
            "<X xmlns:a='urn:a&#32;b'/><X xmlns:a='urn:a&#x20;b'/>",
            "<X xmlns:b='urn:&#38;x'/><X xmlns:a='urn:a&#x9;b'/></Root>"
        );
        // xml, a/space, b/amp, a/referenced TAB.
        assert_eq!(statistics(text).distinct_namespaces, 4);
        let budget =
            scan_with_limits("<Root xmlns:a='urn:&amp;#x9;'/>", ParserLimits::default()).unwrap();
        assert!(
            budget
                .namespaces
                .contains(&(Some("a".into()), "urn:&#x9;".into()))
        );
        for value in ["&nbsp;", "&unknown;", "&#x110000;", "&#xD800;", "&#0;"] {
            assert!(
                scan_with_limits(
                    &format!("<Root xmlns:a='{value}'/>"),
                    ParserLimits::default()
                )
                .is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn redundant_declarations_keep_distinct_identity_and_do_not_count_as_new_pairs() {
        let repeated = "<Child xmlns:a='urn:a'/>".repeat(10_500);
        let text = format!("<Root xmlns:a='urn:a'>{repeated}</Root>");
        let facts = statistics(&text);
        assert_eq!(facts.distinct_namespaces, 2);
        assert_eq!(facts.namespace_references, 21_001);
        assert_eq!(facts.structural_work, 105_005);
        let xml = format!("<Root xmlns:xml='{XML_URI}'/>");
        assert_eq!(statistics(&xml).distinct_namespaces, 1);
        assert_eq!(statistics(&xml).registry_bytes, 0);
        assert_eq!(statistics(&xml).structural_work, 5);
        assert_eq!(
            statistics("<Root xmlns='urn:a' xmlns:a='urn:a'/>").distinct_namespaces,
            3
        );
        assert_eq!(statistics("<Root xmlns=''/>").distinct_namespaces, 2);
    }

    #[test]
    fn unsupported_physical_names_and_prefixed_xmlns_refuse_only_the_new_profile() {
        let schema = SchemaNode::group("Root", vec![]);
        for xml in [
            "<Root><\u{10000}/></Root>",
            "<Root \u{10000}='x'/>",
            "<Root xmlns:\u{10000}='urn:data'/>",
            "<Root><?\u{10000} x?></Root>",
        ] {
            assert!(matches!(
                super::super::from_str_structured(xml, &schema),
                Err(XmlFormatError::StructuredInputPhysicalName)
            ));
        }
        assert!(super::super::super::from_str("<Root><\u{10000}/></Root>", &schema).is_ok());
        assert!(matches!(
            super::super::from_str_structured(
                "<Root xmlns:d='urn:data' d:xmlns='value'/>",
                &schema
            ),
            Err(XmlFormatError::StructuredInputNamespaceAttribute)
        ));
        assert!(matches!(
            super::super::from_str_structured("<Root xmlns:xmlns='urn:data'/>", &schema),
            Err(XmlFormatError::StructuredInputNamespaceAttribute)
        ));
        assert!(super::super::super::from_str("<Root xmlns:xmlns='urn:data'/>", &schema).is_ok());
        assert!(super::super::from_str_structured("<Root xmlns:d='urn:data'/>", &schema).is_ok());
        assert!(super::super::from_str_structured("<Root value='\u{10000}'/>", &schema).is_ok());
        assert!(
            super::super::from_str_structured("<Root><?target \u{10000}?></Root>", &schema).is_ok()
        );
        let scalar = SchemaNode::scalar("Root", ScalarType::String);
        assert!(super::super::from_str_structured("<Root>\u{10000}</Root>", &scalar).is_ok());
    }
}
